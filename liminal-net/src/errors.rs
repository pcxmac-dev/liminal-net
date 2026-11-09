//! Errors: a node's refusals and failures as `$error` records (NETWORK.md §4.1), and the network's own.

use crate::{Map, Value};

/// A node's refusal or failure, as it crosses the wire:
/// `{"$error": kind, "message": ..., "accept": ..., "denied": ..., "status": ...}`.
///
/// The kind is named as today's class (`CommandError`, `Refused`, `NotEntitled` ...), so every side agrees on it.
/// The platform's 400/502 mapping and a form's second confirmation depend on the kind, `accept` and `status`.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeError {
    pub kind: String,
    pub message: String,
    pub accept: Option<Value>,
    pub denied: Option<Value>,
    pub status: Option<u16>,
}

/// A request for a method the node's interface does not offer.
pub const NOT_IN_INTERFACE: &str = "NotInInterface";
/// A call of a handle no longer held.
pub const NO_SUCH_HANDLE: &str = "NoSuchHandle";
/// The node's own code failed (it panicked): the synapse stays up.
pub const NODE_FAILED: &str = "NodeFailed";
/// A link's functions could not be read.
pub const BAD_FUNCTION: &str = "BadFunction";
/// A record that came without a kind.
pub const REMOTE_ERROR: &str = "RemoteError";

impl NodeError {
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        NodeError {
            kind: kind.into(),
            message: message.into(),
            accept: None,
            denied: None,
            status: None,
        }
    }

    pub fn accept(mut self, accept: Value) -> Self {
        self.accept = Some(accept);
        self
    }

    pub fn denied(mut self, denied: Value) -> Self {
        self.denied = Some(denied);
        self
    }

    pub fn status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    /// The `$error` record.
    pub fn to_record(&self) -> Value {
        let mut m = Map::new();
        m.insert("$error".into(), Value::from(self.kind.clone()));
        m.insert("message".into(), Value::from(self.message.clone()));
        if let Some(accept) = &self.accept {
            m.insert("accept".into(), accept.clone());
        }
        if let Some(denied) = &self.denied {
            m.insert("denied".into(), denied.clone());
        }
        if let Some(status) = self.status {
            m.insert("status".into(), Value::from(status));
        }
        Value::Object(m)
    }

    /// Reads a record back. Anything that is not one becomes a `RemoteError` carrying its text.
    pub fn from_record(record: &Value) -> NodeError {
        let Some(m) = record.as_object() else {
            return NodeError::new(REMOTE_ERROR, record.to_string());
        };
        NodeError {
            kind: m
                .get("$error")
                .and_then(Value::as_str)
                .unwrap_or(REMOTE_ERROR)
                .to_string(),
            message: m.get("message").and_then(Value::as_str).unwrap_or_default().to_string(),
            accept: m.get("accept").cloned(),
            denied: m.get("denied").cloned(),
            status: m
                .get("status")
                .and_then(Value::as_u64)
                .and_then(|s| u16::try_from(s).ok()),
        }
    }
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for NodeError {}

/// What can go wrong on the network itself.
#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("frame: {0}")]
    Frame(String),
    #[error("handshake: {0}")]
    Handshake(String),
    #[error("refused: {0}")]
    Refused(String),
    #[error("{0}")]
    Unreachable(String),
    #[error("the synapse closed")]
    Closed,
    #[error("timed out: {0}")]
    Timeout(String),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("{0}")]
    Remote(NodeError),
}

/// A node's code that reached another node answers its own caller with what went wrong: the other node's own
/// refusal as it came, or the network's failure as a `NetError` record.
impl From<NetError> for NodeError {
    fn from(e: NetError) -> NodeError {
        match e {
            NetError::Remote(e) => e,
            other => NodeError::new("NetError", other.to_string()),
        }
    }
}
