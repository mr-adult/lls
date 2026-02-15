use axum::{
    Router,
    routing::{any, get},
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::net::TcpListener;
use tracing_subscriber::{
    EnvFilter,
    {layer::SubscriberExt, util::SubscriberInitExt},
};

use crate::error_logging::PostgresLayer;

mod error_logging;
mod html;
mod language_logging;
mod message;
mod session;
mod utils;

#[derive(Clone)]
struct AppState {
    db: PgPool,
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
        .route("/", get(html::session_search::get_sessions))
        .route("/ws", any(language_logging::handle_ws))
        .route("/session", get(html::get_session))
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
