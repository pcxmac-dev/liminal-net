//! The lifecycle (NETWORK.md §4.3): nothing runs unless used, and a node secures its data before its process ends.

use std::time::Duration;

/// A node's and its axon's times.
#[derive(Debug, Clone)]
pub struct Timing {
    /// How long a new node waits for its first synapse before it ends (host.py's START_SECONDS).
    pub start_wait: Duration,
    /// How long a node waits after its last synapse before it secures its data and ends (decision 27).
    pub grace: Duration,
    /// How long a connection has for the handshake (host.py's HELLO_SECONDS).
    pub hello_wait: Duration,
    /// How long an axon keeps a synapse with no request and no link (LIMINAL_QUIET_SECONDS, decision 13).
    pub quiet: Duration,
    /// How long an axon waits for a node it started, or one securing its data, to answer.
    pub reach_wait: Duration,
    /// State signals are coalesced to one per this interval (decision 20).
    pub coalesce: Duration,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing {
            start_wait: Duration::from_secs(30),
            grace: Duration::from_secs(10),
            hello_wait: Duration::from_secs(5),
            quiet: Duration::from_secs(120),
            reach_wait: Duration::from_secs(30),
            coalesce: Duration::from_millis(250),
        }
    }
}

impl Timing {
    /// The defaults, with LIMINAL_QUIET_SECONDS read as today's clients read it.
    pub fn from_env() -> Timing {
        let mut timing = Timing::default();
        let quiet = std::env::var("LIMINAL_QUIET_SECONDS")
            .ok()
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|q| q.is_finite() && *q >= 0.0);
        if let Some(quiet) = quiet {
            timing.quiet = Duration::from_secs_f64(quiet);
        }
        timing
    }
}

/// Where a node is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Running,
    /// Its data is being secured: new synapses are refused, to be retried once it has ended (decision 83).
    Securing,
    Ended,
}

/// Seconds, as the reasons say them: `10 s`, `0.2 s`.
pub(crate) fn seconds(d: Duration) -> String {
    let s = d.as_secs_f64();
    if s.fract() == 0.0 {
        format!("{s:.0} s")
    } else {
        format!("{s} s")
    }
}
