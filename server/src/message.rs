use std::{
    collections::HashMap,
    fmt,
    io::{self, BufRead, Write},
    vec::IntoIter,
};

use lsp_types::{
    CancelParams, NumberOrString, ProgressParams, WorkDoneProgressParams,
    notification::{
        Cancel, DidChangeConfiguration, DidChangeNotebookDocument, DidChangeTextDocument,
        DidChangeWatchedFiles, DidChangeWorkspaceFolders, DidCloseNotebookDocument,
        DidCloseTextDocument, DidCreateFiles, DidDeleteFiles, DidOpenNotebookDocument,
        DidOpenTextDocument, DidSaveNotebookDocument, DidSaveTextDocument, Exit, Initialized,
        LogMessage, LogTrace, Notification as NotificationTrait, Progress, PublishDiagnostics,
        SetTrace, ShowMessage, TelemetryEvent, WillSaveTextDocument, WorkDoneProgressCancel,
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
use serde::{Deserialize, Serialize};

use crate::session::MessageWithTimeStamp;

pub(crate) struct Conversation {
    pub(crate) messages: Vec<MessageWithTimeStamp>,
    pub(crate) requests: HashMap<RequestId, Request>,
    pub(crate) progress_tokens: HashMap<NumberOrString, Request>,
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

pub fn classify(
    message: &Message,
    containing_conversation: &Conversation,
) -> Option<MessageClassification> {
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
                        .map(|cancel_params| match cancel_params.id {
                            NumberOrString::Number(num) => containing_conversation
                                .requests
                                .get(&RequestId::from(num))
                                .or_else(|| {
                                    containing_conversation
                                        .requests
                                        .get(&RequestId::from(num.to_string()))
                                }),
                            NumberOrString::String(str) => containing_conversation
                                .requests
                                .get(&RequestId::from(str.clone()))
                                .or_else(|| {
                                    str.parse::<i32>()
                                        .ok()
                                        .map(|id| {
                                            containing_conversation
                                                .requests
                                                .get(&RequestId::from(id))
                                        })
                                        .flatten()
                                }),
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
                SetTrace::METHOD => Some(MessageClassification::Lifecycle),
                LogTrace::METHOD => Some(MessageClassification::Lifecycle),
                Initialized::METHOD => Some(MessageClassification::Lifecycle),
                Exit::METHOD => Some(MessageClassification::Lifecycle),
                // document synchronization
                DidOpenTextDocument::METHOD
                | DidChangeTextDocument::METHOD
                | WillSaveTextDocument::METHOD
                | WillSaveWaitUntil::METHOD
                | DidSaveTextDocument::METHOD
                | DidCloseTextDocument::METHOD => {
                    Some(MessageClassification::TextDocumentSynchronization)
                }
                DidOpenNotebookDocument::METHOD
                | DidChangeNotebookDocument::METHOD
                | DidSaveNotebookDocument::METHOD
                | DidCloseNotebookDocument::METHOD => {
                    Some(MessageClassification::NotebookDocumentSynchronization)
                }
                DidChangeConfiguration::METHOD
                | DidChangeWorkspaceFolders::METHOD
                | DidCreateFiles::METHOD
                | DidDeleteFiles::METHOD
                | DidChangeWatchedFiles::METHOD => {
                    Some(MessageClassification::WorkspaceSynchronization)
                }
                ShowMessage::METHOD
                | ShowMessageRequest::METHOD
                | ShowDocument::METHOD
                | LogMessage::METHOD
                | WorkDoneProgressCancel::METHOD => Some(MessageClassification::Workspace),
                TelemetryEvent::METHOD => Some(MessageClassification::Telemetry),
                _ => None,
            }
        }
    }
}

fn classify_request(request: &Request) -> Option<MessageClassification> {
    match request.method.as_str() {
        Initialize::METHOD => Some(MessageClassification::Lifecycle),
        RegisterCapability::METHOD => Some(MessageClassification::Lifecycle),
        UnregisterCapability::METHOD => Some(MessageClassification::Lifecycle),
        Shutdown::METHOD => Some(MessageClassification::Lifecycle),
        Exit::METHOD => Some(MessageClassification::Lifecycle),
        GotoDeclaration::METHOD => Some(MessageClassification::Declaration),
        GotoDefinition::METHOD => Some(MessageClassification::Definition),
        GotoTypeDefinition::METHOD => Some(MessageClassification::TypeDefinition),
        GotoImplementation::METHOD => Some(MessageClassification::Implementation),
        References::METHOD => Some(MessageClassification::References),
        CallHierarchyPrepare::METHOD
        | CallHierarchyIncomingCalls::METHOD
        | CallHierarchyOutgoingCalls::METHOD => Some(MessageClassification::CallHierarchy),
        TypeHierarchyPrepare::METHOD
        | TypeHierarchySupertypes::METHOD
        | TypeHierarchySubtypes::METHOD => Some(MessageClassification::TypeHierarchy),
        DocumentHighlightRequest::METHOD => Some(MessageClassification::DocumentHighlight),
        DocumentLinkRequest::METHOD | DocumentLinkResolve::METHOD => {
            Some(MessageClassification::DocumentLink)
        }
        HoverRequest::METHOD => Some(MessageClassification::Hover),
        CodeLensRequest::METHOD | CodeLensResolve::METHOD => Some(MessageClassification::CodeLens),
        FoldingRangeRequest::METHOD => Some(MessageClassification::FoldingRange),
        SelectionRangeRequest::METHOD => Some(MessageClassification::Selection),
        DocumentSymbolRequest::METHOD => Some(MessageClassification::Symbol),
        SemanticTokensFullRequest::METHOD
        | SemanticTokensFullDeltaRequest::METHOD
        | SemanticTokensRangeRequest::METHOD => Some(MessageClassification::SemanticTokens),
        InlayHintRequest::METHOD | InlayHintResolveRequest::METHOD => {
            Some(MessageClassification::InlayHint)
        }
        InlineValueRequest::METHOD => Some(MessageClassification::InlineValue),
        MonikerRequest::METHOD => Some(MessageClassification::Moniker),
        Completion::METHOD | ResolveCompletionItem::METHOD => {
            Some(MessageClassification::Completion)
        }
        DocumentDiagnosticRequest::METHOD => Some(MessageClassification::Diagnostic),
        WorkspaceDiagnosticRequest::METHOD => Some(MessageClassification::Diagnostic),
        SignatureHelpRequest::METHOD => Some(MessageClassification::SignatureHelp),
        CodeActionRequest::METHOD | CodeActionResolveRequest::METHOD => {
            Some(MessageClassification::CodeAction)
        }
        DocumentColor::METHOD | ColorPresentationRequest::METHOD => {
            Some(MessageClassification::DocumentColor)
        }
        Formatting::METHOD | RangeFormatting::METHOD | OnTypeFormatting::METHOD => {
            Some(MessageClassification::Formatting)
        }
        Rename::METHOD | PrepareRenameRequest::METHOD => Some(MessageClassification::Rename),
        LinkedEditingRange::METHOD => Some(MessageClassification::LinkedEditingRange),
        WorkspaceSymbolRequest::METHOD | WorkspaceSymbolResolve::METHOD => {
            Some(MessageClassification::Symbol)
        }
        WillCreateFiles::METHOD | WillRenameFiles::METHOD => {
            Some(MessageClassification::WorkspaceSynchronization)
        }
        ExecuteCommand::METHOD => Some(MessageClassification::ExecuteCommand),
        CodeLensRefresh::METHOD => Some(MessageClassification::CodeLens),
        SemanticTokensRefresh::METHOD => Some(MessageClassification::SemanticTokens),
        InlayHintRefreshRequest::METHOD => Some(MessageClassification::InlayHint),
        InlineValueRefreshRequest::METHOD => Some(MessageClassification::InlineValue),
        PublishDiagnostics::METHOD => Some(MessageClassification::Diagnostic),
        WorkspaceDiagnosticRefresh::METHOD => Some(MessageClassification::Diagnostic),
        WorkspaceConfiguration::METHOD | WorkspaceFoldersRequest::METHOD => {
            Some(MessageClassification::WorkspaceSynchronization)
        }
        ApplyWorkspaceEdit::METHOD => Some(MessageClassification::Workspace),
        WorkDoneProgressCreate::METHOD => Some(MessageClassification::Lifecycle),
        _ => None,
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub(crate) enum MessageClassification {
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

impl MessageClassification {
    pub(crate) fn all() -> &'static [MessageClassification] {
        &[
            MessageClassification::Lifecycle,
            MessageClassification::TextDocumentSynchronization,
            MessageClassification::NotebookDocumentSynchronization,
            MessageClassification::WorkspaceSynchronization,
            MessageClassification::Workspace,
            MessageClassification::Telemetry,
            MessageClassification::Declaration,
            MessageClassification::Definition,
            MessageClassification::TypeDefinition,
            MessageClassification::Implementation,
            MessageClassification::References,
            MessageClassification::CallHierarchy,
            MessageClassification::TypeHierarchy,
            MessageClassification::DocumentHighlight,
            MessageClassification::DocumentLink,
            MessageClassification::Hover,
            MessageClassification::CodeLens,
            MessageClassification::FoldingRange,
            MessageClassification::Selection,
            MessageClassification::Symbol,
            MessageClassification::SemanticTokens,
            MessageClassification::InlayHint,
            MessageClassification::InlineValue,
            MessageClassification::Moniker,
            MessageClassification::Completion,
            MessageClassification::Diagnostic,
            MessageClassification::SignatureHelp,
            MessageClassification::CodeAction,
            MessageClassification::DocumentColor,
            MessageClassification::Formatting,
            MessageClassification::Rename,
            MessageClassification::LinkedEditingRange,
            MessageClassification::ExecuteCommand,
        ]
    }

    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            MessageClassification::Lifecycle => "life cycle",
            MessageClassification::TextDocumentSynchronization => "document synchronization",
            MessageClassification::NotebookDocumentSynchronization => "notebook synchronization",
            MessageClassification::WorkspaceSynchronization => "workspace synchronization",
            MessageClassification::Workspace => "workspace",
            MessageClassification::Telemetry => "telemetry",
            MessageClassification::Declaration => "declaration",
            MessageClassification::Definition => "definition",
            MessageClassification::TypeDefinition => "type definition",
            MessageClassification::Implementation => "implementation",
            MessageClassification::References => "references",
            MessageClassification::CallHierarchy => "call hierarchy",
            MessageClassification::TypeHierarchy => "type hierarchy",
            MessageClassification::DocumentHighlight => "document highlight",
            MessageClassification::DocumentLink => "document link",
            MessageClassification::Hover => "hover",
            MessageClassification::CodeLens => "code lens",
            MessageClassification::FoldingRange => "folding range",
            MessageClassification::Selection => "selection",
            MessageClassification::Symbol => "symbol",
            MessageClassification::SemanticTokens => "semantic tokens",
            MessageClassification::InlayHint => "inlay hint",
            MessageClassification::InlineValue => "inline value",
            MessageClassification::Moniker => "moniker",
            MessageClassification::Completion => "completion",
            MessageClassification::Diagnostic => "diagnostic",
            MessageClassification::SignatureHelp => "signature help",
            MessageClassification::CodeAction => "code action",
            MessageClassification::DocumentColor => "document color",
            MessageClassification::Formatting => "formatting",
            MessageClassification::Rename => "rename",
            MessageClassification::LinkedEditingRange => "linked editing range",
            MessageClassification::ExecuteCommand => "execute command",
        }
    }

    fn try_parse_str(str: &str) -> Option<Self> {
        match str {
            "life_cycle" => Some(MessageClassification::Lifecycle),
            "document_synchronization" => Some(MessageClassification::TextDocumentSynchronization),
            "notebook_synchronization" => {
                Some(MessageClassification::NotebookDocumentSynchronization)
            }
            "workspace_synchronization" => Some(MessageClassification::WorkspaceSynchronization),
            "workspace" => Some(MessageClassification::Workspace),
            "telemetry" => Some(MessageClassification::Telemetry),
            "declaration" => Some(MessageClassification::Declaration),
            "definition" => Some(MessageClassification::Definition),
            "type_definition" => Some(MessageClassification::TypeDefinition),
            "implementation" => Some(MessageClassification::Implementation),
            "references" => Some(MessageClassification::References),
            "call_hierarchy" => Some(MessageClassification::CallHierarchy),
            "type_hierarchy" => Some(MessageClassification::TypeHierarchy),
            "document_highlight" => Some(MessageClassification::DocumentHighlight),
            "document_link" => Some(MessageClassification::DocumentLink),
            "hover" => Some(MessageClassification::Hover),
            "code_lens" => Some(MessageClassification::CodeLens),
            "folding_range" => Some(MessageClassification::FoldingRange),
            "selection" => Some(MessageClassification::Selection),
            "symbol" => Some(MessageClassification::Symbol),
            "semantic_tokens" => Some(MessageClassification::SemanticTokens),
            "inlay_hint" => Some(MessageClassification::InlayHint),
            "inline_value" => Some(MessageClassification::InlineValue),
            "moniker" => Some(MessageClassification::Moniker),
            "completion" => Some(MessageClassification::Completion),
            "diagnostic" => Some(MessageClassification::Diagnostic),
            "signature_help" => Some(MessageClassification::SignatureHelp),
            "code_action" => Some(MessageClassification::CodeAction),
            "document_color" => Some(MessageClassification::DocumentColor),
            "formatting" => Some(MessageClassification::Formatting),
            "rename" => Some(MessageClassification::Rename),
            "linked_editing_range" => Some(MessageClassification::LinkedEditingRange),
            "execute_command" => Some(MessageClassification::ExecuteCommand),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Message {
    Request(Request),
    Response(Response),
    Notification(Notification),
}

impl Message {
    pub(crate) fn kind(&self) -> MessageKind {
        match self {
            Message::Request(_) => MessageKind::Request,
            Message::Response(_) => MessageKind::Response,
            Message::Notification(_) => MessageKind::Notification,
        }
    }
}

impl From<Request> for Message {
    fn from(request: Request) -> Message {
        Message::Request(request)
    }
}

impl From<Response> for Message {
    fn from(response: Response) -> Message {
        Message::Response(response)
    }
}

impl From<Notification> for Message {
    fn from(notification: Notification) -> Message {
        Message::Notification(notification)
    }
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Request = 0,
    Response = 1,
    Notification = 2,
}

impl TryFrom<u8> for MessageKind {
    type Error = ();
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Request),
            1 => Ok(Self::Response),
            2 => Ok(Self::Notification),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub struct RequestId(IdRepr);

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(untagged)]
enum IdRepr {
    I32(i32),
    String(String),
}

impl From<i32> for RequestId {
    fn from(id: i32) -> RequestId {
        RequestId(IdRepr::I32(id))
    }
}

impl From<String> for RequestId {
    fn from(id: String) -> RequestId {
        RequestId(IdRepr::String(id))
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            IdRepr::I32(it) => fmt::Display::fmt(it, f),
            // Use debug here, to make it clear that `92` and `"92"` are
            // different, and to reduce WTF factor if the sever uses `" "` as an
            // ID.
            IdRepr::String(it) => fmt::Debug::fmt(it, f),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Request {
    pub id: RequestId,
    pub method: String,
    #[serde(default = "serde_json::Value::default")]
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Response {
    // JSON-RPC allows this to be null if we can't find or parse the
    // request id. We fail deserialization in that case, so we just
    // make this field mandatory.
    pub id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<ResponseError>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResponseError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub data: Option<serde_json::Value>,
}

#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum ErrorCode {
    // Defined by JSON RPC:
    ParseError = -32700,
    InvalidRequest = -32600,
    MethodNotFound = -32601,
    InvalidParams = -32602,
    InternalError = -32603,
    ServerErrorStart = -32099,
    ServerErrorEnd = -32000,

    /// Error code indicating that a server received a notification or
    /// request before the server has received the `initialize` request.
    ServerNotInitialized = -32002,
    UnknownErrorCode = -32001,

    // Defined by the protocol:
    /// The client has canceled a request and a server has detected
    /// the cancel.
    RequestCanceled = -32800,

    /// The server detected that the content of a document got
    /// modified outside normal conditions. A server should
    /// NOT send this error code if it detects a content change
    /// in it unprocessed messages. The result even computed
    /// on an older state might still be useful for the client.
    ///
    /// If a client decides that a result is not of any use anymore
    /// the client should cancel the request.
    ContentModified = -32801,

    /// The server cancelled the request. This error code should
    /// only be used for requests that explicitly support being
    /// server cancellable.
    ///
    /// @since 3.17.0
    ServerCancelled = -32802,

    /// A request failed but it was syntactically correct, e.g the
    /// method name was known and the parameters were valid. The error
    /// message should contain human readable information about why
    /// the request failed.
    ///
    /// @since 3.17.0
    RequestFailed = -32803,
}

impl TryFrom<i32> for ErrorCode {
    type Error = ();
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            -32700 => Ok(ErrorCode::ParseError),
            -32600 => Ok(ErrorCode::InvalidRequest),
            -32601 => Ok(ErrorCode::MethodNotFound),
            -32602 => Ok(ErrorCode::InvalidParams),
            -32603 => Ok(ErrorCode::InternalError),
            -32099 => Ok(ErrorCode::ServerErrorStart),
            -32000 => Ok(ErrorCode::ServerErrorEnd),
            -32002 => Ok(ErrorCode::ServerNotInitialized),
            -32001 => Ok(ErrorCode::UnknownErrorCode),
            -32800 => Ok(ErrorCode::RequestCanceled),
            -32801 => Ok(ErrorCode::ContentModified),
            -32802 => Ok(ErrorCode::ServerCancelled),
            -32803 => Ok(ErrorCode::RequestFailed),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Notification {
    pub method: String,
    #[serde(default = "serde_json::Value::default")]
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub params: serde_json::Value,
}

fn invalid_data(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

macro_rules! invalid_data {
    ($($tt:tt)*) => (invalid_data(format!($($tt)*)))
}

impl Message {
    pub fn read(r: &mut impl BufRead) -> io::Result<Option<Message>> {
        Message::_read(r)
    }
    fn _read(r: &mut dyn BufRead) -> io::Result<Option<Message>> {
        let text = match read_msg_text(r)? {
            None => return Ok(None),
            Some(text) => text,
        };

        let msg = match serde_json::from_str(&text) {
            Ok(msg) => msg,
            Err(e) => {
                return Err(invalid_data!("malformed LSP payload `{e:?}`: {text:?}"));
            }
        };

        Ok(Some(msg))
    }
    pub fn write(&self, w: &mut impl Write) -> io::Result<()> {
        self._write(w)
    }
    fn _write(&self, w: &mut dyn Write) -> io::Result<()> {
        #[derive(Serialize)]
        struct JsonRpc<'a> {
            jsonrpc: &'static str,
            #[serde(flatten)]
            msg: &'a Message,
        }
        let text = serde_json::to_string(&JsonRpc {
            jsonrpc: "2.0",
            msg: self,
        })?;
        write_msg_text(w, &text)
    }
}

impl Response {
    pub fn new_ok<R: serde::Serialize>(id: RequestId, result: R) -> Response {
        Response {
            id,
            result: Some(serde_json::to_value(result).unwrap()),
            error: None,
        }
    }
    pub fn new_err(id: RequestId, code: i32, message: String) -> Response {
        let error = ResponseError {
            code,
            message,
            data: None,
        };
        Response {
            id,
            result: None,
            error: Some(error),
        }
    }
}

impl Request {
    pub fn new<P: serde::Serialize>(id: RequestId, method: String, params: P) -> Request {
        Request {
            id,
            method,
            params: serde_json::to_value(params).unwrap(),
        }
    }
}

impl Notification {
    pub fn new(method: String, params: impl serde::Serialize) -> Notification {
        Notification {
            method,
            params: serde_json::to_value(params).unwrap(),
        }
    }
}

fn read_msg_text(inp: &mut dyn BufRead) -> io::Result<Option<String>> {
    let mut size = None;
    let mut buf = String::new();
    loop {
        buf.clear();
        if inp.read_line(&mut buf)? == 0 {
            return Ok(None);
        }
        if !buf.ends_with("\r\n") {
            return Err(invalid_data!("malformed header: {:?}", buf));
        }
        let buf = &buf[..buf.len() - 2];
        if buf.is_empty() {
            break;
        }
        let mut parts = buf.splitn(2, ": ");
        let header_name = parts.next().unwrap();
        let header_value = parts
            .next()
            .ok_or_else(|| invalid_data!("malformed header: {:?}", buf))?;
        if header_name.eq_ignore_ascii_case("Content-Length") {
            size = Some(header_value.parse::<usize>().map_err(invalid_data)?);
        }
    }
    let size: usize = size.ok_or_else(|| invalid_data!("no Content-Length"))?;
    let mut buf = buf.into_bytes();
    buf.resize(size, 0);
    inp.read_exact(&mut buf)?;
    let buf = String::from_utf8(buf).map_err(invalid_data)?;
    Ok(Some(buf))
}

fn write_msg_text(out: &mut dyn Write, msg: &str) -> io::Result<()> {
    write!(out, "Content-Length: {}\r\n\r\n", msg.len())?;
    out.write_all(msg.as_bytes())?;
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Message, Notification, Request, RequestId};

    #[test]
    fn shutdown_with_explicit_null() {
        let text = "{\"jsonrpc\": \"2.0\",\"id\": 3,\"method\": \"shutdown\", \"params\": null }";
        let msg: Message = serde_json::from_str(text).unwrap();

        assert!(
            matches!(msg, Message::Request(req) if req.id == 3.into() && req.method == "shutdown")
        );
    }

    #[test]
    fn shutdown_with_no_params() {
        let text = "{\"jsonrpc\": \"2.0\",\"id\": 3,\"method\": \"shutdown\"}";
        let msg: Message = serde_json::from_str(text).unwrap();

        assert!(
            matches!(msg, Message::Request(req) if req.id == 3.into() && req.method == "shutdown")
        );
    }

    #[test]
    fn notification_with_explicit_null() {
        let text = "{\"jsonrpc\": \"2.0\",\"method\": \"exit\", \"params\": null }";
        let msg: Message = serde_json::from_str(text).unwrap();

        assert!(matches!(msg, Message::Notification(not) if not.method == "exit"));
    }

    #[test]
    fn notification_with_no_params() {
        let text = "{\"jsonrpc\": \"2.0\",\"method\": \"exit\"}";
        let msg: Message = serde_json::from_str(text).unwrap();

        assert!(matches!(msg, Message::Notification(not) if not.method == "exit"));
    }

    #[test]
    fn serialize_request_with_null_params() {
        let msg = Message::Request(Request {
            id: RequestId::from(3),
            method: "shutdown".into(),
            params: serde_json::Value::Null,
        });
        let serialized = serde_json::to_string(&msg).unwrap();

        assert_eq!("{\"id\":3,\"method\":\"shutdown\"}", serialized);
    }

    #[test]
    fn serialize_notification_with_null_params() {
        let msg = Message::Notification(Notification {
            method: "exit".into(),
            params: serde_json::Value::Null,
        });
        let serialized = serde_json::to_string(&msg).unwrap();

        assert_eq!("{\"method\":\"exit\"}", serialized);
    }
}
