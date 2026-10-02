//! Async frame reader (tokio). Body allocation happens only after the
//! header passed [`validate_header`].

use super::*;

/// Read exactly one frame from an async stream. The body buffer is allocated
/// only after the header passed [`validate_header`].
pub async fn read_frame<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
    mtu: u32,
) -> Result<Frame, FrameError> {
    use tokio::io::AsyncReadExt;
    let mut hdr = [0u8; HEADER_LEN];
    let mut filled = 0usize;
    while filled < HEADER_LEN {
        let n = reader.read(&mut hdr[filled..]).await?;
        if n == 0 {
            return Err(FrameError::Truncated { clean: filled == 0 });
        }
        filled += n;
    }
    let raw = RawHeader::parse(&hdr);
    let message_type = validate_header(&raw, mtu)?;
    let mut body = vec![0u8; raw.body_length as usize];
    if !body.is_empty() {
        reader
            .read_exact(&mut body)
            .await
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::UnexpectedEof => FrameError::Truncated { clean: false },
                _ => FrameError::Io(e.to_string()),
            })?;
    }
    Ok(Frame::new(message_type, body))
}
