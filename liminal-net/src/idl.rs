//! Interface descriptions (NETWORK.md §6): what a node offers, described language-neutrally.
//!
//! In 0.1 a node builds its description with [`Interface`]'s builder. The derive that writes it from a trait comes
//! with the first traits, in the platform slice.

use crate::Value;
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};

/// A node's interface: its methods and its signals. Its dendrites serve nothing else.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Interface {
    #[serde(rename = "trait")]
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub base: Vec<String>,
    #[serde(default)]
    pub methods: IndexMap<String, Method>,
    #[serde(default)]
    pub signals: IndexMap<String, Signal>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Method {
    #[serde(default)]
    pub args: Vec<Arg>,
    pub returns: String,
    /// The text the pages show today: Python's signature string (decision 86).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Arg {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    /// Present even when it is null: `"default": null` is a default.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
}

fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

/// A state signal is coalesced, the latest winning (decision 20); a stream signal is delivered every one, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignalKind {
    State,
    Stream,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub kind: SignalKind,
    pub data: String,
}

impl Interface {
    pub fn new(name: impl Into<String>) -> Interface {
        Interface {
            name: name.into(),
            ..Default::default()
        }
    }

    pub fn base(mut self, base: impl Into<String>) -> Interface {
        self.base.push(base.into());
        self
    }

    /// A method, its arguments given as (name, type).
    pub fn method(mut self, name: impl Into<String>, args: &[(&str, &str)], returns: impl Into<String>) -> Interface {
        let args = args
            .iter()
            .map(|(name, ty)| Arg {
                name: name.to_string(),
                ty: ty.to_string(),
                default: None,
            })
            .collect();
        self.methods.insert(
            name.into(),
            Method {
                args,
                returns: returns.into(),
                signature: None,
            },
        );
        self
    }

    pub fn signal(mut self, name: impl Into<String>, kind: SignalKind, data: impl Into<String>) -> Interface {
        self.signals.insert(
            name.into(),
            Signal {
                kind,
                data: data.into(),
            },
        );
        self
    }

    /// Whether the interface offers this method.
    pub fn offers(&self, method: &str) -> bool {
        self.methods.contains_key(method)
    }

    pub fn signal_kind(&self, signal: &str) -> Option<SignalKind> {
        self.signals.get(signal).map(|s| s.kind)
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("an interface always serializes")
    }
}
