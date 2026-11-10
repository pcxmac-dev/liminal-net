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

0.2.0, 2026-11-09:
- `liminal-net`: the network itself (0.1);
- `liminal-net-py`: the Python bridge (0.2), so the Python not yet replaced takes part as nodes.

Nothing in Liminal uses either yet.

## Build and test

```
cargo build
cargo test -p liminal-net
cargo clippy --all-targets --workspace
cargo fmt --check
```

The tests serve nodes in tasks, over a scratch key folder and under names of their own. They start no process and
reach no network. `tests/vectors.rs` checks the test vectors of NETWORK.md §5.7, which today's Python computed.

The bridge is built with maturin into a virtual environment, then tested from Python:

```
cd liminal-net-py
maturin develop --release
python -m pytest -q -p no:cacheprovider tests
```

It is one abi3 wheel per platform, for Python 3.12 and up. Building it needs `PYO3_PYTHON` (or an active
environment) pointing at a Python. `cargo clippy --workspace` needs it too.

CI (`.github/workflows/ci.yml`) runs all of this on Ubuntu and on Windows, with Rust 1.99.1: the format check, the
lints and the network's tests; then the bridge, built as pip builds it, on Python 3.12 and 3.15. It checks out
liminal-libs beside this repository, as on a working PC, so that the bridge's digests are compared with
`host.py`'s own. Without liminal-libs beside it, that one test is skipped.

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

## A node in Python, through the bridge

```python
import liminal_net

class Greeter:
    def greet(self, name):
        return f"hello {name}"

interface = {"trait": "example.Greeter",
             "methods": {"greet": {"args": [{"name": "name", "type": "text"}], "returns": "text"}}}
node = liminal_net.Node("greeter", "example", r"C:\data\liminal.db", Greeter(), interface)
node.start()

caller = liminal_net.Node("caller", "app", r"C:\data\liminal.db", object(), {"trait": "example.Caller"})
print(caller.reach("greeter").request("greet", "you"))   # hello you
```

- The object's methods answer the interface's methods (or one `request(peer, method, args, kwargs)` answers
  them all). It may also have `integrate`, `admits`, `attach`, `detach` and `shutdown`.
- A Python exception crosses as a record: its class's name, its words, and its `accept`, `denied` and `status`.
  It comes back as `liminal_net.RemoteError`.
- `node.fire(signal, data)`, `remote.link(signal, functions, receiver)`, `node.lend(callable)` and `node.call(handle,
  ...)` work as in Rust. Every call lets go of the interpreter's lock while it waits.
