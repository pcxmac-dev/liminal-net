//! The messages (NETWORK.md §5.4), and what travels in them: peers, targets, typed records.

use crate::errors::NetError;
use crate::{Map, Value};
use serde::{Deserialize, Serialize};

/// Who is reaching, as the hello names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peer {
    pub name: String,
    pub kind: String,
    pub scope: String,
    pub pid: u32,
}

/// A node, by name and scope: what an axon reaches.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Target {
    pub node: String,
    pub scope: String,
}

impl Target {
    pub fn new(node: impl Into<String>, scope: impl Into<String>) -> Target {
        Target {
            node: node.into(),
            scope: scope.into(),
        }
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// One message on a synapse.
///
/// `link`, `unlink` and `tune` carry an id, and are answered like a request: an `answer` with a null value, or an
/// `error`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Op {
    Challenge {
        proto: u64,
        nonce: String,
    },
    Proof {
        mac: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nonce: Option<String>,
    },
    Hello {
        node: String,
        from: Peer,
        proto: u64,
    },
    Admitted {
        node: String,
        pid: u32,
        interface: Value,
    },
    Refused {
        why: String,
        #[serde(default, skip_serializing_if = "is_false")]
        retry: bool,
    },
    Request {
        id: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        handle: Option<u64>,
        method: String,
        #[serde(default)]
        args: Vec<Value>,
        #[serde(default)]
        kwargs: Map,
    },
    Answer {
        id: u64,
        value: Value,
    },
    Error {
        id: u64,
        error: Value,
    },
    Fire {
        signal: String,
        from: String,
        seq: u64,
        data: Value,
    },
    Link {
        id: u64,
        signal: String,
        to: Target,
        #[serde(default)]
        functions: Vec<Value>,
    },
    Unlink {
        id: u64,
        signal: String,
        to: Target,
    },
    Tune {
        id: u64,
        signal: String,
        to: Target,
        #[serde(default)]
        functions: Vec<Value>,
    },
    Release {
        handles: Vec<u64>,
    },
    Detach,
}

impl Op {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("an op always serializes")
    }

    pub fn from_value(value: Value) -> Result<Op, NetError> {
        serde_json::from_value(value).map_err(|e| NetError::Protocol(format!("not a message: {e}")))
    }

    /// The op's name, as it is written on the wire.
    pub fn name(&self) -> &'static str {
        match self {
            Op::Challenge { .. } => "challenge",
            Op::Proof { .. } => "proof",
            Op::Hello { .. } => "hello",
            Op::Admitted { .. } => "admitted",
            Op::Refused { .. } => "refused",
            Op::Request { .. } => "request",
            Op::Answer { .. } => "answer",
            Op::Error { .. } => "error",
            Op::Fire { .. } => "fire",
            Op::Link { .. } => "link",
            Op::Unlink { .. } => "unlink",
            Op::Tune { .. } => "tune",
            Op::Release { .. } => "release",
            Op::Detach => "detach",
        }
    }
}

/// A typed record: `{"$type": name, ...fields}`.
pub fn record(type_name: &str, fields: Map) -> Value {
    let mut m = Map::new();
    m.insert("$type".into(), Value::from(type_name));
    m.extend(fields);
    Value::Object(m)
}

/// A record's type, when the value is one.
pub fn record_type(value: &Value) -> Option<&str> {
    value.get("$type")?.as_str()
}
