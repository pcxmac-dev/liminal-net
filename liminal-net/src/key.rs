//! The user's key (NETWORK.md §5.1). It is host.py's own file, so Python's hosts and the network share it.

use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

/// The key, as its file holds it: 64 hex characters, used as they are for the HMAC.
#[derive(Clone)]
pub struct Key(Vec<u8>);

impl Key {
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Key {
        Key(bytes.into())
    }

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(…)")
    }
}

/// The user's home, as Python's `Path.home()` finds it.
pub fn home() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(profile) = std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()) {
            return PathBuf::from(profile);
        }
        let mut home = std::env::var_os("HOMEDRIVE").unwrap_or_default();
        home.push(std::env::var_os("HOMEPATH").unwrap_or_default());
        PathBuf::from(home)
    }
    #[cfg(not(windows))]
    {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
    }
}

/// `LIMINAL_IPC_DIR`, else `~/.liminal/ipc`, as host.py's `ipc_dir()`.
pub fn ipc_dir() -> PathBuf {
    match std::env::var_os("LIMINAL_IPC_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home().join(".liminal").join("ipc"),
    }
}

/// Reads the key under `dir`, making it once when there is none.
/// - Any content of 32 bytes or more is accepted, as host.py accepts it.
/// - A new key is 32 random bytes in hex. It is published whole and with create-new, so two first runs can't end up
///   with two keys (decision 83). A key shorter than 32 bytes is replaced, as host.py replaces it.
pub fn load_or_create(dir: &Path) -> io::Result<Key> {
    let path = dir.join("key");
    match read_key(&path)? {
        Found::Valid(key) => return Ok(key),
        Found::Missing => {}
        // host.py replaces a short key. It is taken away, then made as if there were none, unless another process
        // made a valid one meanwhile: a key that is there is never overwritten.
        Found::Short => {
            if let Found::Valid(key) = read_key(&path)? {
                return Ok(key);
            }
            match fs::remove_file(&path) {
                Err(e) if e.kind() != ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
    }
    fs::create_dir_all(dir)?;
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| io::Error::other(e.to_string()))?;
    let text = crate::handshake::hex(&raw);
    // A name of its own for each attempt: two threads of one process may both be making the key.
    let tmp = dir.join(format!(
        "key.{}.{}.new",
        std::process::id(),
        &crate::handshake::nonce()[..8]
    ));
    fs::write(&tmp, &text)?;
    owner_only(&tmp);
    // Published whole, and only if no key is there yet: the first one made wins, everywhere.
    let placed = match fs::hard_link(&tmp, &path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(_) => create_new(&path, text.as_bytes()),
    };
    let _ = fs::remove_file(&tmp);
    placed?;
    match read_key(&path)? {
        Found::Valid(key) => Ok(key),
        _ => Err(io::Error::new(
            ErrorKind::InvalidData,
            "the key file holds fewer than 32 bytes",
        )),
    }
}

enum Found {
    Valid(Key),
    Short,
    Missing,
}

fn create_new(path: &Path, data: &[u8]) -> io::Result<()> {
    match fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file.write_all(data),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

fn read_key(path: &Path) -> io::Result<Found> {
    match fs::read(path) {
        Ok(bytes) => {
            let text = strip(&bytes);
            Ok(if text.len() >= 32 {
                Found::Valid(Key(text.to_vec()))
            } else {
                Found::Short
            })
        }
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Found::Missing),
        Err(e) => Err(e),
    }
}

/// Python's `bytes.strip()`: ASCII whitespace, the vertical tab included.
fn strip(bytes: &[u8]) -> &[u8] {
    let space = |c: &u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let start = bytes.iter().position(|c| !space(c)).unwrap_or(bytes.len());
    let end = bytes.iter().rposition(|c| !space(c)).map_or(start, |i| i + 1);
    &bytes[start..end]
}

#[cfg(unix)]
fn owner_only(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn owner_only(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_matches_python() {
        assert_eq!(strip(b" \x0b\tab cd\r\n\x0c"), b"ab cd");
        assert_eq!(strip(b"   "), b"");
        assert_eq!(strip(b""), b"");
    }
}
