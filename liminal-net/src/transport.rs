//! The transport (NETWORK.md §5.1): named pipes on Windows, Unix sockets elsewhere. This machine only (decision 15).

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// One connection, at either end.
pub enum Conn {
    #[cfg(windows)]
    Server(tokio::net::windows::named_pipe::NamedPipeServer),
    #[cfg(windows)]
    Client(tokio::net::windows::named_pipe::NamedPipeClient),
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
}

impl AsyncRead for Conn {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            #[cfg(windows)]
            Conn::Server(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(windows)]
            Conn::Client(c) => Pin::new(c).poll_read(cx, buf),
            #[cfg(unix)]
            Conn::Unix(u) => Pin::new(u).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Conn {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            #[cfg(windows)]
            Conn::Server(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(windows)]
            Conn::Client(c) => Pin::new(c).poll_write(cx, buf),
            #[cfg(unix)]
            Conn::Unix(u) => Pin::new(u).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            #[cfg(windows)]
            Conn::Server(s) => Pin::new(s).poll_flush(cx),
            #[cfg(windows)]
            Conn::Client(c) => Pin::new(c).poll_flush(cx),
            #[cfg(unix)]
            Conn::Unix(u) => Pin::new(u).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            #[cfg(windows)]
            Conn::Server(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(windows)]
            Conn::Client(c) => Pin::new(c).poll_shutdown(cx),
            #[cfg(unix)]
            Conn::Unix(u) => Pin::new(u).poll_shutdown(cx),
        }
    }
}

/// Whether an error from `connect` only says that nobody listens there.
pub fn nobody(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused)
}

/// Connects to an address. A pipe that is busy (all its instances taken for a moment) is retried for `wait`.
pub async fn connect(address: &str, wait: Duration) -> io::Result<Conn> {
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        const ERROR_PIPE_BUSY: i32 = 231;
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            match ClientOptions::new().open(address) {
                Ok(client) => return Ok(Conn::Client(client)),
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }
    #[cfg(unix)]
    {
        let _ = wait;
        Ok(Conn::Unix(tokio::net::UnixStream::connect(address).await?))
    }
}

/// Where a node's dendrites listen. One node per address: a second one is refused with `AddrInUse`.
#[cfg(windows)]
pub struct Listener {
    name: String,
    next: tokio::net::windows::named_pipe::NamedPipeServer,
    security: windows::PipeSecurity,
}

#[cfg(windows)]
impl Listener {
    pub fn bind(address: &str) -> io::Result<Listener> {
        const ERROR_ACCESS_DENIED: i32 = 5;
        let security = windows::PipeSecurity::this_user()?;
        let next = security.create(address, true).map_err(|e| {
            if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) {
                io::Error::new(io::ErrorKind::AddrInUse, format!("another node answers at {address}"))
            } else {
                e
            }
        })?;
        Ok(Listener {
            name: address.to_string(),
            next,
            security,
        })
    }

    pub async fn accept(&mut self) -> io::Result<Conn> {
        loop {
            match self.next.connect().await {
                Ok(()) => {
                    let fresh = self.security.create(&self.name, false)?;
                    return Ok(Conn::Server(std::mem::replace(&mut self.next, fresh)));
                }
                // A client that came and went before it was taken: make a fresh instance and go on.
                Err(e) if matches!(e.raw_os_error(), Some(109) | Some(232)) => {
                    self.next = self.security.create(&self.name, false)?;
                }
                Err(e) => return Err(e),
            }
        }
    }
}

/// Where a node's dendrites listen. One node per address: a second one is refused with `AddrInUse`, and a stale
/// socket file nobody answers on is removed.
#[cfg(unix)]
pub struct Listener {
    inner: tokio::net::UnixListener,
    path: std::path::PathBuf,
}

#[cfg(unix)]
impl Listener {
    pub fn bind(address: &str) -> io::Result<Listener> {
        use std::os::unix::fs::PermissionsExt;
        let path = std::path::PathBuf::from(address);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
        match tokio::net::UnixListener::bind(&path) {
            Ok(inner) => Ok(Listener { inner, path }),
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
                if std::os::unix::net::UnixStream::connect(&path).is_ok() {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        format!("another node answers at {address}"),
                    ));
                }
                std::fs::remove_file(&path)?;
                Ok(Listener {
                    inner: tokio::net::UnixListener::bind(&path)?,
                    path,
                })
            }
            Err(e) => Err(e),
        }
    }

    pub async fn accept(&mut self) -> io::Result<Conn> {
        let (stream, _) = self.inner.accept().await?;
        Ok(Conn::Unix(stream))
    }
}

#[cfg(unix)]
impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(windows)]
mod windows {
    //! A pipe's access list admitting this user only (decision 83), in place of the default one, which lets every
    //! local account open the pipe for reading.

    use std::ffi::c_void;
    use std::io;
    use std::ptr::null_mut;
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    pub struct PipeSecurity {
        descriptor: PSECURITY_DESCRIPTOR,
    }

    // The descriptor is only read once made, and freed once, on drop.
    unsafe impl Send for PipeSecurity {}
    unsafe impl Sync for PipeSecurity {}

    impl PipeSecurity {
        pub fn this_user() -> io::Result<PipeSecurity> {
            let sid = user_sid()?;
            let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})").encode_utf16().chain(Some(0)).collect();
            let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
            // SAFETY: the string is NUL-terminated; the descriptor is written by the call and freed in drop.
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(PipeSecurity { descriptor })
        }

        pub fn create(&self, name: &str, first: bool) -> io::Result<NamedPipeServer> {
            let mut attributes = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.descriptor,
                bInheritHandle: 0,
            };
            // SAFETY: the attributes and the descriptor they point to outlive the call.
            unsafe {
                ServerOptions::new()
                    .first_pipe_instance(first)
                    .create_with_security_attributes_raw(
                        name,
                        &mut attributes as *mut SECURITY_ATTRIBUTES as *mut c_void,
                    )
            }
        }
    }

    impl Drop for PipeSecurity {
        fn drop(&mut self) {
            // SAFETY: made by ConvertStringSecurityDescriptorToSecurityDescriptorW, freed once.
            unsafe {
                LocalFree(self.descriptor);
            }
        }
    }

    /// The SID of the user this process runs as, as text (`S-1-5-21-…`).
    fn user_sid() -> io::Result<String> {
        // SAFETY: each handle and buffer is used within its lifetime and freed once.
        unsafe {
            let mut token: HANDLE = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut len = 0u32;
            GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len);
            let mut buffer = vec![0u64; (len as usize).div_ceil(8)];
            let ok = GetTokenInformation(token, TokenUser, buffer.as_mut_ptr().cast(), len, &mut len);
            CloseHandle(token);
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            let user = &*(buffer.as_ptr() as *const TOKEN_USER);
            let mut text: *mut u16 = null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut n = 0;
            while *text.add(n) != 0 {
                n += 1;
            }
            let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, n));
            LocalFree(text.cast());
            Ok(sid)
        }
    }
}
