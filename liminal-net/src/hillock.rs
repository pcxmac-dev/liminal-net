//! The Hillock (NETWORK.md §2): the trait every node realizes, the code that interlinks a node with the others.

use crate::errors::NodeError;
use crate::idl::Interface;
use crate::message::Peer;
use crate::node::Node;
use crate::{Map, Value};

/// What a node's code is told with a request or a signal: who sent it, and the node itself, to fire from or to
/// reach other nodes from.
pub struct Call<'a> {
    pub peer: &'a Peer,
    pub node: &'a Node,
}

/// A node's own code.
///
/// Its methods are plain functions, run on worker threads (decision 60). They may block, and reach other nodes with
/// the blocking forms (`Node::reach_blocking`, `Remote::request_blocking`).
pub trait Hillock: Send + Sync + 'static {
    /// The node's interface: its methods and its signals. Its dendrites serve nothing else.
    fn interface(&self) -> Interface;

    /// Answers a request for one of the interface's methods.
    fn request(&self, call: &Call<'_>, method: &str, args: Vec<Value>, kwargs: Map) -> Result<Value, NodeError>;

    /// What a signal arriving at the dendrites does to the node's state. The runtime has already kept it as the
    /// latest of its source and signal (`Node::latest`); passing it on is the node's own choice (`Node::fire`).
    fn integrate(&self, _call: &Call<'_>, _signal: &str, _data: &Value) {}

    /// Whose axons the dendrites admit (decision 18): `Err(why)` refuses, with those words.
    fn admits(&self, _peer: &Peer) -> Result<(), String> {
        Ok(())
    }

    /// A synapse was admitted.
    fn attach(&self, _peer: &Peer) {}

    /// A synapse closed.
    fn detach(&self, _peer: &Peer) {}

    /// Secures the node's state before its process ends.
    fn shutdown(&self) {}
}
