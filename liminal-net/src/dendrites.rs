//! The dendrites (NETWORK.md §2): a node's input. One task per synapse: the handshake, the admission, then the
//! requests (concurrent, answered by id), the signals (in order, through the synapse's functions, into the hillock)
//! and the links.

use crate::errors::{NODE_FAILED, NodeError};
use crate::frame;
use crate::functions::Stimulus;
use crate::handshake;
use crate::lifecycle::Stage;
use crate::message::{Op, Peer};
use crate::node::Node;
use crate::transport::Conn;
use crate::{PROTO, Value};
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// Serves one synapse, from its handshake to its end.
pub(crate) async fn serve(node: Node, mut conn: Conn, id: u64) {
    let inner = node.inner.clone();
    let config = &inner.config;
    // A wrong key, a silent connection: dropped, never counted.
    let Ok(hello) = handshake::answer(&mut conn, &config.key, config.timing.hello_wait).await else {
        return;
    };
    let refusal = if *inner.stage.lock().unwrap() != Stage::Running {
        Some(Op::Refused {
            why: format!("{} is securing its data", config.name),
            retry: true,
        })
    } else if hello.proto > PROTO {
        Some(Op::Refused {
            why: format!("protocol {} is newer than this node's {PROTO}", hello.proto),
            retry: false,
        })
    } else if hello.node != config.name {
        Some(Op::Refused {
            why: format!("not this node: this is {}", config.name),
            retry: false,
        })
    } else {
        inner
            .hillock
            .admits(&hello.from)
            .err()
            .map(|why| Op::Refused { why, retry: false })
    };
    if let Some(refusal) = refusal {
        let _ = frame::write(&mut conn, &refusal.to_value()).await;
        return;
    }
    let admitted = Op::Admitted {
        node: config.name.clone(),
        pid: std::process::id(),
        interface: inner.interface_value.clone(),
    };
    if frame::write(&mut conn, &admitted.to_value()).await.is_err() {
        return;
    }
    inner.synapses.fetch_add(1, Ordering::SeqCst);
    inner.ever.store(true, Ordering::SeqCst);
    inner.changed.notify_one();
    let peer = hello.from;
    blocking(&node, {
        let (inner, peer) = (inner.clone(), peer.clone());
        move || inner.hillock.attach(&peer)
    })
    .await;

    let (mut reader, mut writer) = tokio::io::split(conn);
    let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
    tokio::spawn(async move {
        while let Some(value) = rx.recv().await {
            if frame::write(&mut writer, &value).await.is_err() {
                break;
            }
        }
    });
    let (signals, inbox) = mpsc::unbounded_channel::<Stimulus>();
    let integrating = tokio::spawn(integrate(node.clone(), peer.clone(), inbox));

    loop {
        let op = match frame::read(&mut reader).await {
            Ok(Some(value)) => match Op::from_value(value) {
                Ok(op) => op,
                Err(_) => break,
            },
            _ => break,
        };
        match op {
            Op::Request {
                id: request,
                handle,
                method,
                args,
                kwargs,
                ..
            } => {
                let (node, peer, tx) = (node.clone(), peer.clone(), tx.clone());
                node.inner.busy.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let inner = node.inner.clone();
                    let answering = node.clone();
                    let result =
                        tokio::task::spawn_blocking(move || answering.answer(&peer, handle, &method, args, kwargs))
                            .await;
                    let reply = match result {
                        Ok(Ok(value)) => Op::Answer { id: request, value },
                        Ok(Err(e)) => Op::Error {
                            id: request,
                            error: e.to_record(),
                        },
                        Err(failed) => Op::Error {
                            id: request,
                            error: NodeError::new(NODE_FAILED, format!("the node's code failed: {failed}")).to_record(),
                        },
                    };
                    let _ = tx.send(reply.to_value());
                    inner.busy.fetch_sub(1, Ordering::SeqCst);
                    inner.changed.notify_one();
                });
            }
            Op::Fire {
                signal,
                from,
                seq,
                data,
            } => {
                let _ = signals.send(Stimulus {
                    signal,
                    from,
                    seq,
                    data,
                });
            }
            Op::Link {
                id: request,
                signal,
                to,
                functions,
            } => {
                let reply = node.add_link(signal, to, &functions, id);
                let _ = tx.send(answer(request, reply).to_value());
            }
            Op::Unlink {
                id: request,
                signal,
                to,
            } => {
                node.remove_link(&signal, &to);
                let _ = tx.send(
                    Op::Answer {
                        id: request,
                        value: Value::Null,
                    }
                    .to_value(),
                );
            }
            Op::Tune {
                id: request,
                signal,
                to,
                functions,
            } => {
                let reply = node.tune_link(&signal, &to, &functions);
                let _ = tx.send(answer(request, reply).to_value());
            }
            Op::Release { handles } => inner.handles.release(&handles),
            Op::Detach => break,
            _ => break,
        }
    }

    // What arrived before the end is integrated before the synapse is let go.
    drop(signals);
    let _ = integrating.await;
    inner.synapses.fetch_sub(1, Ordering::SeqCst);
    node.drop_links_made_on(id);
    inner.handles.release_synapse(id);
    blocking(&node, {
        let inner = inner.clone();
        move || inner.hillock.detach(&peer)
    })
    .await;
    inner.changed.notify_one();
}

fn answer(id: u64, reply: Result<(), NodeError>) -> Op {
    match reply {
        Ok(()) => Op::Answer { id, value: Value::Null },
        Err(e) => Op::Error {
            id,
            error: e.to_record(),
        },
    }
}

/// The node's code, on a worker thread.
async fn blocking(_node: &Node, f: impl FnOnce() + Send + 'static) {
    let _ = tokio::task::spawn_blocking(f).await;
}

/// The signals of one synapse, in the order they came: through the receiver's functions, then into the hillock.
async fn integrate(node: Node, peer: Peer, mut inbox: mpsc::UnboundedReceiver<Stimulus>) {
    loop {
        let due = node.receiver_due(&peer.name);
        let stimulus = tokio::select! {
            received = inbox.recv() => match received {
                Some(stimulus) => Some(stimulus),
                None => break,
            },
            _ = sleep_until(due) => None,
        };
        let now = Instant::now();
        let passed = match stimulus {
            Some(stimulus) => node.receive_through(stimulus, now),
            None => node.receive_tick(&peer.name, now),
        };
        for stimulus in passed {
            node.integrate(&peer, stimulus).await;
        }
    }
}

pub(crate) async fn sleep_until(due: Option<Instant>) {
    match due {
        Some(due) => tokio::time::sleep_until(due).await,
        None => std::future::pending::<()>().await,
    }
}
