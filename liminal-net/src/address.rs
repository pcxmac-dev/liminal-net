//! Addresses (NETWORK.md §5.1): where a node's dendrites listen, named for the node and its scope.
//!
//! The digest is host.py's, so a scope names the same set of nodes in Python and in Rust. The prefix is the
//! network's own, `liminal-net-`, so host.py's pickle clients and the network's nodes never meet on one pipe
//! (decision 82).

use sha1::{Digest, Sha1};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// host.py's `_digest`: the first 10 hex characters of the SHA-1 of the scope's path, resolved as Python's
/// `Path.resolve()` resolves it, then lower-cased.
pub fn digest(scope: &Path) -> String {
    let text = resolve(scope).to_string_lossy().to_lowercase();
    let hash = Sha1::digest(text.as_bytes());
    crate::handshake::hex(&hash)[..10].to_string()
}

/// Where the node `name` of this scope listens: a pipe on Windows, a socket file in `ipc_dir` elsewhere.
/// `ipc_dir` never changes a pipe's name.
pub fn address(name: &str, scope: &Path, ipc_dir: &Path) -> String {
    #[cfg(windows)]
    {
        let _ = ipc_dir;
        format!(r"\\.\pipe\liminal-net-{name}-{}", digest(scope))
    }
    #[cfg(not(windows))]
    {
        ipc_dir
            .join(format!("net-{name}-{}.sock", digest(scope)))
            .to_string_lossy()
            .into_owned()
    }
}

/// Python's `Path.resolve()`, not strict:
/// - the path is made absolute and normalized;
/// - its longest existing part is resolved by the system (links, the real case);
/// - the rest is kept as given;
/// - no `\\?\` prefix.
pub fn resolve(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|dir| dir.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let normal = normalize(&absolute);
    let mut head = normal.clone();
    let mut tail: Vec<OsString> = Vec::new();
    loop {
        if let Ok(real) = std::fs::canonicalize(&head) {
            let mut out = plain(real);
            for part in tail.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (head.parent(), head.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_os_string());
                head = parent.to_path_buf();
            }
            _ => return normal,
        }
    }
}

/// Lexical normalization, as `os.path.normpath`: `.` dropped, `..` taken back, one kind of separator.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() && !out.has_root() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// A path without Windows' verbatim prefix, as Python's `realpath` gives it.
fn plain(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path
}
