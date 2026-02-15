use std::collections::HashSet;

use axum::{
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, Response, StatusCode},
    response::Html,
};
use serde::Deserialize;

use crate::{
    AppState,
    html::chat_view::append_chat_html_to,
    message::{Conversation, MessageKind, classify},
};

mod chat_view;
pub(crate) mod session_search;

#[derive(Deserialize)]
pub(crate) struct GetSessionParams {
    session_id: i64,
    life_cycle: Option<bool>,
    document_synchronization: Option<bool>,
    notebook_synchronization: Option<bool>,
    workspace_synchronization: Option<bool>,
    workspace: Option<bool>,
    telemetry: Option<bool>,
    declaration: Option<bool>,
    definition: Option<bool>,
    type_definition: Option<bool>,
    implementation: Option<bool>,
    references: Option<bool>,
    call_hierarchy: Option<bool>,
    type_hierarchy: Option<bool>,
    document_highlight: Option<bool>,
    document_link: Option<bool>,
    hover: Option<bool>,
    code_lens: Option<bool>,
    folding_range: Option<bool>,
    selection: Option<bool>,
    symbol: Option<bool>,
    semantic_tokens: Option<bool>,
    inlay_hint: Option<bool>,
    inline_value: Option<bool>,
    moniker: Option<bool>,
    completion: Option<bool>,
    diagnostic: Option<bool>,
    signature_help: Option<bool>,
    code_action: Option<bool>,
    document_color: Option<bool>,
    formatting: Option<bool>,
    rename: Option<bool>,
    linked_editing_range: Option<bool>,
    execute_command: Option<bool>,
    uncategorized: Option<bool>,
}

impl GetSessionParams {
    fn build_message_classification_allow_list(&self) -> HashSet<Option<MessageKind>> {
        let request = self;

        let show_all = request.life_cycle.is_none()
            && request.document_synchronization.is_none()
            && request.notebook_synchronization.is_none()
            && request.workspace_synchronization.is_none()
            && request.workspace.is_none()
            && request.telemetry.is_none()
            && request.declaration.is_none()
            && request.definition.is_none()
            && request.type_definition.is_none()
            && request.implementation.is_none()
            && request.references.is_none()
            && request.call_hierarchy.is_none()
            && request.type_hierarchy.is_none()
            && request.document_highlight.is_none()
            && request.document_link.is_none()
            && request.hover.is_none()
            && request.code_lens.is_none()
            && request.folding_range.is_none()
            && request.selection.is_none()
            && request.symbol.is_none()
            && request.semantic_tokens.is_none()
            && request.inlay_hint.is_none()
            && request.inline_value.is_none()
            && request.moniker.is_none()
            && request.completion.is_none()
            && request.diagnostic.is_none()
            && request.signature_help.is_none()
            && request.code_action.is_none()
            && request.document_color.is_none()
            && request.formatting.is_none()
            && request.rename.is_none()
            && request.linked_editing_range.is_none()
            && request.execute_command.is_none()
            && request.uncategorized.is_none();

        let mut msg_types_to_include = HashSet::new();
        if show_all || matches!(request.life_cycle, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::Lifecycle));
        }
        if show_all || matches!(request.document_synchronization, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::TextDocumentSynchronization));
        }
        if show_all || matches!(request.notebook_synchronization, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::NotebookDocumentSynchronization));
        }
        if show_all || matches!(request.workspace_synchronization, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::WorkspaceSynchronization));
        }
        if show_all || matches!(request.workspace, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::Workspace));
        }
        if show_all || matches!(request.telemetry, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::Telemetry));
        }
        if show_all || matches!(request.declaration, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::Declaration));
        }
        if show_all || matches!(request.definition, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::Definition));
        }
        if show_all || matches!(request.type_definition, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::TypeDefinition));
        }
        if show_all || matches!(request.implementation, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::Implementation));
        }
        if show_all || matches!(request.references, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::References));
        }
        if show_all || matches!(request.call_hierarchy, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::CallHierarchy));
        }
        if show_all || matches!(request.type_hierarchy, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::TypeHierarchy));
        }
        if show_all || matches!(request.document_highlight, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::DocumentHighlight));
        }
        if show_all || matches!(request.document_link, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::DocumentLink));
        }
        if show_all || matches!(request.hover, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::Hover));
        }
        if show_all || matches!(request.code_lens, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::CodeLens));
        }
        if show_all || matches!(request.folding_range, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::FoldingRange));
        }
        if show_all || matches!(request.selection, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::Selection));
        }
        if show_all || matches!(request.symbol, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::Symbol));
        }
        if show_all || matches!(request.semantic_tokens, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::SemanticTokens));
        }
        if show_all || matches!(request.inlay_hint, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::InlayHint));
        }
        if show_all || matches!(request.inline_value, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::InlineValue));
        }
        if show_all || matches!(request.moniker, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::Moniker));
        }
        if show_all || matches!(request.completion, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::Completion));
        }
        if show_all || matches!(request.diagnostic, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::Diagnostic));
        }
        if show_all || matches!(request.signature_help, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::SignatureHelp));
        }
        if show_all || matches!(request.code_action, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::CodeAction));
        }
        if show_all || matches!(request.document_color, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::DocumentColor));
        }
        if show_all || matches!(request.formatting, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::Formatting));
        }
        if show_all || matches!(request.rename, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::Rename));
        }
        if show_all || matches!(request.linked_editing_range, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::LinkedEditingRange));
        }
        if show_all || matches!(request.execute_command, Some(true)) {
            msg_types_to_include.insert(Some(MessageKind::ExecuteCommand));
        }
        if show_all || matches!(request.uncategorized, Some(true)) {
            msg_types_to_include.insert(None);
        }
        msg_types_to_include
    }
}

pub(crate) async fn get_session(
    State(state): State<AppState>,
    Query(request): Query<GetSessionParams>,
) -> Result<(StatusCode, HeaderMap, Html<String>), StatusCode> {
    let session = sqlx::query!(
        "SELECT id, end_time_stamp FROM sessions WHERE id = $1 LIMIT 1;",
        request.session_id
    )
    .fetch_one(&state.db)
    .await
    .map_err(|err| match err {
        sqlx::Error::RowNotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    })?;

    let conversation = crate::session::get_all_messages_for_session_in_chronological_order(
        &state.db,
        request.session_id,
    )
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

    html.push_str(&generate_filtering_form(&request, &conversation));
    let allow_list = request.build_message_classification_allow_list();
    append_chat_html_to(&mut html, &conversation, &allow_list);

    html.push_str("</body>");
    html.push_str("</html>");

    // if the session has been ended, it won't change. Set long-lived caching headers
    let mut headers = HeaderMap::new();
    if session.end_time_stamp.is_some() {
        headers.insert("immutable", HeaderValue::from_str("true").unwrap());
    } else {
        headers.insert("no-store", HeaderValue::from_str("true").unwrap());
    }

    Ok((StatusCode::OK, headers, Html(html)))
}

fn generate_filtering_form(request: &GetSessionParams, conversation: &Conversation) -> String {
    let message_types_in_conversation = conversation
        .messages()
        .into_iter()
        .map(|message_with_time_stamp| classify(&message_with_time_stamp.message, conversation))
        .collect::<HashSet<_>>();

    let allow_list = request.build_message_classification_allow_list();

    let mut html = String::new();

    html.push_str("<form action=\"/session\" method=\"GET\" style=\"display: flex;flex-direction: column;align-items: center; background-color: gray; border-radius: 40px; padding: 20px; row-gap: 5px;\">");
    html.push_str("<h2>Filter Your Results</h2>");
    html.push_str("<fieldset style=\"display: grid; grid-template-columns: auto auto; row-gap: 5px; column-gap: 5px; place-content: space-evenly; width: 100%;\">");
    html.push_str("<legend>Filter Messages by Category:</legend>");
    html.push_str("<input type=\"text\" id=\"session_id\" name=\"session_id\" style=\"display: none;\" value=\"");
    html.push_str(&request.session_id.to_string());
    html.push_str("\">");

    for msg_kind in MessageKind::all() {
        if !message_types_in_conversation.contains(&Some(*msg_kind)) {
            continue;
        }

        html.push_str("<span>");
        let msg_name = msg_kind.as_str();
        let msg_id = msg_name.replace(" ", "_");
        html.push_str("<input type=\"checkbox\" id=\"");
        html.push_str(&msg_id);
        html.push_str("\" name=\"");
        html.push_str(&msg_id);
        html.push_str("\" value=\"");
        html.push_str("true");
        html.push_str("\"");

        if allow_list.contains(&Some(*msg_kind)) {
            html.push_str(" checked");
        }

        html.push('>');

        html.push_str("<label for=\"");
        html.push_str(&msg_id);
        html.push_str("\">");
        html.push_str(msg_name);
        html.push_str("</label><br/>");
        html.push_str("</span>");
    }

    if message_types_in_conversation.contains(&None) {
        html.push_str("<span>");
        html.push_str("<input type=\"checkbox\" id=\"uncategorized\" name=\"uncategorized\"");

        if allow_list.contains(&None) {
            html.push_str(" checked");
        }
        html.push('>');

        html.push_str("<label for=\"uncategorized\">uncategorized</label><br/>");
        html.push_str("</span>");
    }

    html.push_str("</fieldset>");

    html.push_str("<button type=\"Submit\">Update Results</button>");

    html.push_str("</form>");

    html
}
