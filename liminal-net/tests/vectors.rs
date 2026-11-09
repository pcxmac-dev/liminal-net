//! Test vectors (NETWORK.md §5.7), computed by today's Python (host.py, hmac, json) on 2026-11-09, and the wire's
//! shapes and types as NETWORK.md writes them.

use liminal_net::address::{address, digest};
use liminal_net::frame::{self, encode};
use liminal_net::handshake::{AXON_LABEL, DENDRITES_LABEL, hex, proof, verify};
use liminal_net::idl::{Arg, Interface, SignalKind};
use liminal_net::message::{Op, Peer, Target};
use liminal_net::{Key, Map, NodeError, Value};
use serde_json::json;
use std::path::Path;

#[cfg(windows)]
#[test]
fn digests_match_host_py() {
    let vectors = [
        (r"C:\no-such-liminal-dir\Scope\liminal.db", "13c27f2a47"),
        (r"C:\No-Such-Liminal\a\..\B\liminal.db", "cc0ae09088"),
        ("C:/no-such-liminal-dir/forward/x.db", "7f2f3803d7"),
        ("C:\\no-such-liminal-dir\\Ünïcode\\liminal.db", "fddc7c1fb7"),
    ];
    for (path, want) in vectors {
        assert_eq!(digest(Path::new(path)), want, "{path}");
    }
    assert_eq!(
        address(
            "fetcher",
            Path::new(r"C:\no-such-liminal-dir\Scope\liminal.db"),
            Path::new("unused")
        ),
        r"\\.\pipe\liminal-net-fetcher-13c27f2a47"
    );
}

#[cfg(unix)]
#[test]
fn digests_match_host_py() {
    assert_eq!(digest(Path::new("/no-such-liminal-dir/Scope/liminal.db")), "405e53966f");
    assert_eq!(
        address(
            "fetcher",
            Path::new("/no-such-liminal-dir/Scope/liminal.db"),
            Path::new("/run/ipc")
        ),
        "/run/ipc/net-fetcher-405e53966f.sock"
    );
}

#[test]
fn a_frame_is_its_length_then_its_json() {
    let detach = Op::Detach.to_value();
    assert_eq!(hex(&encode(&detach).unwrap()), "0000000f7b226f70223a22646574616368227d");
}

#[test]
fn proofs_match_python_hmac() {
    let key = Key::from_bytes("0123456789abcdef".repeat(4));
    let n1 = "00".repeat(32);
    let n2 = "ff".repeat(32);
    let axon = "a7566505e02b8794c77e41223e022b20105e61a1908d95c81980e8f2df063b42";
    let dendrites = "c17a34bff52e829e8caee98530782b12fa62bdd86d0cdde4d27fb597f496ccb0";
    assert_eq!(proof(&key, AXON_LABEL, &n1), axon);
    assert_eq!(proof(&key, DENDRITES_LABEL, &n2), dendrites);
    assert!(verify(&key, AXON_LABEL, &n1, axon));
    assert!(
        !verify(&key, DENDRITES_LABEL, &n1, axon),
        "a proof is bound to its side's label"
    );
    assert!(
        !verify(&Key::from_bytes("x".repeat(64)), AXON_LABEL, &n1, axon),
        "and to the key"
    );
    assert!(!verify(&key, AXON_LABEL, &n1, "not hex"));
}

#[test]
fn messages_are_shaped_as_the_design_writes_them() {
    let request = Op::Request {
        id: 7,
        node: Some("wsl-ubuntu".into()),
        handle: None,
        method: "status".into(),
        args: vec![],
        kwargs: Map::new(),
    };
    assert_eq!(
        serde_json::to_string(&request.to_value()).unwrap(),
        r#"{"op":"request","id":7,"node":"wsl-ubuntu","method":"status","args":[],"kwargs":{}}"#
    );
    let fire = Op::Fire {
        signal: "status".into(),
        from: "wsl-ubuntu".into(),
        seq: 42,
        data: json!({"valid": true}),
    };
    assert_eq!(
        serde_json::to_string(&fire.to_value()).unwrap(),
        r#"{"op":"fire","signal":"status","from":"wsl-ubuntu","seq":42,"data":{"valid":true}}"#
    );
    let link = Op::Link {
        id: 3,
        signal: "status".into(),
        to: Target::new("platform", "C:/x"),
        functions: vec![json!({"fn": "changed", "fields": ["valid", "state", "summary"]})],
    };
    assert_eq!(
        serde_json::to_string(&link.to_value()).unwrap(),
        r#"{"op":"link","id":3,"signal":"status","to":{"node":"platform","scope":"C:/x"},"functions":[{"fn":"changed","fields":["valid","state","summary"]}]}"#
    );
    let hello = Op::Hello {
        node: "wsl-ubuntu".into(),
        from: Peer {
            name: "platform".into(),
            kind: "app".into(),
            scope: "s".into(),
            pid: 1234,
        },
        proto: 1,
    };
    assert_eq!(
        serde_json::to_string(&hello.to_value()).unwrap(),
        r#"{"op":"hello","node":"wsl-ubuntu","from":{"name":"platform","kind":"app","scope":"s","pid":1234},"proto":1}"#
    );
    assert_eq!(
        serde_json::to_string(&Op::Release { handles: vec![12, 13] }.to_value()).unwrap(),
        r#"{"op":"release","handles":[12,13]}"#
    );
    assert_eq!(
        serde_json::to_string(
            &Op::Refused {
                why: "no".into(),
                retry: false
            }
            .to_value()
        )
        .unwrap(),
        r#"{"op":"refused","why":"no"}"#
    );
    // And back.
    let back = Op::from_value(serde_json::from_str(r#"{"op":"request","id":1,"method":"m"}"#).unwrap()).unwrap();
    assert_eq!(
        back,
        Op::Request {
            id: 1,
            node: None,
            handle: None,
            method: "m".into(),
            args: vec![],
            kwargs: Map::new()
        }
    );
}

#[test]
fn the_wire_types_keep_ints_floats_order_and_no_nan() {
    assert_eq!(serde_json::to_string(&json!(1)).unwrap(), "1");
    assert_eq!(serde_json::to_string(&json!(1.0)).unwrap(), "1.0");
    let ns: u64 = 1_794_000_000_123_456_789;
    assert_eq!(
        serde_json::to_string(&json!(ns)).unwrap(),
        "1794000000123456789",
        "a nanosecond time stays exact"
    );
    assert_eq!(Value::from(f64::NAN), Value::Null);
    assert_eq!(Value::from(f64::INFINITY), Value::Null);
    let text = r#"{"z":1,"a":2,"m":{"y":0,"b":1}}"#;
    let value: Value = serde_json::from_str(text).unwrap();
    assert_eq!(serde_json::to_string(&value).unwrap(), text, "keys keep their order");
}

#[test]
fn errors_cross_as_records() {
    let error = NodeError::new("CommandError", "wsl.exe answered 1")
        .accept(json!("--force"))
        .denied(json!(["delete"]))
        .status(400);
    let record = error.to_record();
    assert_eq!(
        serde_json::to_string(&record).unwrap(),
        r#"{"$error":"CommandError","message":"wsl.exe answered 1","accept":"--force","denied":["delete"],"status":400}"#
    );
    assert_eq!(NodeError::from_record(&record), error);
    assert_eq!(NodeError::from_record(&json!("text only")).kind, "RemoteError");
}

#[test]
fn an_interface_is_described_as_the_design_writes_it() {
    let mut interface = Interface::new("liminal.MachinePlugin")
        .base("liminal.PlatformPlugin")
        .method("present", &[], "bool")
        .method("status", &[], "map")
        .signal("status", SignalKind::State, "map");
    interface.methods.insert(
        "setup".into(),
        liminal_net::idl::Method {
            args: vec![
                Arg {
                    name: "log".into(),
                    ty: "node:callable".into(),
                    default: None,
                },
                Arg {
                    name: "steps".into(),
                    ty: "list<text>|null".into(),
                    default: Some(Value::Null),
                },
            ],
            returns: "list<map>".into(),
            signature: None,
        },
    );
    let want = json!({"trait": "liminal.MachinePlugin", "base": ["liminal.PlatformPlugin"],
        "methods": {"present": {"args": [], "returns": "bool"},
                    "status": {"args": [], "returns": "map"},
                    "setup": {"args": [{"name": "log", "type": "node:callable"},
                                       {"name": "steps", "type": "list<text>|null", "default": null}],
                              "returns": "list<map>"}},
        "signals": {"status": {"kind": "state", "data": "map"}}});
    assert_eq!(
        serde_json::to_string(&interface.to_value()).unwrap(),
        serde_json::to_string(&want).unwrap()
    );
    let back: Interface = serde_json::from_value(want).unwrap();
    assert_eq!(
        back.methods["setup"].args[1].default,
        Some(Value::Null),
        "a null default is a default"
    );
    assert_eq!(back.methods["setup"].args[0].default, None);
}

#[tokio::test]
async fn a_frame_larger_than_16_mb_is_refused() {
    let big = Value::from("x".repeat(frame::MAX_FRAME));
    assert!(matches!(encode(&big), Err(liminal_net::NetError::Frame(_))));
    let mut header = std::io::Cursor::new((20u32 * 1024 * 1024).to_be_bytes().to_vec());
    assert!(matches!(
        frame::read(&mut header).await,
        Err(liminal_net::NetError::Frame(_))
    ));
    let mut nothing = std::io::Cursor::new(Vec::<u8>::new());
    assert!(
        matches!(frame::read(&mut nothing).await, Ok(None)),
        "a clean end between frames"
    );
}

#[test]
fn the_key_is_made_once_and_shared() {
    let dir = std::env::temp_dir().join(format!("liminal-net-key-{}", &liminal_net::handshake::nonce()[..12]));
    let made: Vec<Vec<u8>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..8)
            .map(|_| s.spawn(|| liminal_net::key::load_or_create(&dir).unwrap().bytes().to_vec()))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert!(
        made.iter().all(|k| k == &made[0]),
        "eight first runs at once end with one key"
    );
    assert_eq!(made[0].len(), 64);
    assert!(made[0].iter().all(u8::is_ascii_hexdigit));
    // An existing key of 32 bytes or more is kept as it is, whitespace stripped.
    std::fs::write(dir.join("key"), format!("  {}\n", "k".repeat(40))).unwrap();
    assert_eq!(
        liminal_net::key::load_or_create(&dir).unwrap().bytes(),
        "k".repeat(40).as_bytes()
    );
    // A shorter one is replaced.
    std::fs::write(dir.join("key"), "short").unwrap();
    assert_eq!(liminal_net::key::load_or_create(&dir).unwrap().bytes().len(), 64);
    let _ = std::fs::remove_dir_all(&dir);
}
