use std::{fmt::Display, sync::Arc};

use axum::{
    body::Body,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message as AxumWsMessage, WebSocket},
    },
    http::Response as HttpResponse,
};
use futures::{Sink, SinkExt, Stream, StreamExt};
use lsp_types::{InitializeParams, InitializeResult, InitializedParams};
use sqlx::PgPool;
use time::OffsetDateTime;
use tokio::sync::{Mutex, RwLock};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{self, Message as TokioWsMessage},
};
use tracing::{error, info_span};

use crate::AppState;

mod client_to_server_proxy;
mod lsp_session;
use lsp_session::LspSession;
mod server_to_client_proxy;

pub(crate) async fn handle_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> HttpResponse<Body> {
    ws.on_upgrade(|socket| handle_ws_upgrade(socket, state))
}

async fn handle_ws_upgrade(client_socket: WebSocket, state: AppState) {
    run_session_with_client(client_socket, state).await.ok();
}

pub(crate) async fn run_session_with_client<ClientStream, ClientError>(
    mut client_stream: ClientStream,
    state: AppState,
) -> Result<i64, ()>
where
    ClientStream: Stream<Item = Result<AxumWsMessage, ClientError>> + Sink<AxumWsMessage> + Unpin,
    ClientStream::Error: Display,
    ClientError: Display,
{
    let session_id = match start_session(&state.db).await {
        Ok(session_id) => session_id,
        Err(_) => {
            // Close the socket. If it errors then the socket was already closed.
            client_stream.send(AxumWsMessage::Close(None)).await.ok();
            return Err(());
        }
    };

    let session_span = info_span!("session_id", session_id = session_id);
    let _session_span_handle = session_span.enter();

    let server_socket = match connect_async(state.forward_url.as_ref()).await {
        Ok((ws, _)) => ws,
        Err(err) => {
            error!("Failed to connect to the server. Error: {err}");
            // Close the socket. If it errors then the socket was already closed.
            client_stream.send(AxumWsMessage::Close(None)).await.ok();
            return Ok(session_id);
        }
    };

    main_loop(server_socket, client_stream, state.clone(), session_id).await;

    if let Err(err) = sqlx::query("UPDATE sessions SET end_time_stamp = $1 WHERE id = $2")
        .bind(OffsetDateTime::now_utc())
        .bind(session_id)
        .fetch_optional(&state.db)
        .await
    {
        error!("Failed to write the end_time_stamp. Message: {}", err);
    }

    Ok(session_id)
}

pub(crate) async fn run_session<ServerStream, ClientStream>(
    server_stream: ServerStream,
    mut client_stream: ClientStream,
    state: AppState,
) -> Result<i64, ()>
where
    ClientStream: Stream<Item = Result<AxumWsMessage, axum::Error>> + Sink<AxumWsMessage> + Unpin,
    ClientStream::Error: Display,
    ServerStream:
        Stream<Item = Result<TokioWsMessage, tungstenite::Error>> + Sink<TokioWsMessage> + Unpin,
    ServerStream::Error: Display,
{
    let session_id = match start_session(&state.db).await {
        Ok(session_id) => session_id,
        Err(_) => {
            // Close the socket. If it errors then the socket was already closed.
            client_stream.send(AxumWsMessage::Close(None)).await.ok();
            return Err(());
        }
    };

    let session_span = info_span!("session_id", session_id = session_id);
    let _session_span_handle = session_span.enter();

    main_loop(server_stream, client_stream, state.clone(), session_id).await;

    if let Err(err) = sqlx::query("UPDATE sessions SET end_time_stamp = $1 WHERE id = $2")
        .bind(OffsetDateTime::now_utc())
        .bind(session_id)
        .fetch_optional(&state.db)
        .await
    {
        error!("Failed to write the end_time_stamp. Message: {}", err);
    }

    Ok(session_id)
}

pub(crate) async fn start_session(db: &PgPool) -> Result<i64, sqlx::Error> {
    let session_start = OffsetDateTime::now_utc();

    // acquire a session from the database
    let session_id_result = sqlx::query_scalar::<_, i64>(
        "INSERT INTO sessions (start_time_stamp, end_time_stamp) VALUES ($1, NULL) RETURNING id;",
    )
    .bind(session_start)
    .fetch_one(db)
    .await;

    match session_id_result {
        Ok(session_id) => Ok(session_id),
        Err(err) => {
            error!("Failed to get a session_id. Error: {err}");
            return Err(err);
        }
    }
}

pub(crate) async fn main_loop<ServerStream, ClientStream, ClientError>(
    server_socket: ServerStream,
    client_stream: ClientStream,
    state: AppState,
    session_id: i64,
) where
    ClientStream: Stream<Item = Result<AxumWsMessage, ClientError>> + Sink<AxumWsMessage> + Unpin,
    ClientError: Display,
    ClientStream::Error: Display,
    ServerStream:
        Stream<Item = Result<TokioWsMessage, tungstenite::Error>> + Sink<TokioWsMessage> + Unpin,
    ServerStream::Error: Display,
{
    let (client_sink, client_source) = client_stream.split();
    let (server_sink, server_source) = server_socket.split();
    let uninitialized_state = Arc::new(RwLock::new(Uninitialized {
        initialize_params: None,
        initialize_result: None,
        initialized_params: None,
        failed: false,
    }));

    let client_sink = Arc::new(Mutex::new(client_sink));
    let server_sink = Arc::new(Mutex::new(server_sink));
    let shared_session_state = LspSession::new(session_id, &state.db);

    let c_to_s_lifecycle = client_to_server_proxy::run_to_exit(
        client_source,
        server_sink.clone(),
        client_sink.clone(),
        uninitialized_state.clone(),
        &shared_session_state,
    );

    let s_to_c_lifecycle = server_to_client_proxy::run_to_exit(
        server_source,
        client_sink,
        server_sink,
        uninitialized_state,
        &shared_session_state,
    );

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
