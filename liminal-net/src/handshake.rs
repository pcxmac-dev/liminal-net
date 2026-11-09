//! The handshake (NETWORK.md §5.3): both ends prove they hold the user's key without sending it, and the reaching
//! side says who it is. The reached node's dendrites speak first.

use crate::errors::NetError;
use crate::frame;
use crate::key::Key;
use crate::message::{Op, Peer};
use crate::{PROTO, Value};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};

/// The label the reaching side (the axon) proves with.
pub const AXON_LABEL: &str = "liminal/1|axon|";
/// The label the reached side (the dendrites) proves with.
pub const DENDRITES_LABEL: &str = "liminal/1|dendrites|";

/// Bytes as lower-case hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Hex back to bytes; `None` when it is not hex.
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| text.get(i..i + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok()))
        .collect()
}

/// 32 random bytes, in hex.
pub fn nonce() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the system's random source answers");
    hex(&bytes)
}

fn mac(key: &Key, label: &str, nonce: &str) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.bytes()).expect("HMAC takes a key of any length");
    mac.update(label.as_bytes());
    mac.update(nonce.as_bytes());
    mac
}

/// A proof: the HMAC-SHA256, with the key, of the prover's label and the other side's nonce, in hex.
pub fn proof(key: &Key, label: &str, nonce: &str) -> String {
    hex(&mac(key, label, nonce).finalize().into_bytes())
}

/// Checks a proof, in constant time.
pub fn verify(key: &Key, label: &str, nonce: &str, proof_hex: &str) -> bool {
    match unhex(proof_hex) {
        Some(given) => mac(key, label, nonce).verify_slice(&given).is_ok(),
        None => false,
    }
}

async fn next<C: AsyncRead + Unpin>(conn: &mut C, wait: Duration) -> Result<Op, NetError> {
    let read = tokio::time::timeout(wait, frame::read(conn))
        .await
        .map_err(|_| NetError::Timeout("no answer within the handshake's wait".into()))??;
    Op::from_value(read.ok_or(NetError::Closed)?)
}

async fn send<C: AsyncWrite + Unpin>(conn: &mut C, op: &Op) -> Result<(), NetError> {
    frame::write(conn, &op.to_value()).await
}

/// What the reached side learns from a handshake that passed: which node is wanted, and who is reaching.
#[derive(Debug, Clone)]
pub struct Hello {
    pub node: String,
    pub from: Peer,
    pub proto: u64,
}

/// The reached side: challenge, check the proof, prove itself, and take the hello. The caller then admits or
/// refuses. A wrong proof ends here, and nothing about the key is said.
pub async fn answer<C: AsyncRead + AsyncWrite + Unpin>(
    conn: &mut C,
    key: &Key,
    wait: Duration,
) -> Result<Hello, NetError> {
    let n1 = nonce();
    send(
        conn,
        &Op::Challenge {
            proto: PROTO,
            nonce: n1.clone(),
        },
    )
    .await?;
    let Op::Proof { mac, nonce: Some(n2) } = next(conn, wait).await? else {
        return Err(NetError::Handshake(
            "the first answer was not a proof with a nonce".into(),
        ));
    };
    if !verify(key, AXON_LABEL, &n1, &mac) {
        return Err(NetError::Handshake("a wrong proof".into()));
    }
    send(
        conn,
        &Op::Proof {
            mac: proof(key, DENDRITES_LABEL, &n2),
            nonce: None,
        },
    )
    .await?;
    let Op::Hello { node, from, proto } = next(conn, wait).await? else {
        return Err(NetError::Handshake("no hello after the proofs".into()));
    };
    Ok(Hello { node, from, proto })
}

/// How the reached node answered a hello.
#[derive(Debug, Clone)]
pub enum Admission {
    Admitted {
        node: String,
        pid: u32,
        interface: Value,
    },
    /// `retry`: the node is securing its data, and will be gone shortly.
    Refused {
        why: String,
        retry: bool,
    },
}

/// The reaching side: prove, check the other side's proof, then say hello, and take the admission.
pub async fn reach<C: AsyncRead + AsyncWrite + Unpin>(
    conn: &mut C,
    key: &Key,
    node: &str,
    from: &Peer,
    wait: Duration,
) -> Result<Admission, NetError> {
    let Op::Challenge { proto, nonce: n1 } = next(conn, wait).await? else {
        return Err(NetError::Handshake("the node did not challenge".into()));
    };
    if proto > PROTO {
        return Err(NetError::Refused(format!(
            "the node speaks protocol {proto}, newer than this one's {PROTO}"
        )));
    }
    let n2 = nonce();
    send(
        conn,
        &Op::Proof {
            mac: proof(key, AXON_LABEL, &n1),
            nonce: Some(n2.clone()),
        },
    )
    .await?;
    let Op::Proof { mac, .. } = next(conn, wait).await? else {
        return Err(NetError::Handshake("the node answered no proof".into()));
    };
    if !verify(key, DENDRITES_LABEL, &n2, &mac) {
        return Err(NetError::Handshake("the node's proof is wrong".into()));
    }
    send(
        conn,
        &Op::Hello {
            node: node.to_string(),
            from: from.clone(),
            proto: PROTO,
        },
    )
    .await?;
    match next(conn, wait).await? {
        Op::Admitted { node, pid, interface } => Ok(Admission::Admitted { node, pid, interface }),
        Op::Refused { why, retry } => Ok(Admission::Refused { why, retry }),
        other => Err(NetError::Protocol(format!(
            "expected admitted or refused, not {}",
            other.name()
        ))),
    }
}
