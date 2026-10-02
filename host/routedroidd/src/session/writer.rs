//! The TCP writer: the one task that owns the write half. Frames queued
//! while it was writing go out together in one `write_all`.

use routedroid_proto::frame::{self, Frame};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

/// Bounded queue depth for every packet/frame channel.
pub const QUEUE_DEPTH: usize = 256;

/// Stop adding queued frames to a write once it is this large.
const BATCH_BYTES: usize = 64 * 1024;

pub async fn writer_task<W: AsyncWrite + Unpin>(
    mut wr: W,
    mut rx: mpsc::Receiver<Frame>,
) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(BATCH_BYTES + frame::HEADER_LEN + 65_536);
    while let Some(first) = rx.recv().await {
        buf.clear();
        first.encode_into(&mut buf);
        while buf.len() < BATCH_BYTES {
            let Ok(next) = rx.try_recv() else { break };
            next.encode_into(&mut buf);
        }
        wr.write_all(&buf).await?;
    }
    wr.shutdown().await.ok();
    Ok(())
}
