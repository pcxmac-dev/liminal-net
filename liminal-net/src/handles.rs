//! Handles (NETWORK.md §4.1): how a node or a callable is passed in a message. A handle is answered at its owner's
//! dendrites, and released explicitly when done (decisions 17 and 44).

use crate::errors::{NO_SUCH_HANDLE, NodeError};
use crate::message::Target;
use crate::{Map, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// A callable lent to other nodes.
pub type HandleFn = Arc<dyn Fn(Vec<Value>, Map) -> Result<Value, NodeError> + Send + Sync>;

struct Held {
    function: HandleFn,
    /// The synapse it was lent over: it is released when that synapse closes.
    synapse: Option<u64>,
}

/// A node's table of what it has lent.
#[derive(Default)]
pub struct Handles {
    next: AtomicU64,
    table: Mutex<HashMap<u64, Held>>,
}

impl Handles {
    pub fn register(&self, function: HandleFn, synapse: Option<u64>) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.table.lock().unwrap().insert(id, Held { function, synapse });
        id
    }

    pub fn call(&self, id: u64, args: Vec<Value>, kwargs: Map) -> Result<Value, NodeError> {
        let function = self.table.lock().unwrap().get(&id).map(|held| held.function.clone());
        match function {
            Some(function) => function(args, kwargs),
            None => Err(NodeError::new(NO_SUCH_HANDLE, format!("handle {id} is no longer held"))),
        }
    }

    pub fn release(&self, ids: &[u64]) {
        let mut table = self.table.lock().unwrap();
        for id in ids {
            table.remove(id);
        }
    }

    /// Everything lent over this synapse.
    pub fn release_synapse(&self, synapse: u64) {
        self.table
            .lock()
            .unwrap()
            .retain(|_, held| held.synapse != Some(synapse));
    }

    pub fn len(&self) -> usize {
        self.table.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A handle as it travels: `{"$node": id, "traits": [...], "owner": {"node": ..., "scope": ...}}`.
#[derive(Debug, Clone, PartialEq)]
pub struct HandleRef {
    pub id: u64,
    pub owner: Target,
    pub traits: Vec<String>,
}

impl HandleRef {
    pub fn to_value(&self) -> Value {
        serde_json::json!({
            "$node": self.id,
            "traits": self.traits,
            "owner": {"node": self.owner.node, "scope": self.owner.scope},
        })
    }

    pub fn from_value(value: &Value) -> Option<HandleRef> {
        let id = value.get("$node")?.as_u64()?;
        let owner = value.get("owner")?;
        let owner = Target::new(owner.get("node")?.as_str()?, owner.get("scope")?.as_str()?);
        let traits = value
            .get("traits")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(|t| t.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        Some(HandleRef { id, owner, traits })
    }
}
