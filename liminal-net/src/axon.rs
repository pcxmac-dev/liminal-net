//! The axon (NETWORK.md §2): a node's output. It reaches other nodes' dendrites, starting a resident node's process
//! when nobody answers, and holds them as `Remote`s.

use crate::address;
use crate::errors::NetError;
use crate::handshake::{self, Admission};
use crate::lifecycle::seconds;
use crate::message::Target;
use crate::node::Node;
use crate::synapse::Remote;
use crate::transport;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::time::Instant;

/// host.py's flags for a resident process: DETACHED_PROCESS, CREATE_NEW_PROCESS_GROUP and CREATE_NO_WINDOW, so no
/// window ever opens.
pub const START_FLAGS: u32 = 0x0800_0208;

/// Starts a node's process when nobody answers at its address.
pub trait Launcher: Send + Sync {
    fn launch(&self) -> io::Result<()>;
}

/// A launcher that runs a command: detached, windowless, its standard streams closed, never waited on.
#[derive(Debug, Clone)]
pub struct CommandLauncher {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
}

impl CommandLauncher {
    pub fn new(program: impl Into<PathBuf>) -> CommandLauncher {
        CommandLauncher {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> CommandLauncher {
        self.args.push(arg.into());
        self
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> CommandLauncher {
        self.env.push((key.into(), value.into()));
        self
    }

    /// The command, as it is run.
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(START_FLAGS);
        }
        command
    }
}

impl Launcher for CommandLauncher {
    fn launch(&self) -> io::Result<()> {
        self.command().spawn().map(drop)
    }
}

/// Any closure is a launcher too.
impl<F> Launcher for F
where
    F: Fn() -> io::Result<()> + Send + Sync,
{
    fn launch(&self) -> io::Result<()> {
        self()
    }
}

impl Node {
    /// Reaches `target`'s dendrites.
    /// - A synapse already open to it is shared.
    /// - When nobody answers, `launcher` starts the node, once, and the axon waits for it.
    /// - A node securing its data refuses for now: the axon waits for its end, then starts it again (decision 83).
    pub async fn reach(&self, target: &Target, launcher: Option<&dyn Launcher>) -> Result<Remote, NetError> {
        if let Some(remote) = self
            .inner
            .remotes
            .lock()
            .unwrap()
            .get(target)
            .filter(|r| r.is_open())
            .cloned()
        {
            return Ok(remote);
        }
        let config = &self.inner.config;
        let address = address::address(&target.node, Path::new(&target.scope), &config.ipc_dir);
        let deadline = Instant::now() + config.timing.reach_wait;
        let mut launched = false;
        loop {
            match transport::connect(&address, Duration::from_secs(2)).await {
                Ok(mut conn) => {
                    let admission = handshake::reach(
                        &mut conn,
                        &config.key,
                        &target.node,
                        &self.peer(),
                        config.timing.hello_wait,
                    )
                    .await?;
                    match admission {
                        Admission::Admitted { pid, interface, .. } => {
                            let id = self.inner.next_synapse.fetch_add(1, Ordering::SeqCst) + 1;
                            let remote = Remote::start(&self.inner, id, conn, target.clone(), pid, interface);
                            self.inner
                                .remotes
                                .lock()
                                .unwrap()
                                .insert(target.clone(), remote.clone());
                            return Ok(remote);
                        }
                        Admission::Refused { retry: true, why } => {
                            if Instant::now() >= deadline {
                                return Err(NetError::Timeout(format!("{}: {why}", target.node)));
                            }
                        }
                        Admission::Refused { why, .. } => return Err(NetError::Refused(why)),
                    }
                }
                Err(e) if transport::nobody(&e) => {
                    let Some(launcher) = launcher else {
                        return Err(NetError::Unreachable(format!("no node {} answers", target.node)));
                    };
                    if !launched {
                        launcher.launch()?;
                        launched = true;
                    }
                    if Instant::now() >= deadline {
                        return Err(NetError::Timeout(format!(
                            "{} did not answer within {}",
                            target.node,
                            seconds(config.timing.reach_wait)
                        )));
                    }
                }
                Err(e) => return Err(e.into()),
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// The blocking form, for a node's own code on its worker threads (never on an async task).
    pub fn reach_blocking(&self, target: &Target, launcher: Option<&dyn Launcher>) -> Result<Remote, NetError> {
        crate::synapse::block(self.reach(target, launcher))
    }
}
