//! The two socket tasks used by the session driver.

use routedroid_proto::frame::{self, Frame, FrameError};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

/// Bounded queue depth for every packet/frame channel.
pub const QUEUE_DEPTH: usize = 256;

/// Reads frames on its own task so the main loop's `select!` never drops a
/// half-read frame (`read_frame` is not cancellation-safe).
pub async fn reader_task<R: AsyncRead + Unpin>(mut rd: R, mtu: u32, tx: mpsc::Sender<Result<Frame, FrameError>>) {
    loop {
        let r = frame::read_frame(&mut rd, mtu).await;
        let stop = r.is_err();
        if tx.send(r).await.is_err() || stop {
            return;
        }
    }
}

pub async fn writer_task<W: AsyncWrite + Unpin>(mut wr: W, mut rx: mpsc::Receiver<Frame>) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(frame::HEADER_LEN + 65_536);
    while let Some(f) = rx.recv().await {
        buf.clear();
        f.encode_into(&mut buf);
        wr.write_all(&buf).await?;
    }
    wr.shutdown().await.ok();
    Ok(())
}
