//! What the network tests share: scratch folders, quick times, and two small nodes.
//!
//! No test starts a process or reaches the network: nodes are served in tasks of the test's own runtime, over a
//! scratch key folder, under names no other test uses.

#![allow(dead_code)]

use liminal_net::{Call, Hillock, Interface, Map, NodeConfig, NodeError, Peer, SignalKind, Timing, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A scratch folder, removed when dropped.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let name = format!(
            "liminal-net-test-{}-{}",
            std::process::id(),
            &liminal_net::handshake::nonce()[..12]
        );
        let dir = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Times short enough for tests.
pub fn quick() -> Timing {
    Timing {
        start_wait: Duration::from_secs(5),
        grace: Duration::from_millis(200),
        hello_wait: Duration::from_secs(1),
        quiet: Duration::from_secs(60),
        reach_wait: Duration::from_secs(5),
        coalesce: Duration::from_millis(100),
    }
}

/// A node's config in a scratch folder: its own key, a scope of its own, quick times.
pub fn config(dir: &TempDir, name: &str, kind: &str) -> NodeConfig {
    let ipc_dir = dir.path().join("ipc");
    let key = liminal_net::key::load_or_create(&ipc_dir).unwrap();
    NodeConfig {
        name: name.to_string(),
        kind: kind.to_string(),
        scope: dir.path().join("scope.db"),
        ipc_dir,
        key,
        timing: quick(),
    }
}

/// A launcher that starts nothing: `reach` with it waits for a node that is still binding.
pub fn wait_for_it() -> std::io::Result<()> {
    Ok(())
}

/// A node with a few methods and two signals.
#[derive(Default)]
pub struct Echo {
    pub secure_for: Duration,
    pub secured: Arc<AtomicUsize>,
    pub only_kind: Option<String>,
    pub calls: Arc<AtomicUsize>,
}

impl Hillock for Echo {
    fn interface(&self) -> Interface {
        Interface::new("test.Echo")
            .method("echo", &[("value", "any")], "any")
            .method("slow", &[("ms", "int")], "text")
            .method("fail", &[], "null")
            .method("callback", &[("log", "node:callable")], "any")
            .method("panic", &[], "null")
            .signal("status", SignalKind::State, "map")
            .signal("lines", SignalKind::Stream, "any")
    }

    fn request(&self, call: &Call<'_>, method: &str, args: Vec<Value>, _kwargs: Map) -> Result<Value, NodeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match method {
            "echo" => Ok(args.into_iter().next().unwrap_or(Value::Null)),
            "slow" => {
                let ms = args.first().and_then(Value::as_u64).unwrap_or(200);
                std::thread::sleep(Duration::from_millis(ms));
                Ok(Value::from("slow"))
            }
            "fail" => Err(NodeError::new("CommandError", "the command failed")
                .accept(Value::Bool(true))
                .status(400)),
            "callback" => {
                let handle = liminal_net::HandleRef::from_value(&args[0]).expect("a handle");
                call.node
                    .call_blocking(&handle, vec![Value::from("from echo")], Map::new())
                    .map_err(NodeError::from)
            }
            "panic" => panic!("on purpose"),
            other => unreachable!("{other} is not in the interface"),
        }
    }

    fn admits(&self, peer: &Peer) -> Result<(), String> {
        match &self.only_kind {
            Some(kind) if &peer.kind != kind => Err(format!("only {kind} nodes")),
            _ => Ok(()),
        }
    }

    fn shutdown(&self) {
        std::thread::sleep(self.secure_for);
        self.secured.fetch_add(1, Ordering::SeqCst);
    }
}

/// A node that only receives: it records every signal it integrates.
#[derive(Default, Clone)]
pub struct Recorder {
    pub got: Arc<Mutex<Vec<(String, Value)>>>,
}

impl Recorder {
    /// Waits until `n` signals came, or `within` passed, and gives back what came.
    pub async fn wait(&self, n: usize, within: Duration) -> Vec<(String, Value)> {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            let got = self.got.lock().unwrap().clone();
            if got.len() >= n || tokio::time::Instant::now() >= deadline {
                return got;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub fn data(&self) -> Vec<Value> {
        self.got.lock().unwrap().iter().map(|(_, d)| d.clone()).collect()
    }
}

impl Hillock for Recorder {
    fn interface(&self) -> Interface {
        Interface::new("test.Recorder")
    }

    fn request(&self, _call: &Call<'_>, method: &str, _args: Vec<Value>, _kwargs: Map) -> Result<Value, NodeError> {
        Err(NodeError::new("NotInInterface", method))
    }

    fn integrate(&self, _call: &Call<'_>, signal: &str, data: &Value) {
        self.got.lock().unwrap().push((signal.to_string(), data.clone()));
    }
}

/// Polls until `check` holds, or `within` passed.
pub async fn eventually(within: Duration, check: impl Fn() -> bool) -> bool {
    let deadline = tokio::time::Instant::now() + within;
    while tokio::time::Instant::now() < deadline {
        if check() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    check()
}
