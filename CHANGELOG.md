# Changelog

## 0.1.0 (2026-11-09)

The network in Rust, as `docs/NETWORK.md` designs it:
- frames: a 4-byte length, then JSON, at most 16 MB;
- the handshake: HMAC-SHA256 proofs with the user's key, the reached side challenging first;
- host.py's key file, made once with create-new, and host.py's digest under the network's own `liminal-net-` prefix;
- named pipes on Windows, one node per name, with an access list for this user only; Unix sockets elsewhere;
- the messages, peers, targets and typed records;
- the `Hillock` trait, and the node runtime:
  - the dendrites: an admission, requests run concurrently on worker threads, signals in order;
  - the axon: reaching nodes, starting a resident one windowless, waiting for one securing its data;
  - the lifecycle: the start wait, the grace, the quiet end, data secured before the end;
- the eight built-in synaptic functions, and closures at the receiver's end;
- state signals coalesced to the latest; a new link given a state's latest at once;
- handles, released explicitly or with their synapse;
- `$error` records;
- interface descriptions.

48 tests, among them the test vectors computed by today's Python.
