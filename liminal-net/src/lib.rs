//! liminal-net: Liminal's network of nodes.
//!
//! Every code object of Liminal becomes a stateful node on the [`Hillock`] trait.
//! - A node's **dendrites** take its input: they accept synapses from other nodes' axons.
//! - Its **axon** carries its output: requests out and their answers back, and the node's signals to the nodes
//!   linked for them.
//! - At each **synapse**, synaptic functions filter and order the stimulus.
//!
//! The protocol is language-neutral (JSON frames, an HMAC handshake, typed records, handles), so Rust, C++ and,
//! while it remains, Python take part alike. The design is `docs/NETWORK.md`.

// A refusal crosses with its words, `accept` and `denied` (NETWORK.md §4.1), so the error type is large. Errors are
// the rare path, and boxing them would only make every node's code harder to read.
#![allow(clippy::result_large_err)]

pub mod address;
pub mod axon;
pub mod dendrites;
pub mod errors;
pub mod frame;
pub mod functions;
pub mod handles;
pub mod handshake;
pub mod hillock;
pub mod idl;
pub mod key;
pub mod lifecycle;
pub mod message;
pub mod node;
pub mod synapse;
pub mod transport;

pub use axon::{CommandLauncher, Launcher, START_FLAGS};
pub use errors::{NetError, NodeError};
pub use functions::{Chain, Stimulus, SynapticFunction};
pub use handles::HandleRef;
pub use hillock::{Call, Hillock};
pub use idl::{Interface, SignalKind};
pub use key::Key;
pub use lifecycle::Timing;
pub use message::{Op, Peer, Target};
pub use node::{Node, NodeConfig};
pub use synapse::Remote;

/// A message's value on the wire: JSON, with a map's keys kept in the order they were put in (decision 71).
pub type Value = serde_json::Value;
/// A JSON map whose keys keep their order.
pub type Map = serde_json::Map<String, Value>;

/// The protocol's version, sent in the challenge and the hello (NETWORK.md §5.6).
pub const PROTO: u64 = 1;
