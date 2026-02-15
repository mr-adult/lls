use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Html,
};
use serde::Deserialize;

use crate::{AppState, html::chat_view::append_chat_html_to};

mod chat_view;

#[derive(Deserialize)]
pub(crate) struct GetSessionParams {
    session_id: i64,
}

pub(crate) async fn get_session(
    State(state): State<AppState>,
    Query(GetSessionParams {
        session_id,
    }): Query<GetSessionParams>,
) -> Result<Html<String>, StatusCode> {
    let _session = sqlx::query!("SELECT * FROM sessions WHERE id = $1 LIMIT 1;", session_id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let all_messages =
        crate::session::get_all_messages_for_session_in_chronological_order(&state.db, session_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut html = String::new();
    html.push_str("<!DOCTYPE=html>");
    html.push_str("<html>");

    html.push_str("<head>");
    html.push_str("<meta charset=\"UTF-8\"/>");
    html.push_str("<title>LSP Analyzer</title>");
    html.push_str("</head>");

    html.push_str("<body>");
    html.push_str("<style>");
    html.push_str(include_str!("../css/chat.css"));
    html.push_str("</style>");

    append_chat_html_to(&mut html, &all_messages);

    html.push_str("</body>");
    html.push_str("</html>");

    Ok(Html(html))
}
