//! A synapse seen from the axon's end: one connection to another node's dendrites, its requests matched by id
//! (decision 48), many in flight at once.

use crate::errors::{NetError, NodeError};
use crate::frame;
use crate::functions::Chain;
use crate::message::{Op, Target};
use crate::node::Inner;
use crate::transport::Conn;
use crate::{Map, Value};
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Notify, mpsc, oneshot};
use tokio::time::Instant;

type Reply = oneshot::Sender<Result<Value, NetError>>;

/// Another node, held through a synapse from this node's axon. Its methods go out as requests and come back
/// answered. Clones share the one synapse.
#[derive(Clone)]
pub struct Remote {
    pub(crate) inner: Arc<Out>,
}

pub(crate) struct Out {
    pub id: u64,
    pub target: Target,
    /// The reaching node itself: where a link's signals come back to.
    pub back_to: Target,
    pub pid: u32,
    pub interface: Value,
    tx: mpsc::UnboundedSender<Value>,
    pending: Mutex<HashMap<u64, Reply>>,
    next: AtomicU64,
    open: AtomicBool,
    last_used: Mutex<Instant>,
    links: AtomicUsize,
    closed: Notify,
    node: Weak<Inner>,
}

impl Out {
    fn settle(&self, id: u64, result: Result<Value, NetError>) {
        if let Some(reply) = self.pending.lock().unwrap().remove(&id) {
            let _ = reply.send(result);
        }
    }

    fn close(&self) {
        self.open.store(false, Ordering::SeqCst);
        let pending: Vec<Reply> = self.pending.lock().unwrap().drain().map(|(_, reply)| reply).collect();
        for reply in pending {
            let _ = reply.send(Err(NetError::Closed));
        }
        if let Some(node) = self.node.upgrade() {
            node.handles.release_synapse(self.id);
            node.forget_remote(&self.target, self.id);
        }
        self.closed.notify_waiters();
    }

    fn touch(&self) {
        *self.last_used.lock().unwrap() = Instant::now();
    }

    /// Whether something still holds the synapse: a request in flight, a link made over it, or links of this node's
    /// own whose signals it carries.
    fn held(&self) -> bool {
        self.links.load(Ordering::SeqCst) > 0
            || !self.pending.lock().unwrap().is_empty()
            || self.node.upgrade().is_some_and(|node| node.links_to(&self.target))
    }
}

impl Remote {
    pub(crate) fn start(node: &Arc<Inner>, id: u64, conn: Conn, target: Target, pid: u32, interface: Value) -> Remote {
        let (mut reader, mut writer) = tokio::io::split(conn);
        let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
        let out = Arc::new(Out {
            id,
            target,
            back_to: node.target(),
            pid,
            interface,
            tx,
            pending: Mutex::new(HashMap::new()),
            next: AtomicU64::new(0),
            open: AtomicBool::new(true),
            last_used: Mutex::new(Instant::now()),
            links: AtomicUsize::new(0),
            closed: Notify::new(),
            node: Arc::downgrade(node),
        });
        tokio::spawn(async move {
            while let Some(value) = rx.recv().await {
                let detach = value.get("op").and_then(Value::as_str) == Some("detach");
                if frame::write(&mut writer, &value).await.is_err() || detach {
                    break;
                }
            }
            let _ = writer.shutdown().await;
        });
        let reading = out.clone();
        tokio::spawn(async move {
            while let Ok(Some(value)) = frame::read(&mut reader).await {
                match Op::from_value(value) {
                    Ok(Op::Answer { id, value }) => reading.settle(id, Ok(value)),
                    Ok(Op::Error { id, error }) => {
                        reading.settle(id, Err(NetError::Remote(NodeError::from_record(&error))))
                    }
                    _ => {}
                }
            }
            reading.close();
        });
        let watching = out.clone();
        let quiet = node.config.timing.quiet;
        tokio::spawn(async move {
            let step = (quiet / 4).clamp(Duration::from_millis(10), Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = watching.closed.notified() => break,
                    _ = tokio::time::sleep(step) => {}
                }
                if !watching.open.load(Ordering::SeqCst) {
                    break;
                }
                if watching.last_used.lock().unwrap().elapsed() >= quiet && !watching.held() {
                    Remote {
                        inner: watching.clone(),
                    }
                    .detach();
                    break;
                }
            }
        });
        Remote { inner: out }
    }

    /// The node reached.
    pub fn target(&self) -> &Target {
        &self.inner.target
    }

    /// Its process.
    pub fn pid(&self) -> u32 {
        self.inner.pid
    }

    /// Its interface description, as it was admitted with.
    pub fn interface(&self) -> &Value {
        &self.inner.interface
    }

    pub fn is_open(&self) -> bool {
        self.inner.open.load(Ordering::SeqCst)
    }

    async fn ask(&self, op: impl FnOnce(u64) -> Op) -> Result<Value, NetError> {
        if !self.is_open() {
            return Err(NetError::Closed);
        }
        let id = self.inner.next.fetch_add(1, Ordering::SeqCst) + 1;
        let (reply, answer) = oneshot::channel();
        self.inner.pending.lock().unwrap().insert(id, reply);
        self.inner.touch();
        if self.inner.tx.send(op(id).to_value()).is_err() {
            self.inner.pending.lock().unwrap().remove(&id);
            return Err(NetError::Closed);
        }
        let result = answer.await.unwrap_or(Err(NetError::Closed));
        self.inner.touch();
        result
    }

    /// Requests one of the node's interface methods.
    pub async fn request(&self, method: &str, args: Vec<Value>, kwargs: Map) -> Result<Value, NetError> {
        let node = self.inner.target.node.clone();
        let method = method.to_string();
        self.ask(move |id| Op::Request {
            id,
            node: Some(node),
            handle: None,
            method,
            args,
            kwargs,
        })
        .await
    }

    /// The blocking form, for a node's own code on its worker threads (never on an async task).
    pub fn request_blocking(&self, method: &str, args: Vec<Value>, kwargs: Map) -> Result<Value, NetError> {
        block(self.request(method, args, kwargs))
    }

    /// Calls a handle this node lent: at the node's dendrites.
    pub async fn call_handle(&self, handle: u64, args: Vec<Value>, kwargs: Map) -> Result<Value, NetError> {
        self.ask(move |id| Op::Request {
            id,
            node: None,
            handle: Some(handle),
            method: "call".into(),
            args,
            kwargs,
        })
        .await
    }

    /// Asks the node to extend its axon to this node's dendrites for `signal`. `functions` are the built-ins it runs
    /// at its end, before the signal is ever sent.
    pub async fn link(&self, signal: &str, functions: Vec<Value>) -> Result<(), NetError> {
        let to = self.inner.back_to.clone();
        let signal_name = signal.to_string();
        self.ask(move |id| Op::Link {
            id,
            signal: signal_name,
            to,
            functions,
        })
        .await?;
        self.inner.links.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    /// `link`, with this node's own functions at its end of the link, run on every stimulus before it is integrated.
    pub async fn link_with(&self, signal: &str, functions: Vec<Value>, receiver: Chain) -> Result<(), NetError> {
        if let Some(node) = self.inner.node.upgrade() {
            node.set_receiver(&self.inner.target.node, signal, receiver);
        }
        self.link(signal, functions).await
    }

    /// The reverse of `link`.
    pub async fn unlink(&self, signal: &str) -> Result<(), NetError> {
        let to = self.inner.back_to.clone();
        let signal_name = signal.to_string();
        self.ask(move |id| Op::Unlink {
            id,
            signal: signal_name,
            to,
        })
        .await?;
        let _ = self
            .inner
            .links
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
        Ok(())
    }

    /// Replaces a live link's functions at the node's end (decision 52).
    pub async fn tune(&self, signal: &str, functions: Vec<Value>) -> Result<(), NetError> {
        let to = self.inner.back_to.clone();
        let signal_name = signal.to_string();
        self.ask(move |id| Op::Tune {
            id,
            signal: signal_name,
            to,
            functions,
        })
        .await
        .map(drop)
    }

    /// Tells the node these handles of its are no longer held.
    pub fn release(&self, handles: Vec<u64>) {
        let _ = self.inner.tx.send(Op::Release { handles }.to_value());
    }

    pub(crate) fn send(&self, op: &Op) -> bool {
        self.is_open() && self.inner.tx.send(op.to_value()).is_ok()
    }

    /// Lets go of the synapse. Detaching twice does nothing more.
    pub fn detach(&self) {
        if self.inner.open.swap(false, Ordering::SeqCst) {
            let _ = self.inner.tx.send(Op::Detach.to_value());
        }
    }

    /// Waits until the synapse has closed.
    pub async fn closed(&self) {
        while self.is_open() || !self.inner.pending.lock().unwrap().is_empty() {
            tokio::select! {
                _ = self.inner.closed.notified() => {}
                _ = tokio::time::sleep(Duration::from_millis(20)) => {}
            }
        }
    }
}

impl std::fmt::Debug for Remote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Remote")
            .field("target", &self.inner.target)
            .field("pid", &self.inner.pid)
            .field("open", &self.is_open())
            .finish()
    }
}

/// Runs an async call from a node's own code, on a worker thread.
pub(crate) fn block<F: Future>(future: F) -> F::Output {
    tokio::runtime::Handle::current().block_on(future)
}
