use std::{collections::HashMap, vec::IntoIter};

use lsp_server::{Message, Request, RequestId};
use lsp_types::{
    CancelParams, NumberOrString, ProgressParams, WorkDoneProgressParams,
    notification::{
        Cancel, DidChangeConfiguration, DidChangeNotebookDocument, DidChangeTextDocument,
        DidChangeWatchedFiles, DidChangeWorkspaceFolders, DidCloseNotebookDocument,
        DidCloseTextDocument, DidCreateFiles, DidDeleteFiles, DidOpenNotebookDocument,
        DidOpenTextDocument, DidSaveNotebookDocument, DidSaveTextDocument, Exit, Initialized,
        LogMessage, LogTrace, Notification, Progress, PublishDiagnostics, SetTrace, ShowMessage,
        TelemetryEvent, WillSaveTextDocument, WorkDoneProgressCancel,
    },
    request::{
        ApplyWorkspaceEdit, CallHierarchyIncomingCalls, CallHierarchyOutgoingCalls,
        CallHierarchyPrepare, CodeActionRequest, CodeActionResolveRequest, CodeLensRefresh,
        CodeLensRequest, CodeLensResolve, ColorPresentationRequest, Completion, DocumentColor,
        DocumentDiagnosticRequest, DocumentHighlightRequest, DocumentLinkRequest,
        DocumentLinkResolve, DocumentSymbolRequest, ExecuteCommand, FoldingRangeRequest,
        Formatting, GotoDeclaration, GotoDefinition, GotoImplementation, GotoTypeDefinition,
        HoverRequest, Initialize, InlayHintRefreshRequest, InlayHintRequest,
        InlayHintResolveRequest, InlineValueRefreshRequest, InlineValueRequest, LinkedEditingRange,
        MonikerRequest, OnTypeFormatting, PrepareRenameRequest, RangeFormatting, References,
        RegisterCapability, Rename, Request as LspRequest, ResolveCompletionItem,
        SelectionRangeRequest, SemanticTokensFullDeltaRequest, SemanticTokensFullRequest,
        SemanticTokensRangeRequest, SemanticTokensRefresh, ShowDocument, ShowMessageRequest,
        Shutdown, SignatureHelpRequest, TypeHierarchyPrepare, TypeHierarchySubtypes,
        TypeHierarchySupertypes, UnregisterCapability, WillCreateFiles, WillRenameFiles,
        WillSaveWaitUntil, WorkDoneProgressCreate, WorkspaceConfiguration,
        WorkspaceDiagnosticRefresh, WorkspaceDiagnosticRequest, WorkspaceFoldersRequest,
        WorkspaceSymbolRequest, WorkspaceSymbolResolve,
    },
};

use crate::session::{MessageSource, MessageWithTimeStamp};

pub(crate) struct Conversation {
    messages: Vec<MessageWithTimeStamp>,
    requests: HashMap<RequestId, Request>,
    progress_tokens: HashMap<NumberOrString, Request>,
}

impl Conversation {
    pub(crate) fn messages(&self) -> &[MessageWithTimeStamp] {
        &self.messages
    }

    pub(crate) fn requests(&self) -> &HashMap<RequestId, Request> {
        &self.requests
    }
}

impl From<Vec<MessageWithTimeStamp>> for Conversation {
    fn from(value: Vec<MessageWithTimeStamp>) -> Self {
        let mut requests = HashMap::new();
        let mut progress_tokens = HashMap::new();

        for msg in value.iter() {
            match &msg.message {
                Message::Request(request) => {
                    requests.insert(request.id.clone(), request.clone());

                    match request.method.as_str() {
                        Initialize::METHOD
                        | GotoDeclaration::METHOD
                        | GotoDefinition::METHOD
                        | GotoTypeDefinition::METHOD
                        | GotoImplementation::METHOD
                        | References::METHOD
                        | CallHierarchyPrepare::METHOD
                        | CallHierarchyIncomingCalls::METHOD
                        | CallHierarchyOutgoingCalls::METHOD
                        | TypeHierarchyPrepare::METHOD
                        | TypeHierarchySupertypes::METHOD
                        | TypeHierarchySubtypes::METHOD
                        | DocumentHighlightRequest::METHOD
                        | DocumentLinkRequest::METHOD
                        | HoverRequest::METHOD
                        | CodeLensRequest::METHOD
                        | FoldingRangeRequest::METHOD
                        | SelectionRangeRequest::METHOD
                        | DocumentSymbolRequest::METHOD
                        | SemanticTokensFullRequest::METHOD
                        | SemanticTokensFullDeltaRequest::METHOD
                        | SemanticTokensRangeRequest::METHOD
                        | InlayHintRequest::METHOD
                        | InlineValueRequest::METHOD
                        | MonikerRequest::METHOD
                        | Completion::METHOD
                        | DocumentDiagnosticRequest::METHOD
                        | WorkspaceDiagnosticRequest::METHOD
                        | SignatureHelpRequest::METHOD
                        | CodeActionRequest::METHOD
                        | DocumentColor::METHOD
                        | ColorPresentationRequest::METHOD
                        | Formatting::METHOD
                        | RangeFormatting::METHOD
                        | Rename::METHOD
                        | PrepareRenameRequest::METHOD
                        | LinkedEditingRange::METHOD
                        | WorkspaceSymbolRequest::METHOD
                        | ExecuteCommand::METHOD => {
                            if let Ok(params) = serde_json::from_value::<WorkDoneProgressParams>(
                                request.params.clone(),
                            ) {
                                if let Some(token) = params.work_done_token {
                                    progress_tokens.insert(token, request.clone());
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Message::Response(_) => {}
                Message::Notification(_) => {}
            }
        }

        Self {
            messages: value,
            requests,
            progress_tokens,
        }
    }
}

impl IntoIterator for Conversation {
    type IntoIter = IntoIter<MessageWithTimeStamp>;
    type Item = MessageWithTimeStamp;

    fn into_iter(self) -> Self::IntoIter {
        self.messages.into_iter()
    }
}

impl<'a> IntoIterator for &'a Conversation {
    type IntoIter = std::slice::Iter<'a, MessageWithTimeStamp>;
    type Item = &'a MessageWithTimeStamp;

    fn into_iter(self) -> Self::IntoIter {
        (&self.messages).into_iter()
    }
}

impl<'a> IntoIterator for &'a mut Conversation {
    type IntoIter = std::slice::IterMut<'a, MessageWithTimeStamp>;
    type Item = &'a mut MessageWithTimeStamp;

    fn into_iter(self) -> Self::IntoIter {
        (&mut self.messages).into_iter()
    }
}

pub(crate) fn get_source(
    message: &Message,
    containing_conversation: &Conversation,
) -> Option<MessageSource> {
    match message {
        Message::Request(request) => get_request_source(request),
        Message::Response(response) => containing_conversation
            .requests
            .get(&response.id)
            .map(|source| {
                get_request_source(source)
                    .as_ref()
                    .map(MessageSource::other)
            })
            .flatten(),
        Message::Notification(notification) => match notification.method.as_str() {
            Cancel::METHOD => serde_json::from_value::<CancelParams>(notification.params.clone())
                .ok()
                .map(|cancel_params| {
                    let request_id = match cancel_params.id {
                        NumberOrString::Number(num) => RequestId::from(num),
                        NumberOrString::String(str) => RequestId::from(str),
                    };

                    containing_conversation.requests.get(&request_id).cloned()
                })
                .flatten()
                .as_ref()
                .map(get_request_source)
                .flatten(),
            Progress::METHOD => {
                serde_json::from_value::<ProgressParams>(notification.params.clone())
                    .ok()
                    .map(|progress_params| {
                        containing_conversation
                            .progress_tokens
                            .get(&progress_params.token)
                            .map(get_request_source)
                            .flatten()
                            .as_ref()
                            .map(MessageSource::other)
                    })
                    .flatten()
            }
            SetTrace::METHOD => Some(MessageSource::Client),
            LogTrace::METHOD => Some(MessageSource::Server),
            Initialized::METHOD => Some(MessageSource::Client),
            Exit::METHOD => Some(MessageSource::Client),
            // document synchronization
            DidOpenTextDocument::METHOD
            | DidChangeTextDocument::METHOD
            | WillSaveTextDocument::METHOD
            | WillSaveWaitUntil::METHOD
            | DidSaveTextDocument::METHOD
            | DidCloseTextDocument::METHOD => Some(MessageSource::Client),
            DidOpenNotebookDocument::METHOD
            | DidChangeNotebookDocument::METHOD
            | DidSaveNotebookDocument::METHOD
            | DidCloseNotebookDocument::METHOD => Some(MessageSource::Client),
            DidChangeConfiguration::METHOD
            | DidChangeWorkspaceFolders::METHOD
            | DidCreateFiles::METHOD
            | DidDeleteFiles::METHOD
            | DidChangeWatchedFiles::METHOD => Some(MessageSource::Client),
            ShowMessage::METHOD
            | ShowMessageRequest::METHOD
            | ShowDocument::METHOD
            | LogMessage::METHOD
            | WorkDoneProgressCancel::METHOD => Some(MessageSource::Client),
            TelemetryEvent::METHOD => Some(MessageSource::Server),
            _ => None,
        },
    }
}

fn get_request_source(request: &Request) -> Option<MessageSource> {
    match request.method.as_str() {
        Initialize::METHOD => Some(MessageSource::Client),
        RegisterCapability::METHOD => Some(MessageSource::Server),
        UnregisterCapability::METHOD => Some(MessageSource::Server),
        Shutdown::METHOD => Some(MessageSource::Client),
        GotoDeclaration::METHOD
        | GotoDefinition::METHOD
        | GotoTypeDefinition::METHOD
        | GotoImplementation::METHOD
        | References::METHOD
        | CallHierarchyPrepare::METHOD
        | CallHierarchyIncomingCalls::METHOD
        | CallHierarchyOutgoingCalls::METHOD
        | TypeHierarchyPrepare::METHOD
        | TypeHierarchySupertypes::METHOD
        | TypeHierarchySubtypes::METHOD
        | DocumentHighlightRequest::METHOD
        | DocumentLinkRequest::METHOD
        | DocumentLinkResolve::METHOD
        | HoverRequest::METHOD
        | CodeLensRequest::METHOD
        | CodeLensResolve::METHOD
        | FoldingRangeRequest::METHOD
        | SelectionRangeRequest::METHOD
        | DocumentSymbolRequest::METHOD
        | SemanticTokensFullRequest::METHOD
        | SemanticTokensFullDeltaRequest::METHOD
        | SemanticTokensRangeRequest::METHOD
        | InlayHintRequest::METHOD
        | InlayHintResolveRequest::METHOD
        | InlineValueRequest::METHOD
        | MonikerRequest::METHOD
        | Completion::METHOD
        | ResolveCompletionItem::METHOD
        | DocumentDiagnosticRequest::METHOD
        | WorkspaceDiagnosticRequest::METHOD
        | SignatureHelpRequest::METHOD
        | CodeActionRequest::METHOD
        | CodeActionResolveRequest::METHOD
        | DocumentColor::METHOD
        | ColorPresentationRequest::METHOD
        | Formatting::METHOD
        | RangeFormatting::METHOD
        | OnTypeFormatting::METHOD
        | Rename::METHOD
        | PrepareRenameRequest::METHOD
        | LinkedEditingRange::METHOD
        | WorkspaceSymbolRequest::METHOD
        | WorkspaceSymbolResolve::METHOD
        | WillCreateFiles::METHOD
        | WillRenameFiles::METHOD
        | ExecuteCommand::METHOD => Some(MessageSource::Client),
        CodeLensRefresh::METHOD
        | SemanticTokensRefresh::METHOD
        | InlayHintRefreshRequest::METHOD
        | InlineValueRefreshRequest::METHOD
        | PublishDiagnostics::METHOD
        | WorkspaceDiagnosticRefresh::METHOD
        | WorkspaceConfiguration::METHOD
        | WorkspaceFoldersRequest::METHOD
        | ApplyWorkspaceEdit::METHOD => Some(MessageSource::Server),
        WorkDoneProgressCreate::METHOD => Some(MessageSource::Server),
        _ => None,
    }
}

pub fn classify(message: &Message, containing_conversation: &Conversation) -> Option<MessageKind> {
    match message {
        Message::Request(request) => classify_request(request),
        Message::Response(response) => containing_conversation
            .requests
            .get(&response.id)
            .map(classify_request)
            .flatten(),
        Message::Notification(notification) => {
            match notification.method.as_str() {
                Cancel::METHOD => {
                    serde_json::from_value::<CancelParams>(notification.params.clone())
                        .ok()
                        .map(|cancel_params| {
                            let request_id = match cancel_params.id {
                                NumberOrString::Number(num) => RequestId::from(num),
                                NumberOrString::String(str) => RequestId::from(str),
                            };

                            containing_conversation.requests.get(&request_id)
                        })
                        .flatten()
                        .map(classify_request)
                        .flatten()
                }
                Progress::METHOD => {
                    serde_json::from_value::<ProgressParams>(notification.params.clone())
                        .ok()
                        .map(|progress_params| {
                            containing_conversation
                                .progress_tokens
                                .get(&progress_params.token)
                                .map(classify_request)
                        })
                        .flatten()
                        .flatten()
                }
                SetTrace::METHOD => Some(MessageKind::Lifecycle),
                LogTrace::METHOD => Some(MessageKind::Lifecycle),
                Initialized::METHOD => Some(MessageKind::Lifecycle),
                Exit::METHOD => Some(MessageKind::Lifecycle),
                // document synchronization
                DidOpenTextDocument::METHOD
                | DidChangeTextDocument::METHOD
                | WillSaveTextDocument::METHOD
                | WillSaveWaitUntil::METHOD
                | DidSaveTextDocument::METHOD
                | DidCloseTextDocument::METHOD => Some(MessageKind::TextDocumentSynchronization),
                DidOpenNotebookDocument::METHOD
                | DidChangeNotebookDocument::METHOD
                | DidSaveNotebookDocument::METHOD
                | DidCloseNotebookDocument::METHOD => {
                    Some(MessageKind::NotebookDocumentSynchronization)
                }
                DidChangeConfiguration::METHOD
                | DidChangeWorkspaceFolders::METHOD
                | DidCreateFiles::METHOD
                | DidDeleteFiles::METHOD
                | DidChangeWatchedFiles::METHOD => Some(MessageKind::WorkspaceSynchronization),
                ShowMessage::METHOD
                | ShowMessageRequest::METHOD
                | ShowDocument::METHOD
                | LogMessage::METHOD
                | WorkDoneProgressCancel::METHOD => Some(MessageKind::Workspace),
                TelemetryEvent::METHOD => Some(MessageKind::Telemetry),
                _ => None,
            }
        }
    }
}

fn classify_request(request: &Request) -> Option<MessageKind> {
    match request.method.as_str() {
        Initialize::METHOD => Some(MessageKind::Lifecycle),
        RegisterCapability::METHOD => Some(MessageKind::Lifecycle),
        UnregisterCapability::METHOD => Some(MessageKind::Lifecycle),
        Shutdown::METHOD => Some(MessageKind::Lifecycle),
        Exit::METHOD => Some(MessageKind::Lifecycle),
        GotoDeclaration::METHOD => Some(MessageKind::Declaration),
        GotoDefinition::METHOD => Some(MessageKind::Definition),
        GotoTypeDefinition::METHOD => Some(MessageKind::TypeDefinition),
        GotoImplementation::METHOD => Some(MessageKind::Implementation),
        References::METHOD => Some(MessageKind::References),
        CallHierarchyPrepare::METHOD
        | CallHierarchyIncomingCalls::METHOD
        | CallHierarchyOutgoingCalls::METHOD => Some(MessageKind::CallHierarchy),
        TypeHierarchyPrepare::METHOD
        | TypeHierarchySupertypes::METHOD
        | TypeHierarchySubtypes::METHOD => Some(MessageKind::TypeHierarchy),
        DocumentHighlightRequest::METHOD => Some(MessageKind::DocumentHighlight),
        DocumentLinkRequest::METHOD | DocumentLinkResolve::METHOD => {
            Some(MessageKind::DocumentLink)
        }
        HoverRequest::METHOD => Some(MessageKind::Hover),
        CodeLensRequest::METHOD | CodeLensResolve::METHOD => Some(MessageKind::CodeLens),
        FoldingRangeRequest::METHOD => Some(MessageKind::FoldingRange),
        SelectionRangeRequest::METHOD => Some(MessageKind::Selection),
        DocumentSymbolRequest::METHOD => Some(MessageKind::Symbol),
        SemanticTokensFullRequest::METHOD
        | SemanticTokensFullDeltaRequest::METHOD
        | SemanticTokensRangeRequest::METHOD => Some(MessageKind::SemanticTokens),
        InlayHintRequest::METHOD | InlayHintResolveRequest::METHOD => Some(MessageKind::InlayHint),
        InlineValueRequest::METHOD => Some(MessageKind::InlineValue),
        MonikerRequest::METHOD => Some(MessageKind::Moniker),
        Completion::METHOD | ResolveCompletionItem::METHOD => Some(MessageKind::Completion),
        DocumentDiagnosticRequest::METHOD => Some(MessageKind::Diagnostic),
        WorkspaceDiagnosticRequest::METHOD => Some(MessageKind::Diagnostic),
        SignatureHelpRequest::METHOD => Some(MessageKind::SignatureHelp),
        CodeActionRequest::METHOD | CodeActionResolveRequest::METHOD => {
            Some(MessageKind::CodeAction)
        }
        DocumentColor::METHOD | ColorPresentationRequest::METHOD => {
            Some(MessageKind::DocumentColor)
        }
        Formatting::METHOD | RangeFormatting::METHOD | OnTypeFormatting::METHOD => {
            Some(MessageKind::Formatting)
        }
        Rename::METHOD | PrepareRenameRequest::METHOD => Some(MessageKind::Rename),
        LinkedEditingRange::METHOD => Some(MessageKind::LinkedEditingRange),
        WorkspaceSymbolRequest::METHOD | WorkspaceSymbolResolve::METHOD => {
            Some(MessageKind::Symbol)
        }
        WillCreateFiles::METHOD | WillRenameFiles::METHOD => {
            Some(MessageKind::WorkspaceSynchronization)
        }
        ExecuteCommand::METHOD => Some(MessageKind::ExecuteCommand),
        CodeLensRefresh::METHOD => Some(MessageKind::CodeLens),
        SemanticTokensRefresh::METHOD => Some(MessageKind::SemanticTokens),
        InlayHintRefreshRequest::METHOD => Some(MessageKind::InlayHint),
        InlineValueRefreshRequest::METHOD => Some(MessageKind::InlineValue),
        PublishDiagnostics::METHOD => Some(MessageKind::Diagnostic),
        WorkspaceDiagnosticRefresh::METHOD => Some(MessageKind::Diagnostic),
        WorkspaceConfiguration::METHOD | WorkspaceFoldersRequest::METHOD => {
            Some(MessageKind::WorkspaceSynchronization)
        }
        ApplyWorkspaceEdit::METHOD => Some(MessageKind::Workspace),
        WorkDoneProgressCreate::METHOD => Some(MessageKind::Lifecycle),
        _ => None,
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub(crate) enum MessageKind {
    Lifecycle,
    TextDocumentSynchronization,
    NotebookDocumentSynchronization,
    WorkspaceSynchronization,
    Workspace,
    Telemetry,
    Declaration,
    Definition,
    TypeDefinition,
    Implementation,
    References,
    CallHierarchy,
    TypeHierarchy,
    DocumentHighlight,
    DocumentLink,
    Hover,
    CodeLens,
    FoldingRange,
    Selection,
    Symbol,
    SemanticTokens,
    InlayHint,
    InlineValue,
    Moniker,
    Completion,
    Diagnostic,
    SignatureHelp,
    CodeAction,
    DocumentColor,
    Formatting,
    Rename,
    LinkedEditingRange,
    ExecuteCommand,
}

impl MessageKind {
    pub(crate) fn all() -> &'static [MessageKind] {
        &[
            MessageKind::Lifecycle,
            MessageKind::TextDocumentSynchronization,
            MessageKind::NotebookDocumentSynchronization,
            MessageKind::WorkspaceSynchronization,
            MessageKind::Workspace,
            MessageKind::Telemetry,
            MessageKind::Declaration,
            MessageKind::Definition,
            MessageKind::TypeDefinition,
            MessageKind::Implementation,
            MessageKind::References,
            MessageKind::CallHierarchy,
            MessageKind::TypeHierarchy,
            MessageKind::DocumentHighlight,
            MessageKind::DocumentLink,
            MessageKind::Hover,
            MessageKind::CodeLens,
            MessageKind::FoldingRange,
            MessageKind::Selection,
            MessageKind::Symbol,
            MessageKind::SemanticTokens,
            MessageKind::InlayHint,
            MessageKind::InlineValue,
            MessageKind::Moniker,
            MessageKind::Completion,
            MessageKind::Diagnostic,
            MessageKind::SignatureHelp,
            MessageKind::CodeAction,
            MessageKind::DocumentColor,
            MessageKind::Formatting,
            MessageKind::Rename,
            MessageKind::LinkedEditingRange,
            MessageKind::ExecuteCommand,
        ]
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            MessageKind::Lifecycle => "life cycle",
            MessageKind::TextDocumentSynchronization => "document synchronization",
            MessageKind::NotebookDocumentSynchronization => "notebook synchronization",
            MessageKind::WorkspaceSynchronization => "workspace synchronization",
            MessageKind::Workspace => "workspace",
            MessageKind::Telemetry => "telemetry",
            MessageKind::Declaration => "declaration",
            MessageKind::Definition => "definition",
            MessageKind::TypeDefinition => "type definition",
            MessageKind::Implementation => "implementation",
            MessageKind::References => "references",
            MessageKind::CallHierarchy => "call hierarchy",
            MessageKind::TypeHierarchy => "type hierarchy",
            MessageKind::DocumentHighlight => "document highlight",
            MessageKind::DocumentLink => "document link",
            MessageKind::Hover => "hover",
            MessageKind::CodeLens => "code lens",
            MessageKind::FoldingRange => "folding range",
            MessageKind::Selection => "selection",
            MessageKind::Symbol => "symbol",
            MessageKind::SemanticTokens => "semantic tokens",
            MessageKind::InlayHint => "inlay hint",
            MessageKind::InlineValue => "inline value",
            MessageKind::Moniker => "moniker",
            MessageKind::Completion => "completion",
            MessageKind::Diagnostic => "diagnostic",
            MessageKind::SignatureHelp => "signature help",
            MessageKind::CodeAction => "code action",
            MessageKind::DocumentColor => "document color",
            MessageKind::Formatting => "formatting",
            MessageKind::Rename => "rename",
            MessageKind::LinkedEditingRange => "linked editing range",
            MessageKind::ExecuteCommand => "execute command",
        }
    }

    fn try_parse_str(str: &str) -> Option<Self> {
        match str {
            "life_cycle" => Some(MessageKind::Lifecycle),
            "document_synchronization" => Some(MessageKind::TextDocumentSynchronization),
            "notebook_synchronization" => Some(MessageKind::NotebookDocumentSynchronization),
            "workspace_synchronization" => Some(MessageKind::WorkspaceSynchronization),
            "workspace" => Some(MessageKind::Workspace),
            "telemetry" => Some(MessageKind::Telemetry),
            "declaration" => Some(MessageKind::Declaration),
            "definition" => Some(MessageKind::Definition),
            "type_definition" => Some(MessageKind::TypeDefinition),
            "implementation" => Some(MessageKind::Implementation),
            "references" => Some(MessageKind::References),
            "call_hierarchy" => Some(MessageKind::CallHierarchy),
            "type_hierarchy" => Some(MessageKind::TypeHierarchy),
            "document_highlight" => Some(MessageKind::DocumentHighlight),
            "document_link" => Some(MessageKind::DocumentLink),
            "hover" => Some(MessageKind::Hover),
            "code_lens" => Some(MessageKind::CodeLens),
            "folding_range" => Some(MessageKind::FoldingRange),
            "selection" => Some(MessageKind::Selection),
            "symbol" => Some(MessageKind::Symbol),
            "semantic_tokens" => Some(MessageKind::SemanticTokens),
            "inlay_hint" => Some(MessageKind::InlayHint),
            "inline_value" => Some(MessageKind::InlineValue),
            "moniker" => Some(MessageKind::Moniker),
            "completion" => Some(MessageKind::Completion),
            "diagnostic" => Some(MessageKind::Diagnostic),
            "signature_help" => Some(MessageKind::SignatureHelp),
            "code_action" => Some(MessageKind::CodeAction),
            "document_color" => Some(MessageKind::DocumentColor),
            "formatting" => Some(MessageKind::Formatting),
            "rename" => Some(MessageKind::Rename),
            "linked_editing_range" => Some(MessageKind::LinkedEditingRange),
            "execute_command" => Some(MessageKind::ExecuteCommand),
            _ => None,
        }
    }
}
