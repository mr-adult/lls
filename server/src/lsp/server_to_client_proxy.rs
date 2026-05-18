use std::{io::BufReader, sync::Arc};

use axum::extract::ws::{Message as AxumWsMessage, WebSocket};
use futures::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use lsp_types::{
    InitializeParams, InitializeResult, InitializedParams,
    notification::{Initialized, Notification as LspNotification},
    request::{Initialize, Request as LspRequest, ShowMessageRequest},
};
use time::{Duration, OffsetDateTime};
use tokio::{
    net::TcpStream,
    sync::{Mutex, RwLock},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{self, Message as TokioWsMessage},
};
use tracing::{error, warn};

use crate::{
    lsp::{LspSession, Shutdown, Uninitialized},
    message::{ErrorCode, Message as LspMessage, MessageKind, Notification, Request, Response},
    session::{ExpectedSender, MessageSource},
};

pub(super) struct ServerToClientProxy<'session, State> {
    source: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    client_sink: Arc<Mutex<SplitSink<WebSocket, AxumWsMessage>>>,
    server_sink: Arc<Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, TokioWsMessage>>>,
    state: State,
    session: &'session LspSession<'session>,
}

impl<'session, State> ServerToClientProxy<'session, State> {
    fn parse_message(message: TokioWsMessage) -> Result<Option<LspMessage>, ()> {
        let lsp_message_bytes = match &message {
            TokioWsMessage::Text(utf8_bytes) => utf8_bytes.as_bytes(),
            TokioWsMessage::Binary(bytes) => &bytes,
            TokioWsMessage::Ping(_) | TokioWsMessage::Pong(_) => return Ok(None),
            TokioWsMessage::Close(_) => return Ok(None),
            TokioWsMessage::Frame(_) => return Ok(None),
        };

        match LspMessage::read(&mut BufReader::new(lsp_message_bytes)) {
            Err(_) => {
                error!(
                    "Malformed lsp_message from client. Contents: {}",
                    str::from_utf8(lsp_message_bytes)
                        .map(|str| str.to_string())
                        .unwrap_or_else(|_| format!("{:?}", lsp_message_bytes))
                );
                Err(())
            }
            Ok(None) => Ok(None),
            Ok(Some(parsed)) => Ok(Some(parsed)),
        }
    }

    async fn proxy_request(&mut self, request: Request) -> Result<(), axum::Error> {
        let mut raw_message = Vec::new();
        LspMessage::Request(request.clone())
            .write(&mut raw_message)
            .expect("writing to a vec to never fail");

        let raw_message = AxumWsMessage::Text(
            raw_message
                .try_into()
                .expect("raw_message to always contain UTF-8 bytes"),
        );

        async { self.client_sink.lock().await.send(raw_message).await }
            .await
            .map_err(|err| {
                error!("Failed to proxy a request to the client. Error: {err}");
                err
            })
    }

    async fn proxy_response(&mut self, response: Response) -> Result<(), axum::Error> {
        let mut raw_message = Vec::new();
        LspMessage::Response(response.clone())
            .write(&mut raw_message)
            .expect("writing to a vec to never fail");

        let raw_message = AxumWsMessage::Text(
            raw_message
                .try_into()
                .expect("raw_message to always contain UTF-8 bytes"),
        );

        async { self.client_sink.lock().await.send(raw_message).await }
            .await
            .map_err(|err| {
                error!("Failed to proxy a response to the server. Error: {err}");
                err
            })
    }

    async fn send_response_skipping_client(
        &mut self,
        response: Response,
        received_time: OffsetDateTime,
    ) -> Result<(), tungstenite::Error> {
        let mut raw_message = Vec::new();
        LspMessage::Response(response.clone())
            .write(&mut raw_message)
            .expect("writing to a vec to never fail");

        let raw_message = TokioWsMessage::Text(
            raw_message
                .try_into()
                .expect("raw_message to always contain UTF-8 bytes"),
        );

        let send_future = async {
            async { self.server_sink.lock().await.send(raw_message).await }
                .await
                .map_err(|err| {
                    error!("Failed to send an interrupt response back to the client. Error: {err}");
                    err
                })
        };

        let log_future = async {
            self.session
                .log_response(response, MessageSource::Proxy, received_time)
                .await
                .ok();
        };

        tokio::join!(send_future, log_future).0
    }

    async fn proxy_notification(&mut self, notification: Notification) -> Result<(), axum::Error> {
        let mut raw_message = Vec::new();
        LspMessage::Notification(notification.clone())
            .write(&mut raw_message)
            .expect("writing to a vec to never fail");

        let raw_message = AxumWsMessage::Text(
            raw_message
                .try_into()
                .expect("raw_message to always contain UTF-8 bytes"),
        );

        self.client_sink.lock().await.send(raw_message).await
    }

    fn validate_message_kind_and_sender(kind: MessageKind, method: &str) {
        LspSession::validate_message_kind(kind, method);
        match LspSession::get_expected_sender(kind, method) {
            None => {
                /* assume that this branch will already be coverd by message kind validation. */
            }
            Some(ExpectedSender::Client) => {
                error!(
                    "Expected `{method}` request to be sent by the client, but received one from the server."
                );
            }
            Some(ExpectedSender::Unknown) => {
                warn!("Received unknown `{method}` message method type.");
            }
            Some(ExpectedSender::Server) | Some(ExpectedSender::Either) => {}
        }
    }
}

impl<'session> ServerToClientProxy<'session, Arc<RwLock<Uninitialized>>> {
    pub(super) fn new(
        source: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
        client_sink: Arc<Mutex<SplitSink<WebSocket, AxumWsMessage>>>,
        server_sink: Arc<
            Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, TokioWsMessage>>,
        >,
        state: Arc<RwLock<Uninitialized>>,
        session: &'session LspSession<'session>,
    ) -> Self {
        Self {
            source,
            client_sink,
            server_sink,
            state: state,
            session,
        }
    }

    pub(super) async fn initialize(
        mut self,
    ) -> Result<ServerToClientProxy<'session, InitializedState>, ()> {
        while let Some(client_msg) = self.source.next().await {
            let received_time = OffsetDateTime::now_utc();

            if *self.session.exited.read().await {
                return Err(());
            }

            let msg = match client_msg {
                Err(err) => {
                    error!(
                        "Encountered an error in the websocket connection. Error: {}",
                        err
                    );
                    // client disconnected
                    *self.session.exited.write().await = true;
                    return Err(());
                }
                Ok(msg) => msg,
            };

            let message = match Self::parse_message(msg) {
                Err(()) | Ok(None) => continue,
                Ok(Some(message)) => message,
            };

            {
                let read_lock = self.state.read().await;
                if let Some(initialized) = read_lock.initialized_params {
                    if read_lock.failed {
                        *self.session.exited.write().await = true;
                        return Err(());
                    }

                    return Ok(ServerToClientProxy {
                        source: self.source,
                        client_sink: self.client_sink,
                        server_sink: self.server_sink,
                        state: InitializedState {
                            initialize_params: read_lock
                                .initialize_params
                                .clone()
                                .unwrap_or_default(),
                            initialize_result: read_lock
                                .initialize_result
                                .clone()
                                .unwrap_or_default(),
                            initialized_params: initialized,
                            peeked: Some((message, received_time)),
                        },
                        session: self.session,
                    });
                }
            }

            match message {
                LspMessage::Request(request) => {
                    let span = self
                        .session
                        .start_request(request.clone(), MessageSource::Server, received_time)
                        .await;
                    let _handle = span.as_ref().map(|span| span.enter());

                    Self::validate_message_kind_and_sender(
                        MessageKind::Request,
                        request.method.as_str(),
                    );

                    match request.method.as_str() {
                        ShowMessageRequest::METHOD => {}
                        _ => {
                            match self.send_response_skipping_client(Response::new_err(request.id.clone(), ErrorCode::RequestFailed as i32, format!("Requests with method `{}` cannot be handled before the initialization handshake is completed.", 
                        request.method.as_str())), received_time).await {
                            Ok(()) => continue,
                            Err(_) => {
                    *self.session.exited.write().await = true;
                                return Err(());
                            }
                        };
                        }
                    }

                    match self.proxy_request(request).await {
                        Ok(()) => {}
                        Err(err) => {
                            error!("Failed to proxy a request to the client. Error: {err}");
                            *self.session.exited.write().await = true;
                            return Err(());
                        }
                    }
                }
                LspMessage::Response(response) => {
                    let response_id_and_span = self
                        .session
                        .start_response(response.clone(), MessageSource::Server, received_time)
                        .await;
                    let _handle = response_id_and_span.as_ref().map(|span| span.1.enter());

                    match self.session.get_request_for_response(&response).await {
                        Err(_) | Ok(None) => {}
                        Ok(Some((_, request))) => {
                            Self::validate_message_kind_and_sender(
                                MessageKind::Response,
                                request.method.as_str(),
                            );

                            match request.method.as_str() {
                                Initialize::METHOD => {
                                    let mut write_lock = self.state.write().await;
                                    if write_lock.initialized_params.is_some() {
                                        error!(
                                            "Received an `{}` response after initialization was completed.",
                                            Initialize::METHOD
                                        );
                                        continue;
                                    }

                                    if let Some(_) = response.error {
                                        write_lock.initialize_params = None;
                                    } else if let Some(result) = response.result.clone() {
                                        if let Ok(initialize_result) =
                                            serde_json::from_value::<InitializeResult>(result)
                                        {
                                            write_lock.initialize_result = Some(initialize_result);
                                        }
                                    }
                                }
                                ShowMessageRequest::METHOD => {}
                                other => {
                                    error!(
                                        "Received response for `{other}` request before initialization was complete."
                                    );
                                }
                            }

                            LspSession::validate_response(
                                request.method.as_str(),
                                response.clone(),
                            );
                        }
                    };

                    match self.proxy_response(response).await {
                        Ok(()) => {}
                        Err(err) => {
                            error!("Failed to proxy a response to the client. Error: {err}");
                            *self.session.exited.write().await = true;
                            return Err(());
                        }
                    };
                }
                LspMessage::Notification(notification) => {
                    let span = self
                        .session
                        .start_notification(
                            notification.clone(),
                            MessageSource::Server,
                            received_time,
                        )
                        .await;
                    let _handle = span.as_ref().map(|span| span.enter());

                    Self::validate_message_kind_and_sender(
                        MessageKind::Notification,
                        notification.method.as_str(),
                    );

                    match notification.method.as_str() {
                        Initialized::METHOD => {
                            error!(
                                "Server sent a `{}` notification. This should only be sent by the client.",
                                Initialized::METHOD
                            );
                        }
                        other => {
                            error!(
                                "received a `{}` notification before initialization was completed.",
                                other
                            );
                        }
                    }

                    match self.proxy_notification(notification).await {
                        Ok(()) => {}
                        Err(err) => {
                            error!("Failed to proxy a notification to the server. Error: {err}");
                            *self.session.exited.write().await = true;
                            return Err(());
                        }
                    }
                }
            }
        }

        *self.session.exited.write().await = true;
        Err(())
    }
}

impl<'session> ServerToClientProxy<'session, InitializedState> {
    pub(super) async fn run_proxy(mut self) {
        while let Some(protocol_result) = self.get_next_message().await {
            let message_parse_result = match protocol_result {
                Err(()) => break,
                Ok(message_parse_result) => message_parse_result,
            };

            if *self.session.exited.read().await {
                return;
            }

            let (message, received_time) = match message_parse_result {
                Err(()) => continue,
                Ok(message) => message,
            };

            match message {
                LspMessage::Request(request) => {
                    let span = self
                        .session
                        .start_request(request.clone(), MessageSource::Server, received_time)
                        .await;
                    let _handle = span.as_ref().map(|span| span.enter());

                    Self::validate_message_kind_and_sender(
                        MessageKind::Request,
                        request.method.as_str(),
                    );

                    match LspSession::validate_request_params(request.clone()).await {
                        Ok(()) => {}
                        Err(response) => {
                            match self
                                .send_response_skipping_client(
                                    response,
                                    received_time - Duration::milliseconds(1),
                                )
                                .await
                            {
                                Ok(()) => continue,
                                Err(err) => {
                                    error!(
                                        "Failed to send interrupt response back to client. Error: {err}"
                                    );
                                    *self.session.exited.write().await = true;
                                    return;
                                }
                            };
                        }
                    };

                    match self.proxy_request(request).await {
                        Ok(()) => {}
                        Err(err) => {
                            error!("Failed to proxy a request to the server. Error: {err}");
                            *self.session.exited.write().await = true;
                            return;
                        }
                    };
                }
                LspMessage::Response(response) => {
                    let request_id_and_span = self
                        .session
                        .start_response(response.clone(), MessageSource::Server, received_time)
                        .await;

                    let mut request_id = None;
                    let _handle = request_id_and_span.as_ref().map(|request_id_and_span| {
                        request_id = Some(request_id_and_span.0);
                        request_id_and_span.1.enter()
                    });

                    if let Some(db_id) = request_id {
                        match self
                            .session
                            .get_request_from_id(db_id, response.id.clone())
                            .await
                        {
                            Err(_) => {}
                            Ok(request) => {
                                Self::validate_message_kind_and_sender(
                                    MessageKind::Response,
                                    request.method.as_str(),
                                );

                                LspSession::validate_response(
                                    request.method.as_str(),
                                    response.clone(),
                                );
                            }
                        };
                    }

                    match self.proxy_response(response).await {
                        Ok(()) => {}
                        Err(err) => {
                            error!("Failed to proxy a response to the client. Error: {err}");
                            *self.session.exited.write().await = true;
                            return;
                        }
                    }
                }
                LspMessage::Notification(notification) => {
                    let span = self
                        .session
                        .start_notification(
                            notification.clone(),
                            MessageSource::Server,
                            received_time,
                        )
                        .await;
                    let _handle = span.as_ref().map(|span| span.enter());

                    Self::validate_message_kind_and_sender(
                        MessageKind::Notification,
                        notification.method.as_str(),
                    );
                    LspSession::validate_notification_params(notification.clone());

                    match self.proxy_notification(notification).await {
                        Ok(()) => {}
                        Err(err) => {
                            error!("Failed to proxy a response to the client. Error: {err}");
                            *self.session.exited.write().await = true;
                            return;
                        }
                    }
                }
            }
        }

        *self.session.exited.write().await = true;
    }

    async fn get_next_message(
        &mut self,
    ) -> Option<Result<Result<(LspMessage, OffsetDateTime), ()>, ()>> {
        if let Some(message) = self.state.peeked.take() {
            Some(Ok(Ok(message)))
        } else if let Some(client_msg) = self.source.next().await {
            let received_time = OffsetDateTime::now_utc();

            let msg = match client_msg {
                Err(err) => {
                    error!(
                        "Encountered an error in the websocket connection. Error: {}",
                        err
                    );
                    // client disconnected
                    return Some(Err(()));
                }
                Ok(msg) => msg,
            };

            let message = match Self::parse_message(msg) {
                Err(()) | Ok(None) => return Some(Ok(Err(()))),
                Ok(Some(message)) => message,
            };

            Some(Ok(Ok((message, received_time))))
        } else {
            None
        }
    }
}

pub(super) struct InitializedState {
    initialize_params: InitializeParams,
    initialize_result: InitializeResult,
    initialized_params: InitializedParams,
    peeked: Option<(LspMessage, OffsetDateTime)>,
}
