//! The network end to end, in one process: nodes served in tasks, over a scratch key folder, under names of their
//! own. No process is started, and nothing leaves this machine.

#![allow(clippy::result_large_err)]

mod common;

use common::{Echo, Recorder, TempDir, config, eventually, quick, wait_for_it};
use liminal_net::handshake::{AXON_LABEL, nonce, proof};
use liminal_net::lifecycle::Stage;
use liminal_net::message::Op;
use liminal_net::{Chain, CommandLauncher, Map, NetError, Node, START_FLAGS, Stimulus, Value, frame, transport};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

fn serve(node: &Node) -> JoinHandle<String> {
    let node = node.clone();
    tokio::spawn(async move { node.serve().await })
}

fn echo(dir: &TempDir) -> (Node, Arc<Echo>) {
    let hillock = Arc::new(Echo::default());
    (Node::new(config(dir, "echo", "plugin"), hillock.clone()), hillock)
}

fn app(dir: &TempDir) -> (Node, Recorder) {
    let recorder = Recorder::default();
    (
        Node::new(config(dir, "app", "app"), Arc::new(recorder.clone())),
        recorder,
    )
}

const SECOND: Duration = Duration::from_secs(1);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requests_are_answered() {
    let dir = TempDir::new();
    let (node, _) = echo(&dir);
    let _served = serve(&node);
    let (caller, _) = app(&dir);
    let remote = caller.reach(&node.target(), Some(&wait_for_it)).await.unwrap();
    let value = json!({"z": 1, "a": [1, 2.5, "three", null], "m": {"y": true}});
    let answer = remote.request("echo", vec![value.clone()], Map::new()).await.unwrap();
    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        serde_json::to_string(&value).unwrap()
    );
    assert_eq!(remote.pid(), std::process::id());
    assert_eq!(remote.interface()["trait"], "test.Echo");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requests_run_concurrently_and_are_answered_by_id() {
    let dir = TempDir::new();
    let (node, _) = echo(&dir);
    let _served = serve(&node);
    let (caller, _) = app(&dir);
    let remote = caller.reach(&node.target(), Some(&wait_for_it)).await.unwrap();
    let slow = {
        let remote = remote.clone();
        tokio::spawn(async move {
            remote.request("slow", vec![json!(400)], Map::new()).await.unwrap();
            Instant::now()
        })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    remote.request("echo", vec![json!(1)], Map::new()).await.unwrap();
    let quick_done = Instant::now();
    let slow_done = slow.await.unwrap();
    assert!(
        quick_done < slow_done,
        "the quick request did not wait for the slow one"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refusal_crosses_with_its_kind_words_accept_and_status() {
    let dir = TempDir::new();
    let (node, _) = echo(&dir);
    let _served = serve(&node);
    let (caller, _) = app(&dir);
    let remote = caller.reach(&node.target(), Some(&wait_for_it)).await.unwrap();
    let Err(NetError::Remote(error)) = remote.request("fail", vec![], Map::new()).await else {
        panic!("the refusal did not cross");
    };
    assert_eq!(
        (error.kind.as_str(), error.message.as_str()),
        ("CommandError", "the command failed")
    );
    assert_eq!((error.accept, error.status), (Some(Value::Bool(true)), Some(400)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_the_interface_is_served_and_a_panic_is_only_an_error() {
    let dir = TempDir::new();
    let (node, hillock) = echo(&dir);
    let _served = serve(&node);
    let (caller, _) = app(&dir);
    let remote = caller.reach(&node.target(), Some(&wait_for_it)).await.unwrap();
    let Err(NetError::Remote(error)) = remote.request("shutdown", vec![], Map::new()).await else {
        panic!("a method outside the interface was answered");
    };
    assert_eq!(error.kind, "NotInInterface");
    assert_eq!(hillock.calls.load(Ordering::SeqCst), 0, "the node's code never saw it");
    let Err(NetError::Remote(error)) = remote.request("panic", vec![], Map::new()).await else {
        panic!("a panic was not answered");
    };
    assert_eq!(error.kind, "NodeFailed");
    assert_eq!(
        remote.request("echo", vec![json!(2)], Map::new()).await.unwrap(),
        json!(2),
        "the synapse stayed up"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wrong_key_is_refused_and_never_counted() {
    let dir = TempDir::new();
    let (node, _) = echo(&dir);
    let _served = serve(&node);
    let mut stranger = config(&dir, "stranger", "app");
    stranger.key = liminal_net::Key::from_bytes("f".repeat(64));
    let stranger = Node::new(stranger, Arc::new(Recorder::default()));
    let result = stranger.reach(&node.target(), Some(&wait_for_it)).await;
    assert!(
        matches!(
            result,
            Err(NetError::Handshake(_)) | Err(NetError::Closed) | Err(NetError::Io(_))
        ),
        "{result:?}"
    );
    assert_eq!(node.synapses(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn admits_refuses_with_its_words() {
    let dir = TempDir::new();
    let hillock = Arc::new(Echo {
        only_kind: Some("app".into()),
        ..Default::default()
    });
    let node = Node::new(config(&dir, "echo", "plugin"), hillock);
    let _served = serve(&node);
    let other = Node::new(config(&dir, "other-plugin", "plugin"), Arc::new(Recorder::default()));
    match other.reach(&node.target(), Some(&wait_for_it)).await {
        Err(NetError::Refused(why)) => assert_eq!(why, "only app nodes"),
        other => panic!("not refused: {:?}", other.map(|_| ())),
    }
    let (caller, _) = app(&dir);
    assert!(caller.reach(&node.target(), None).await.is_ok(), "an app is admitted");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_newer_protocol_is_refused_with_words() {
    let dir = TempDir::new();
    let (node, _) = echo(&dir);
    let _served = serve(&node);
    let address = node.config().address();
    let key = node.config().key.clone();
    let mut conn = loop {
        match transport::connect(&address, SECOND).await {
            Ok(conn) => break conn,
            Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    };
    let Some(challenge) = frame::read(&mut conn).await.unwrap() else {
        panic!("no challenge")
    };
    let n1 = challenge["nonce"].as_str().unwrap().to_string();
    let proof_op = Op::Proof {
        mac: proof(&key, AXON_LABEL, &n1),
        nonce: Some(nonce()),
    };
    frame::write(&mut conn, &proof_op.to_value()).await.unwrap();
    frame::read(&mut conn).await.unwrap().expect("the node's proof");
    let hello = json!({"op": "hello", "node": "echo", "from": {"name": "future", "kind": "app", "scope": "s", "pid": 1}, "proto": 2});
    frame::write(&mut conn, &hello).await.unwrap();
    let refused = frame::read(&mut conn).await.unwrap().unwrap();
    assert_eq!(refused["op"], "refused");
    assert_eq!(refused["why"], "protocol 2 is newer than this node's 1");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_node_per_name() {
    let dir = TempDir::new();
    let (node, _) = echo(&dir);
    let _served = serve(&node);
    let (caller, _) = app(&dir);
    caller.reach(&node.target(), Some(&wait_for_it)).await.unwrap();
    let (twin, twin_hillock) = echo(&dir);
    let reason = twin.serve().await;
    assert!(reason.starts_with("another node answers for echo"), "{reason}");
    assert_eq!(
        twin_hillock.secured.load(Ordering::SeqCst),
        0,
        "the twin never ran, so it secured nothing"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silent_connection_is_not_a_synapse() {
    let dir = TempDir::new();
    let mut c = config(&dir, "echo", "plugin");
    c.timing.start_wait = Duration::from_millis(600);
    c.timing.hello_wait = Duration::from_millis(200);
    let node = Node::new(c, Arc::new(Echo::default()));
    let served = serve(&node);
    let address = node.config().address();
    let _silent = loop {
        match transport::connect(&address, SECOND).await {
            Ok(conn) => break conn,
            Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
        }
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(node.synapses(), 0);
    assert_eq!(served.await.unwrap(), "nobody came within 0.6 s");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_node_secures_its_data_and_ends_after_its_last_synapse() {
    let dir = TempDir::new();
    let (node, hillock) = echo(&dir);
    let served = serve(&node);
    let (caller, _) = app(&dir);
    let remote = caller.reach(&node.target(), Some(&wait_for_it)).await.unwrap();
    remote.request("echo", vec![json!(1)], Map::new()).await.unwrap();
    assert!(eventually(SECOND, || node.synapses() == 1).await);
    remote.detach();
    remote.detach();
    let reason = tokio::time::timeout(5 * SECOND, served)
        .await
        .expect("the node ended")
        .unwrap();
    assert_eq!(reason, "no synapse for 0.2 s");
    assert_eq!(
        hillock.secured.load(Ordering::SeqCst),
        1,
        "secured once, before it ended"
    );
    assert_eq!(node.stage(), Stage::Ended);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reaching_again_within_the_grace_keeps_the_node() {
    let dir = TempDir::new();
    let (node, hillock) = echo(&dir);
    let served = serve(&node);
    let (caller, _) = app(&dir);
    let first = caller.reach(&node.target(), Some(&wait_for_it)).await.unwrap();
    first.detach();
    first.closed().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let second = caller
        .reach(&node.target(), None)
        .await
        .expect("still there within the grace");
    assert_eq!(
        second.request("echo", vec![json!("again")], Map::new()).await.unwrap(),
        json!("again")
    );
    assert!(!served.is_finished());
    assert_eq!(hillock.secured.load(Ordering::SeqCst), 0);
    second.detach();
    tokio::time::timeout(5 * SECOND, served)
        .await
        .expect("then it ended")
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_synapse_is_counted() {
    let dir = TempDir::new();
    let (node, _) = echo(&dir);
    let _served = serve(&node);
    let one = Node::new(config(&dir, "one", "app"), Arc::new(Recorder::default()));
    let two = Node::new(config(&dir, "two", "app"), Arc::new(Recorder::default()));
    let a = one.reach(&node.target(), Some(&wait_for_it)).await.unwrap();
    let b = two.reach(&node.target(), Some(&wait_for_it)).await.unwrap();
    assert!(eventually(SECOND, || node.synapses() == 2).await, "{}", node.synapses());
    a.detach();
    assert!(eventually(SECOND, || node.synapses() == 1).await);
    b.detach();
    assert!(eventually(SECOND, || node.synapses() == 0).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_node_nobody_reaches_ends_after_the_start_wait() {
    let dir = TempDir::new();
    let mut c = config(&dir, "echo", "plugin");
    c.timing.start_wait = Duration::from_millis(300);
    let node = Node::new(c, Arc::new(Echo::default()));
    assert_eq!(node.serve().await, "nobody came within 0.3 s");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_launcher_starts_a_node_once_and_the_axon_waits_for_it() {
    let dir = TempDir::new();
    let (node, _) = echo(&dir);
    let launches = Arc::new(AtomicUsize::new(0));
    let launcher = {
        let (node, launches) = (node.clone(), launches.clone());
        move || {
            launches.fetch_add(1, Ordering::SeqCst);
            let node = node.clone();
            tokio::spawn(async move { node.serve().await });
            Ok(())
        }
    };
    let (caller, _) = app(&dir);
    let remote = caller.reach(&node.target(), Some(&launcher)).await.unwrap();
    assert_eq!(
        remote.request("echo", vec![json!("up")], Map::new()).await.unwrap(),
        json!("up")
    );
    let again = caller.reach(&node.target(), Some(&launcher)).await.unwrap();
    assert_eq!(again.pid(), remote.pid());
    assert_eq!(launches.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_node_securing_its_data_is_waited_for_then_started_again() {
    let dir = TempDir::new();
    let hillock = Arc::new(Echo {
        secure_for: Duration::from_millis(600),
        ..Default::default()
    });
    let node = Node::new(config(&dir, "echo", "plugin"), hillock.clone());
    let ended = Arc::new(AtomicBool::new(false));
    {
        let (node, ended) = (node.clone(), ended.clone());
        tokio::spawn(async move {
            node.serve().await;
            ended.store(true, Ordering::SeqCst);
        });
    }
    let (caller, _) = app(&dir);
    let first = caller.reach(&node.target(), Some(&wait_for_it)).await.unwrap();
    first.detach();
    assert!(
        eventually(3 * SECOND, || node.stage() == Stage::Securing).await,
        "it began securing"
    );
    let launched_after_end = Arc::new(AtomicBool::new(false));
    let launches = Arc::new(AtomicUsize::new(0));
    let (successor, _) = echo(&dir);
    let launcher = {
        let (successor, ended, after, launches) = (
            successor.clone(),
            ended.clone(),
            launched_after_end.clone(),
            launches.clone(),
        );
        move || {
            launches.fetch_add(1, Ordering::SeqCst);
            after.store(ended.load(Ordering::SeqCst), Ordering::SeqCst);
            let successor = successor.clone();
            tokio::spawn(async move { successor.serve().await });
            Ok(())
        }
    };
    let second = caller.reach(&node.target(), Some(&launcher)).await.unwrap();
    assert_eq!(
        second.request("echo", vec![json!("new")], Map::new()).await.unwrap(),
        json!("new")
    );
    assert_eq!(hillock.secured.load(Ordering::SeqCst), 1);
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    assert!(
        launched_after_end.load(Ordering::SeqCst),
        "no duplicate started while the old one secured its data"
    );
}

#[test]
fn the_start_command_is_detached_windowless_and_silent() {
    assert_eq!(START_FLAGS, 0x0800_0208);
    let launcher = CommandLauncher::new("pythonw.exe")
        .arg("-m")
        .arg("liminal_charts")
        .arg("host")
        .arg("fetcher")
        .env("LIMINAL_DB", r"C:\data\liminal.db");
    let command = launcher.command();
    assert_eq!(command.get_program(), "pythonw.exe");
    let args: Vec<_> = command.get_args().collect();
    assert_eq!(args, ["-m", "liminal_charts", "host", "fetcher"]);
    let env: Vec<_> = command.get_envs().collect();
    assert_eq!(
        env,
        [(
            std::ffi::OsStr::new("LIMINAL_DB"),
            Some(std::ffi::OsStr::new(r"C:\data\liminal.db"))
        )]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_quiet_synapse_closes() {
    let dir = TempDir::new();
    let (node, _) = echo(&dir);
    let _served = serve(&node);
    let mut c = config(&dir, "app", "app");
    c.timing.quiet = Duration::from_millis(200);
    let caller = Node::new(c, Arc::new(Recorder::default()));
    let remote = caller.reach(&node.target(), Some(&wait_for_it)).await.unwrap();
    remote.request("echo", vec![json!(1)], Map::new()).await.unwrap();
    assert!(
        eventually(2 * SECOND, || !remote.is_open()).await,
        "closed after the quiet time"
    );
    assert!(eventually(SECOND, || node.synapses() == 0).await);
}

// Signals

async fn linked(dir: &TempDir) -> (Node, Node, Recorder, liminal_net::Remote, JoinHandle<String>) {
    let (node, _) = echo(dir);
    let served = serve(&node);
    let (receiver, recorder) = app(dir);
    let _receiving = serve(&receiver);
    let remote = receiver.reach(&node.target(), Some(&wait_for_it)).await.unwrap();
    (node, receiver, recorder, remote, served)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stream_signals_arrive_every_one_in_order() {
    let dir = TempDir::new();
    let (node, _receiver, recorder, remote, _) = linked(&dir).await;
    remote.link("lines", vec![]).await.unwrap();
    for i in 0..20 {
        node.fire("lines", json!(i));
    }
    let got = recorder.wait(20, 3 * SECOND).await;
    let values: Vec<_> = got.iter().map(|(_, d)| d.as_i64().unwrap()).collect();
    assert_eq!(values, (0..20).collect::<Vec<_>>());
    assert!(got.iter().all(|(signal, _)| signal == "lines"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn state_signals_are_coalesced_to_the_latest() {
    let dir = TempDir::new();
    let (node, receiver, recorder, remote, _) = linked(&dir).await;
    remote.link("status", vec![]).await.unwrap();
    for i in 0..10 {
        node.fire("status", json!({"n": i}));
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        recorder.data(),
        vec![json!({"n": 0}), json!({"n": 9})],
        "the first at once, then the latest"
    );
    assert_eq!(receiver.latest("echo", "status"), Some(json!({"n": 9})));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_link_gets_the_latest_state_at_once() {
    let dir = TempDir::new();
    let (node, _receiver, recorder, remote, _) = linked(&dir).await;
    node.fire("status", json!({"state": "ready"}));
    tokio::time::sleep(Duration::from_millis(50)).await;
    remote.link("status", vec![]).await.unwrap();
    assert_eq!(
        recorder.wait(1, 2 * SECOND).await,
        vec![("status".to_string(), json!({"state": "ready"}))]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sender_functions_keep_unwanted_signals_from_being_sent() {
    let dir = TempDir::new();
    let (node, _receiver, recorder, remote, _) = linked(&dir).await;
    remote
        .link("lines", vec![json!({"fn": "only", "where": {"keep": true}})])
        .await
        .unwrap();
    for (n, keep) in [(1, true), (2, false), (3, true)] {
        node.fire("lines", json!({"n": n, "keep": keep}));
    }
    let got = recorder.wait(2, 2 * SECOND).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(got.len(), 2);
    assert_eq!(
        recorder.data(),
        vec![json!({"n": 1, "keep": true}), json!({"n": 3, "keep": true})]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn receiver_functions_run_at_the_receivers_end() {
    let dir = TempDir::new();
    let (node, _receiver, recorder, remote, _) = linked(&dir).await;
    let evens = Chain::new().then(|s: Stimulus| if s.seq.is_multiple_of(2) { vec![s] } else { vec![] });
    remote.link_with("lines", vec![], evens).await.unwrap();
    for i in 1..=6 {
        node.fire("lines", json!(i));
    }
    recorder.wait(3, 2 * SECOND).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        recorder.data(),
        vec![json!(2), json!(4), json!(6)],
        "seq counts what was sent, from 1"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tune_replaces_a_live_links_functions_and_unlink_ends_it() {
    let dir = TempDir::new();
    let (node, _receiver, recorder, remote, _) = linked(&dir).await;
    remote
        .link("lines", vec![json!({"fn": "only", "where": {"keep": true}})])
        .await
        .unwrap();
    node.fire("lines", json!({"keep": false, "n": 1}));
    remote.tune("lines", vec![]).await.unwrap();
    node.fire("lines", json!({"keep": false, "n": 2}));
    recorder.wait(1, 2 * SECOND).await;
    remote.unlink("lines").await.unwrap();
    node.fire("lines", json!({"keep": false, "n": 3}));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(recorder.data(), vec![json!({"keep": false, "n": 2})]);
}

// Handles

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lent_callable_is_answered_at_its_owners_dendrites() {
    let dir = TempDir::new();
    let (node, _) = echo(&dir);
    let _served = serve(&node);
    let (owner, _) = app(&dir);
    let _owning = serve(&owner);
    let calls = Arc::new(AtomicUsize::new(0));
    let handle = {
        let calls = calls.clone();
        owner.lend(move |args, _kwargs| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!(format!("got {}", args[0].as_str().unwrap_or("?"))))
        })
    };
    let remote = owner.reach(&node.target(), Some(&wait_for_it)).await.unwrap();
    let answer = remote
        .request("callback", vec![handle.to_value()], Map::new())
        .await
        .unwrap();
    assert_eq!(answer, json!("got from echo"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // Released by the node holding it: the owner no longer holds it lent.
    assert_eq!(owner.lent(), 1);
    node.release(&handle).await.unwrap();
    assert!(eventually(SECOND, || owner.lent() == 0).await);
    let Err(NetError::Remote(error)) = remote.request("callback", vec![handle.to_value()], Map::new()).await else {
        panic!("a released handle answered");
    };
    assert_eq!(error.kind, "NoSuchHandle");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn handles_lent_over_a_synapse_go_when_it_closes() {
    let dir = TempDir::new();
    let (node, _) = echo(&dir);
    let _served = serve(&node);
    let (owner, _) = app(&dir);
    let remote = owner.reach(&node.target(), Some(&wait_for_it)).await.unwrap();
    owner.lend_to(&remote, |_, _| Ok(Value::Null));
    owner.lend(|_, _| Ok(Value::Null));
    assert_eq!(owner.lent(), 2);
    remote.detach();
    remote.closed().await;
    assert!(
        eventually(SECOND, || owner.lent() == 1).await,
        "only the one lent over it went"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stop_ends_a_node_with_its_reason() {
    let dir = TempDir::new();
    let mut c = config(&dir, "echo", "plugin");
    c.timing = quick();
    let node = Node::new(c, Arc::new(Echo::default()));
    let served = serve(&node);
    tokio::time::sleep(Duration::from_millis(50)).await;
    node.stop("asked to");
    node.stop("asked again");
    assert_eq!(
        tokio::time::timeout(5 * SECOND, served).await.unwrap().unwrap(),
        "asked to"
    );
}
