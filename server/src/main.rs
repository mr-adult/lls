use std::io::BufReader;

use axum::{
    Json, Router, body::Body, extract::{
        State, WebSocketUpgrade,
        ws::{Message as WsMessage, WebSocket},
    }, http::{Response, StatusCode}, routing::any
};
use lsp_server::Message as LspMessage;
use serde::Deserialize;
use sqlx::{PgPool, postgres::PgPoolOptions};
use time::OffsetDateTime;
use tokio::net::TcpListener;
use tracing::{error, info_span};
use tracing_subscriber::{
    EnvFilter,
    {layer::SubscriberExt, util::SubscriberInitExt},
};

use crate::logging::PostgresLayer;

mod logging;

#[derive(Clone)]
struct AppState {
    db: PgPool,
}

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

async fn handle_ws(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response<Body> {
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
                error!("Malformed lsp_message. Contents: {}", str::from_utf8(lsp_message_bytes).map(|str| str.to_string()).unwrap_or_else(|_| format!("{:?}", lsp_message_bytes)));
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

async fn log_message(db: &PgPool, msg: LspMessage, session_id: Option<i64>, received_time: OffsetDateTime) -> StatusCode {
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

#[tokio::main]
pub async fn main() {
    println!("Starting Up...");

    // First, parse the .env file for our environment setup.
    dotenvy::dotenv().ok();

    // We create a single connection pool for SQLx that's shared across the whole application.
    // This saves us from opening a new connection for every API call, which is wasteful.
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    eprintln!("DB URL: {}", database_url);
    let pool = PgPoolOptions::new()
        // The default connection limit for a Postgres server is 100 connections, minus 3 for superusers.
        // We should leave some connections available for manual access.
        //
        // If you're deploying your application with multiple replicas, then the total
        // across all replicas should not exceed the Postgres connection limit.
        .max_connections(10)
        .connect(&database_url)
        .await
        .unwrap_or_else(|err| panic!("Could not connect to dabase_url. Error: \n{}", err));

    // Run any SQL migrations to get the DB into the correct state
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .unwrap_or_else(|err| panic!("Failed to migrate the database. Error: \n{}", err));

    tracing_subscriber::registry()
        .with(
            EnvFilter::from_default_env()
                // this directive prevent sqlx from infinitely logggin its own events.
                .add_directive("lls".parse().unwrap()),
        )
        .with(PostgresLayer::from(pool.clone()))
        .init();

    let router = Router::new()
        .route("/ws", any(handle_ws))
        // FUTURE: handle regular POST requests. Need to create an API to retrieve a session ID first.
        // .route("/log", post(handle_log))
        .with_state(AppState { db: pool })
        .into_make_service();

    let port: u16 = 8080;
    let tcp_listener = TcpListener::bind(&format!("[::]:{port}"))
        .await
        .expect(&format!("failed to bind to [::]:{port}"));
    println!("Listening on: [::]:{port}");

    axum::serve(tcp_listener, router)
        .await
        .expect("failed to start service");
}
