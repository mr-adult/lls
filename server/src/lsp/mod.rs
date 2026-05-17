use std::sync::Arc;

use axum::{
    body::Body,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message as AxumWsMessage, WebSocket},
    },
    http::Response as HttpResponse,
};
use futures::StreamExt;
use lsp_types::{InitializeParams, InitializeResult, InitializedParams};
use time::OffsetDateTime;
use tokio::{
    net::TcpStream,
    sync::{Mutex, RwLock},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tracing::{error, info_span};

use crate::{
    AppState,
    lsp::{
        client_to_server_proxy::ClientToServerProxy, server_to_client_proxy::ServerToClientProxy,
    },
};

mod client_to_server_proxy;
mod lsp_session;
use lsp_session::LspSession;
mod server_to_client_proxy;

struct Sockets {
    client: WebSocket,
    server: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

pub(crate) async fn handle_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> HttpResponse<Body> {
    ws.on_upgrade(|socket| handle_ws_upgrade(socket, state))
}

pub(crate) async fn handle_ws_upgrade(mut client_socket: WebSocket, state: AppState) {
    let session_start = OffsetDateTime::now_utc();

    // acquire a session from the database
    let session_id_result = sqlx::query_scalar::<_, i64>(
        "INSERT INTO sessions (start_time_stamp, end_time_stamp) VALUES ($1, NULL) RETURNING id;",
    )
    .bind(session_start)
    .fetch_one(&state.db)
    .await;

    let session_id = match session_id_result {
        Ok(session_id) => session_id,
        Err(err) => {
            error!("Failed to get a session_id. Error: {err}");
            // Close the socket. If it errors then the socket was already closed.
            client_socket.send(AxumWsMessage::Close(None)).await.ok();
            return;
        }
    };

    let session_span = info_span!("session_id", session_id = session_id);
    let _session_span_handle = session_span.enter();

    let server_socket = match connect_async(state.forward_url.as_ref()).await {
        Ok((ws, _)) => ws,
        Err(err) => {
            error!("Failed to connect to the server. Error: {err}");
            // Close the socket. If it errors then the socket was already closed.
            client_socket.send(AxumWsMessage::Close(None)).await.ok();
            return;
        }
    };

    let sockets = Sockets {
        client: client_socket,
        server: server_socket,
    };

    main_loop(sockets, state.clone(), session_id).await;

    if let Err(err) = sqlx::query("UPDATE sessions SET end_time_stamp = $1 WHERE id = $2")
        .bind(OffsetDateTime::now_utc())
        .bind(session_id)
        .fetch_optional(&state.db)
        .await
    {
        error!("Failed to write the end_time_stamp. Message: {}", err);
    }
}

async fn main_loop(sockets: Sockets, state: AppState, session_id: i64) {
    let (client_sink, client_source) = sockets.client.split();
    let (server_sink, server_source) = sockets.server.split();
    let uninitialized_state = Arc::new(RwLock::new(Uninitialized {
        initialize_params: None,
        initialize_result: None,
        initialized_params: None,
        failed: false,
    }));

    let client_sink = Arc::new(Mutex::new(client_sink));
    let server_sink = Arc::new(Mutex::new(server_sink));
    let shared_session_state = LspSession::new(session_id, &state.db);

    let c_to_s_proxy = ClientToServerProxy::new(
        client_source,
        server_sink.clone(),
        client_sink.clone(),
        uninitialized_state.clone(),
        &shared_session_state,
    );

    let c_to_s_lifecycle = async move {
        match c_to_s_proxy.initialize().await {
            Ok(initialized) => match initialized.run_proxy().await {
                Ok(shutdown) => shutdown.listen_for_exit().await,
                Err(()) => {}
            },
            Err(()) => {}
        };
    };

    let s_to_c_proxy = ServerToClientProxy::new(
        server_source,
        client_sink,
        server_sink,
        uninitialized_state,
        &shared_session_state,
    );

    let s_to_c_lifecycle = async move {
        match s_to_c_proxy.initialize().await {
            Ok(initialized) => initialized.run_proxy().await,
            Err(()) => {}
        }
    };

    tokio::select! {
        _ = s_to_c_lifecycle => {},
        _ = c_to_s_lifecycle => {}
    };
}

#[derive(Default)]
struct Uninitialized {
    initialize_params: Option<InitializeParams>,
    initialize_result: Option<InitializeResult>,
    initialized_params: Option<InitializedParams>,
    failed: bool,
}

struct Shutdown;
