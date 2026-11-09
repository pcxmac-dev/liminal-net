# liminal-net

Liminal's network of nodes, in Rust: the runtime every node links.

- Every code object of Liminal becomes a stateful node on the `Hillock` trait.
- A node's **dendrites** take its input: they accept synapses from other nodes' axons.
- Its **axon** carries its output: requests out and their answers back, and its signals to the nodes linked for
  them.
- At each **synapse**, synaptic functions filter and order the stimulus.
- The protocol is language-neutral: JSON frames, an HMAC handshake with the user's key, typed records, handles.

The design, with every decision and its state, is [docs/NETWORK.md](docs/NETWORK.md).

## Status

0.1.0, 2026-11-09, local only: the network itself. 0.2 adds the Python bridge, so the Python not yet replaced can
take part. Nothing in Liminal uses it yet.

## Build and test

```
cargo build
cargo test
cargo clippy --all-targets
cargo fmt --check
```

The tests serve nodes in tasks, over a scratch key folder and under names of their own. They start no process and
reach no network. `tests/vectors.rs` checks the test vectors of NETWORK.md §5.7, which today's Python computed.

## A node

```rust
use liminal_net::{Call, Hillock, Interface, Map, Node, NodeConfig, NodeError, SignalKind, Value};
use std::sync::Arc;

struct Greeter;

impl Hillock for Greeter {
    fn interface(&self) -> Interface {
        Interface::new("example.Greeter")
            .method("greet", &[("name", "text")], "text")
            .signal("status", SignalKind::State, "map")
    }

    fn request(&self, _call: &Call<'_>, _method: &str, args: Vec<Value>, _kwargs: Map) -> Result<Value, NodeError> {
        Ok(Value::from(format!("hello {}", args[0].as_str().unwrap_or("you"))))
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let config = NodeConfig::from_env("greeter", "example", r"C:\data\liminal.db")?;
    let node = Node::new(config, Arc::new(Greeter));
    println!("ended: {}", node.serve().await);
    Ok(())
}
```

Another node reaches it with `node.reach(&target, launcher)` and calls `remote.request("greet", ...)`. It asks for
its signals with `remote.link("status", functions)`.
