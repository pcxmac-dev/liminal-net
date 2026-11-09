//! Frames (NETWORK.md §5.2): a 4-byte big-endian length, then that many bytes of UTF-8 JSON, at most 16 MB. The same
//! on pipes and on sockets.

use crate::{NetError, Value};
use std::io::ErrorKind;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The largest frame: 16 MB (decision 45). A larger message is refused with words.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

/// One message as its frame's bytes.
pub fn encode(value: &Value) -> Result<Vec<u8>, NetError> {
    let body = serde_json::to_vec(value).map_err(|e| NetError::Frame(e.to_string()))?;
    if body.len() > MAX_FRAME {
        return Err(NetError::Frame(format!(
            "a message of {} bytes is larger than a frame holds (16 MB)",
            body.len()
        )));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Writes one message.
pub async fn write<W: AsyncWrite + Unpin + ?Sized>(w: &mut W, value: &Value) -> Result<(), NetError> {
    let bytes = encode(value)?;
    w.write_all(&bytes).await?;
    w.flush().await?;
    Ok(())
}

/// Reads one message: `None` when the other end closed between two frames.
pub async fn read<R: AsyncRead + Unpin + ?Sized>(r: &mut R) -> Result<Option<Value>, NetError> {
    let mut len = [0u8; 4];
    if let Err(e) = r.read_exact(&mut len).await {
        return if closed(&e) { Ok(None) } else { Err(e.into()) };
    }
    let n = u32::from_be_bytes(len) as usize;
    if n > MAX_FRAME {
        return Err(NetError::Frame(format!("a frame of {n} bytes is larger than 16 MB")));
    }
    let mut body = vec![0u8; n];
    r.read_exact(&mut body)
        .await
        .map_err(|e| if closed(&e) { NetError::Closed } else { e.into() })?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| NetError::Frame(format!("not JSON: {e}")))
}

/// Whether an error only says that the other end went away.
pub(crate) fn closed(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        ErrorKind::UnexpectedEof | ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted
    ) || matches!(e.raw_os_error(), Some(109) | Some(232)) // ERROR_BROKEN_PIPE, ERROR_NO_DATA
}
