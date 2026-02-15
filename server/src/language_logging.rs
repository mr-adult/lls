use std::io::BufReader;

use axum::{
    Json,
    body::Body,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message as WsMessage, WebSocket},
    },
    http::{Response, StatusCode},
};
use lsp_server::Message as LspMessage;
use serde::Deserialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use tracing::{error, info_span};

use crate::AppState;

#[repr(u8)]
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum LspMessageSource {
    Client = 0,
    Server = 1,
}

#[derive(Deserialize)]
struct WrappedLspMessage {
    source: LspMessageSource,
    #[serde(flatten)]
    msg: LspMessage,
}

pub(crate) async fn handle_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response<Body> {
    ws.on_upgrade(|socket| handle_ws_upgrade(socket, state))
}

async fn handle_ws_upgrade(mut socket: WebSocket, state: AppState) {
    // acquire a session from the database
    let session_id_result =
        sqlx::query_scalar::<_, i64>("INSERT INTO sessions DEFAULT VALUES RETURNING id;")
            .fetch_one(&state.db)
            .await;

    let session_id = match session_id_result {
        Ok(session_id) => session_id,
        Err(err) => {
            error!("Failed to get a session_id. Error: {err}");
            // Close the socket. If it errors then the socket was already closed.
            socket.send(WsMessage::Close(None)).await.ok();
            return;
        }
    };

    let session_span = info_span!("session_id", session = session_id);
    let _session_span_handle = session_span.enter();

    while let Some(msg) = socket.recv().await {
        eprintln!("received message");
        let now = OffsetDateTime::now_utc();

        let msg = match msg {
            Err(err) => {
                error!(
                    "Encountered an error in the websocket connection. Error: {}",
                    err
                );
                // client disconnected
                break;
            }
            Ok(msg) => msg,
        };

        let lsp_message_bytes = match &msg {
            WsMessage::Text(utf8_bytes) => utf8_bytes.as_bytes(),
            WsMessage::Binary(bytes) => &bytes,
            WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
            WsMessage::Close(_) => break,
        };

        let msg = match LspMessage::read(&mut BufReader::new(lsp_message_bytes)) {
            Err(_) => {
                error!(
                    "Malformed lsp_message. Contents: {}",
                    str::from_utf8(lsp_message_bytes)
                        .map(|str| str.to_string())
                        .unwrap_or_else(|_| format!("{:?}", lsp_message_bytes))
                );
                continue;
            }
            Ok(None) => continue,
            Ok(Some(parsed)) => parsed,
        };

        log_message(&state.db, msg, Some(session_id), now).await;
    }
}

async fn handle_log(State(state): State<AppState>, Json(msg): Json<LspMessage>) -> StatusCode {
    log_message(&state.db, msg, None, OffsetDateTime::now_utc()).await
}

async fn log_message(
    db: &PgPool,
    msg: LspMessage,
    session_id: Option<i64>,
    received_time: OffsetDateTime,
) -> StatusCode {
    match msg {
        LspMessage::Request(req) => {
            let req_id = sqlx::query_scalar!(
                "INSERT INTO requests (request_id, session_id, method, params, time_stamp) VALUES ($1, $2, $3, $4, $5) RETURNING id;",
                format!("{}", req.id),
                session_id,
                req.method.clone(),
                req.params.clone(),
                received_time
            )
                .fetch_one(db)
                .await;

            if let Err(err) = req_id {
                error!("Failed to log a request to the database. Error: {err}");
                return StatusCode::INTERNAL_SERVER_ERROR;
            }

            return StatusCode::CREATED;
        }
        LspMessage::Notification(not) => {
            let not_id = sqlx::query_scalar!(
                "INSERT INTO notifications (session_id, method, params, time_stamp, source) VALUES ($1, $2, $3, $4, $5) RETURNING id;",
                session_id,
                not.method,
                not.params,
                received_time,
                LspMessageSource::Client as i32
            )
                .fetch_one(db)
                .await;

            if let Err(err) = not_id {
                error!("Failed to log a notification to the database. Error: {err}");
                return StatusCode::INTERNAL_SERVER_ERROR;
            }

            return StatusCode::CREATED;
        }
        LspMessage::Response(resp) => {
            let is_err;
            let error_code;
            let error_message;
            let error_data;
            let result;
            if let Some(err) = resp.error {
                is_err = true;
                error_code = Some(err.code);
                error_message = Some(err.message);
                error_data = err.data;
                result = None;
            } else if let Some(res) = resp.result {
                is_err = false;
                error_code = None;
                error_message = None;
                error_data = None;
                result = Some(res);
            } else {
                is_err = false;
                error_code = None;
                error_message = None;
                error_data = None;
                result = None;
            }

            let resp_id = sqlx::query_scalar!(
                "INSERT INTO responses (session_id, is_error, result, error_code, error_message, error_data, time_stamp) VALUES ($1, $2, $3, $4, $5, $6, $7);",
                session_id,
                is_err,
                result,
                error_code,
                error_message,
                error_data,
                received_time
            )
                .fetch_optional(db)
                .await;

            if let Err(err) = resp_id {
                error!("Failed to log a response to the database. Error: {err}");
                return StatusCode::INTERNAL_SERVER_ERROR;
            }

            return StatusCode::CREATED;
        }
    }
}
