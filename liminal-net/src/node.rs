//! The node runtime (NETWORK.md §3): a Hillock served at its dendrites, reaching other nodes through its axon, firing
//! its signals to the nodes linked for them.

use crate::address;
use crate::dendrites::{self, sleep_until};
use crate::errors::{BAD_FUNCTION, NOT_IN_INTERFACE, NetError, NodeError};
use crate::functions::{Chain, Stimulus, Throttle};
use crate::handles::{HandleRef, Handles};
use crate::hillock::{Call, Hillock};
use crate::idl::{Interface, SignalKind};
use crate::key::{self, Key};
use crate::lifecycle::{Stage, Timing, seconds};
use crate::message::{Op, Peer, Target};
use crate::synapse::Remote;
use crate::transport::Listener;
use crate::{Map, Value};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Notify, mpsc};
use tokio::time::Instant;

/// What a node is: its name and kind, its scope, the user's folder and key, and its times.
#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub name: String,
    pub kind: String,
    pub scope: PathBuf,
    pub ipc_dir: PathBuf,
    pub key: Key,
    pub timing: Timing,
}

impl NodeConfig {
    /// A node named `name` in `scope`, with the user's folder and key as host.py finds them (LIMINAL_IPC_DIR).
    pub fn from_env(name: &str, kind: &str, scope: impl Into<PathBuf>) -> io::Result<NodeConfig> {
        let ipc_dir = key::ipc_dir();
        let key = key::load_or_create(&ipc_dir)?;
        Ok(NodeConfig {
            name: name.to_string(),
            kind: kind.to_string(),
            scope: scope.into(),
            ipc_dir,
            key,
            timing: Timing::from_env(),
        })
    }

    /// Where its dendrites listen.
    pub fn address(&self) -> String {
        address::address(&self.name, &self.scope, &self.ipc_dir)
    }

    pub fn target(&self) -> Target {
        Target::new(&self.name, scope_text(&self.scope))
    }
}

/// A scope, as it travels: its path as text.
pub fn scope_text(scope: &Path) -> String {
    scope.to_string_lossy().into_owned()
}

/// A node: its Hillock, served at its dendrites, with its axon to the others.
#[derive(Clone)]
pub struct Node {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct LinkOut {
    pub signal: String,
    pub to: Target,
    pub chain: Chain,
    /// The synapse the link was made over: it goes when that synapse closes.
    pub made_on: u64,
    pub seq: u64,
}

pub(crate) enum Firing {
    Fire { signal: String, data: Value },
    Linked { signal: String, to: Target },
    Wake,
}

pub(crate) struct Inner {
    pub config: NodeConfig,
    pub hillock: Arc<dyn Hillock>,
    pub interface: Interface,
    pub interface_value: Value,
    pub handles: Handles,
    pub links: Mutex<Vec<LinkOut>>,
    pub last_state: Mutex<HashMap<String, Value>>,
    pub receivers: Mutex<HashMap<(String, String), Chain>>,
    pub latest: Mutex<HashMap<(String, String), Value>>,
    pub remotes: Mutex<HashMap<Target, Remote>>,
    pub synapses: AtomicUsize,
    pub busy: AtomicUsize,
    pub ever: AtomicBool,
    pub stage: Mutex<Stage>,
    pub changed: Notify,
    pub stop: Mutex<Option<String>>,
    pub firing: mpsc::UnboundedSender<Firing>,
    pub firing_inbox: Mutex<Option<mpsc::UnboundedReceiver<Firing>>>,
    pub next_synapse: AtomicU64,
}

impl Inner {
    pub fn target(&self) -> Target {
        self.config.target()
    }

    /// Whether this node holds links whose signals go to `target`.
    pub fn links_to(&self, target: &Target) -> bool {
        self.links.lock().unwrap().iter().any(|l| &l.to == target)
    }

    pub fn forget_remote(&self, target: &Target, id: u64) {
        let mut remotes = self.remotes.lock().unwrap();
        if remotes.get(target).is_some_and(|r| r.inner.id == id) {
            remotes.remove(target);
        }
    }

    pub fn set_receiver(&self, from: &str, signal: &str, chain: Chain) {
        self.receivers
            .lock()
            .unwrap()
            .insert((from.to_string(), signal.to_string()), chain);
    }
}

impl Node {
    pub fn new(config: NodeConfig, hillock: Arc<dyn Hillock>) -> Node {
        let interface = hillock.interface();
        let interface_value = interface.to_value();
        let (firing, inbox) = mpsc::unbounded_channel();
        Node {
            inner: Arc::new(Inner {
                config,
                hillock,
                interface,
                interface_value,
                handles: Handles::default(),
                links: Mutex::new(Vec::new()),
                last_state: Mutex::new(HashMap::new()),
                receivers: Mutex::new(HashMap::new()),
                latest: Mutex::new(HashMap::new()),
                remotes: Mutex::new(HashMap::new()),
                synapses: AtomicUsize::new(0),
                busy: AtomicUsize::new(0),
                ever: AtomicBool::new(false),
                stage: Mutex::new(Stage::Running),
                changed: Notify::new(),
                stop: Mutex::new(None),
                firing,
                firing_inbox: Mutex::new(Some(inbox)),
                next_synapse: AtomicU64::new(0),
            }),
        }
    }

    pub fn name(&self) -> &str {
        &self.inner.config.name
    }

    pub fn config(&self) -> &NodeConfig {
        &self.inner.config
    }

    pub fn target(&self) -> Target {
        self.inner.target()
    }

    /// This node, as its hello names it.
    pub fn peer(&self) -> Peer {
        let config = &self.inner.config;
        Peer {
            name: config.name.clone(),
            kind: config.kind.clone(),
            scope: scope_text(&config.scope),
            pid: std::process::id(),
        }
    }

    /// How many synapses its dendrites hold now.
    pub fn synapses(&self) -> usize {
        self.inner.synapses.load(Ordering::SeqCst)
    }

    /// Where it is in its life.
    pub fn stage(&self) -> Stage {
        *self.inner.stage.lock().unwrap()
    }

    /// Serves the node until it ends, and says why it ended:
    /// - another node already answers at its address: it never starts;
    /// - nobody came within the start wait;
    /// - its last synapse went, no request is in progress, and the grace passed;
    /// - `stop` was asked.
    ///
    /// Before it ends it secures its data (`Hillock::shutdown`), refusing new synapses meanwhile, to be retried.
    pub async fn serve(&self) -> String {
        let inner = &self.inner;
        let timing = inner.config.timing.clone();
        let address = inner.config.address();
        let mut listener = match Listener::bind(&address) {
            Ok(listener) => listener,
            Err(e) => return format!("another node answers for {} ({e})", inner.config.name),
        };
        let inbox = inner.firing_inbox.lock().unwrap().take();
        let firing = inbox.map(|inbox| tokio::spawn(fire_loop(self.clone(), inbox)));
        let started = Instant::now();
        let mut quiet_since: Option<Instant> = None;
        let reason = loop {
            let now = Instant::now();
            let in_use = self.synapses() > 0 || inner.busy.load(Ordering::SeqCst) > 0;
            if let Some(reason) = inner.stop.lock().unwrap().take() {
                break reason;
            }
            let deadline = if in_use {
                quiet_since = None;
                None
            } else if !inner.ever.load(Ordering::SeqCst) {
                if now - started >= timing.start_wait {
                    break format!("nobody came within {}", seconds(timing.start_wait));
                }
                Some(started + timing.start_wait)
            } else {
                let since = *quiet_since.get_or_insert(now);
                if now - since >= timing.grace {
                    break format!("no synapse for {}", seconds(timing.grace));
                }
                Some(since + timing.grace)
            };
            tokio::select! {
                accepted = listener.accept() => match accepted {
                    Ok(conn) => {
                        let id = inner.next_synapse.fetch_add(1, Ordering::SeqCst) + 1;
                        tokio::spawn(dendrites::serve(self.clone(), conn, id));
                    }
                    Err(e) => break format!("its dendrites failed ({e})"),
                },
                _ = inner.changed.notified() => {}
                _ = sleep_until(deadline) => {}
            }
        };
        *inner.stage.lock().unwrap() = Stage::Securing;
        let hillock = inner.hillock.clone();
        let mut securing = tokio::task::spawn_blocking(move || hillock.shutdown());
        loop {
            tokio::select! {
                _ = &mut securing => break,
                accepted = listener.accept() => if let Ok(conn) = accepted {
                    let id = inner.next_synapse.fetch_add(1, Ordering::SeqCst) + 1;
                    tokio::spawn(dendrites::serve(self.clone(), conn, id));
                },
            }
        }
        drop(listener);
        *inner.stage.lock().unwrap() = Stage::Ended;
        if let Some(firing) = firing {
            firing.abort();
        }
        let remotes: Vec<Remote> = inner.remotes.lock().unwrap().drain().map(|(_, r)| r).collect();
        for remote in remotes {
            remote.detach();
        }
        reason
    }

    /// Ends the node now, for this reason: the first reason asked wins.
    pub fn stop(&self, reason: impl Into<String>) {
        let mut stop = self.inner.stop.lock().unwrap();
        if stop.is_none() {
            *stop = Some(reason.into());
        }
        drop(stop);
        self.inner.changed.notify_one();
    }

    /// Fires a signal along the axon, to every node linked for it. It may be called from any thread.
    pub fn fire(&self, signal: &str, data: Value) {
        let _ = self.inner.firing.send(Firing::Fire {
            signal: signal.to_string(),
            data,
        });
    }

    /// The latest of a signal this node integrated, from that source.
    pub fn latest(&self, from: &str, signal: &str) -> Option<Value> {
        self.inner
            .latest
            .lock()
            .unwrap()
            .get(&(from.to_string(), signal.to_string()))
            .cloned()
    }

    /// This node's own functions at its end of the links it makes for `signal` from `from` (decision 50).
    pub fn receive(&self, from: &str, signal: &str, chain: Chain) {
        self.inner.set_receiver(from, signal, chain);
    }

    /// Lends a callable to other nodes. They call it at this node's dendrites; it is held until released.
    pub fn lend(&self, f: impl Fn(Vec<Value>, Map) -> Result<Value, NodeError> + Send + Sync + 'static) -> HandleRef {
        let id = self.inner.handles.register(Arc::new(f), None);
        HandleRef {
            id,
            owner: self.target(),
            traits: vec!["callable".into()],
        }
    }

    /// `lend`, for the node at the other end of `remote`: released when that synapse closes, too.
    pub fn lend_to(
        &self,
        remote: &Remote,
        f: impl Fn(Vec<Value>, Map) -> Result<Value, NodeError> + Send + Sync + 'static,
    ) -> HandleRef {
        let id = self.inner.handles.register(Arc::new(f), Some(remote.inner.id));
        HandleRef {
            id,
            owner: self.target(),
            traits: vec!["callable".into()],
        }
    }

    /// How many handles this node holds lent.
    pub fn lent(&self) -> usize {
        self.inner.handles.len()
    }

    /// Calls a handle another node lent, at its owner's dendrites (decision 44).
    pub async fn call(&self, handle: &HandleRef, args: Vec<Value>, kwargs: Map) -> Result<Value, NetError> {
        let owner = self.reach(&handle.owner, None).await?;
        owner.call_handle(handle.id, args, kwargs).await
    }

    /// The blocking form, for a node's own code on its worker threads.
    pub fn call_blocking(&self, handle: &HandleRef, args: Vec<Value>, kwargs: Map) -> Result<Value, NetError> {
        crate::synapse::block(self.call(handle, args, kwargs))
    }

    /// Tells a handle's owner it is no longer held.
    pub async fn release(&self, handle: &HandleRef) -> Result<(), NetError> {
        let owner = self.reach(&handle.owner, None).await?;
        owner.release(vec![handle.id]);
        Ok(())
    }

    pub(crate) fn answer(
        &self,
        peer: &Peer,
        handle: Option<u64>,
        method: &str,
        args: Vec<Value>,
        kwargs: Map,
    ) -> Result<Value, NodeError> {
        if let Some(handle) = handle {
            return self.inner.handles.call(handle, args, kwargs);
        }
        if !self.inner.interface.offers(method) {
            return Err(NodeError::new(
                NOT_IN_INTERFACE,
                format!("{} offers no method {method:?}", self.inner.config.name),
            ));
        }
        let call = Call { peer, node: self };
        self.inner.hillock.request(&call, method, args, kwargs)
    }

    fn link_chain(&self, signal: &str, functions: &[Value]) -> Result<Chain, NodeError> {
        let declared = Chain::declared(functions).map_err(|e| NodeError::new(BAD_FUNCTION, e.to_string()))?;
        let mut chain = Chain::new();
        if self.inner.interface.signal_kind(signal) == Some(SignalKind::State) {
            chain.push(Box::new(Throttle::new(self.inner.config.timing.coalesce)));
        }
        chain.extend(declared);
        Ok(chain)
    }

    pub(crate) fn add_link(
        &self,
        signal: String,
        to: Target,
        functions: &[Value],
        made_on: u64,
    ) -> Result<(), NodeError> {
        let chain = self.link_chain(&signal, functions)?;
        let mut links = self.inner.links.lock().unwrap();
        links.retain(|l| !(l.signal == signal && l.to == to));
        links.push(LinkOut {
            signal: signal.clone(),
            to: to.clone(),
            chain,
            made_on,
            seq: 0,
        });
        drop(links);
        let _ = self.inner.firing.send(Firing::Linked { signal, to });
        Ok(())
    }

    pub(crate) fn remove_link(&self, signal: &str, to: &Target) {
        self.inner
            .links
            .lock()
            .unwrap()
            .retain(|l| !(l.signal == signal && &l.to == to));
    }

    pub(crate) fn tune_link(&self, signal: &str, to: &Target, functions: &[Value]) -> Result<(), NodeError> {
        let chain = self.link_chain(signal, functions)?;
        let mut links = self.inner.links.lock().unwrap();
        match links.iter_mut().find(|l| l.signal == signal && &l.to == to) {
            Some(link) => link.chain = chain,
            None => return Err(NodeError::new(BAD_FUNCTION, format!("no link for {signal:?} to tune"))),
        }
        drop(links);
        let _ = self.inner.firing.send(Firing::Wake);
        Ok(())
    }

    pub(crate) fn drop_links_made_on(&self, synapse: u64) {
        self.inner.links.lock().unwrap().retain(|l| l.made_on != synapse);
    }

    pub(crate) fn receiver_due(&self, from: &str) -> Option<Instant> {
        self.inner
            .receivers
            .lock()
            .unwrap()
            .iter()
            .filter(|((f, _), _)| f == from)
            .filter_map(|(_, c)| c.due())
            .min()
    }

    pub(crate) fn receive_through(&self, stimulus: Stimulus, now: Instant) -> Vec<Stimulus> {
        let key = (stimulus.from.clone(), stimulus.signal.clone());
        match self.inner.receivers.lock().unwrap().get_mut(&key) {
            Some(chain) => chain.apply(stimulus, now),
            None => vec![stimulus],
        }
    }

    pub(crate) fn receive_tick(&self, from: &str, now: Instant) -> Vec<Stimulus> {
        let mut receivers = self.inner.receivers.lock().unwrap();
        let mut out = Vec::new();
        for ((f, _), chain) in receivers.iter_mut() {
            if f == from && chain.due().is_some_and(|due| due <= now) {
                out.extend(chain.tick(now));
            }
        }
        out
    }

    pub(crate) async fn integrate(&self, peer: &Peer, stimulus: Stimulus) {
        self.inner
            .latest
            .lock()
            .unwrap()
            .insert((stimulus.from.clone(), stimulus.signal.clone()), stimulus.data.clone());
        let (node, peer) = (self.clone(), peer.clone());
        let _ = tokio::task::spawn_blocking(move || {
            let call = Call {
                peer: &peer,
                node: &node,
            };
            node.inner.hillock.integrate(&call, &stimulus.signal, &stimulus.data);
        })
        .await;
    }

    /// Runs the links' functions on a fired signal, or on what came due, and gives back what to send where.
    fn route(&self, message: Option<Firing>, now: Instant) -> Vec<(Target, Op)> {
        let name = self.inner.config.name.clone();
        let mut links = self.inner.links.lock().unwrap();
        let mut out = Vec::new();
        let mut send = |link: &mut LinkOut, passed: Vec<Stimulus>| {
            for stimulus in passed {
                link.seq += 1;
                let op = Op::Fire {
                    signal: stimulus.signal,
                    from: name.clone(),
                    seq: link.seq,
                    data: stimulus.data,
                };
                out.push((link.to.clone(), op));
            }
        };
        match message {
            Some(Firing::Fire { signal, data }) => {
                if self.inner.interface.signal_kind(&signal) == Some(SignalKind::State) {
                    self.inner
                        .last_state
                        .lock()
                        .unwrap()
                        .insert(signal.clone(), data.clone());
                }
                for link in links.iter_mut().filter(|l| l.signal == signal) {
                    let passed = link.chain.apply(Stimulus::new(&signal, &name, 0, data.clone()), now);
                    send(link, passed);
                }
            }
            // A state signal's latest goes to a new link at once.
            Some(Firing::Linked { signal, to }) => {
                let latest = self.inner.last_state.lock().unwrap().get(&signal).cloned();
                if let (Some(data), Some(link)) = (latest, links.iter_mut().find(|l| l.signal == signal && l.to == to))
                {
                    let passed = link.chain.apply(Stimulus::new(&signal, &name, 0, data), now);
                    send(link, passed);
                }
            }
            Some(Firing::Wake) => {}
            None => {
                for link in links.iter_mut() {
                    if link.chain.due().is_some_and(|due| due <= now) {
                        let passed = link.chain.tick(now);
                        send(link, passed);
                    }
                }
            }
        }
        out
    }

    fn links_due(&self) -> Option<Instant> {
        self.inner
            .links
            .lock()
            .unwrap()
            .iter()
            .filter_map(|l| l.chain.due())
            .min()
    }
}

/// The node's signals: through each link's functions at this end, then along the axon.
async fn fire_loop(node: Node, mut inbox: mpsc::UnboundedReceiver<Firing>) {
    loop {
        let due = node.links_due();
        let message = tokio::select! {
            received = inbox.recv() => match received {
                Some(message) => Some(message),
                None => break,
            },
            _ = sleep_until(due) => None,
        };
        for (to, op) in node.route(message, Instant::now()) {
            if let Ok(remote) = node.reach(&to, None).await {
                remote.send(&op);
            }
        }
    }
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node").field("name", &self.inner.config.name).finish()
    }
}

#[allow(dead_code)]
fn _assert_send_sync() {
    fn check<T: Send + Sync>() {}
    check::<Node>();
    check::<Remote>();
    let _ = Duration::ZERO;
}
