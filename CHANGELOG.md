# Changelog

## 0.2.0 (2026-11-09)

The Python bridge, `liminal-net-py` (decision 62): a Python object as a node on the network, built with PyO3 0.30
as one abi3 wheel for Python 3.12 and up (`liminal_net`).
- `Node(name, kind, scope, hillock, interface, ipc_dir=None, timing=None)`: `serve`, `start` and `join`, `stop`,
  `fire`, `reach`, `lend`, `call`, `release`, `latest`, `receive`.
- `Remote`: `request(method, *args, **kwargs)`, `link`, `unlink`, `tune`, `detach`, `closed`.
- Python's values cross as the wire's types: a set as a list, bytes as base64, a time as ISO-8601 text, NaN as null.
- A Python exception crosses as a record, and comes back as `RemoteError` with its kind, words, `accept`, `denied`
  and `status`. The network's own failures are `NetworkError`.
- Every call into the network lets go of the interpreter's lock while it waits.

In liminal-net, a node's admission and its receiver's functions now run on worker threads, as requests do. They may
be Python's.

12 Python tests, among them a live check of the digests against host.py's own.

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
