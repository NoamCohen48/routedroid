//! Timing constants and the two socket tasks used by the session driver.

use std::time::Duration;

use routedroid_proto::frame::{self, Frame, FrameError};
use routedroid_proto::state::State;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

/// Bounded queue depth for every packet/frame channel.
pub const QUEUE_DEPTH: usize = 256;
/// §5.1: PING after this much silence, dead after `KEEPALIVE_DEAD`.
pub const KEEPALIVE_IDLE: Duration = Duration::from_secs(10);
pub const KEEPALIVE_DEAD: Duration = Duration::from_secs(30);
/// Connected → Configuring must complete within this (anyone on the phone can
/// connect to the reverse port; a silent peer must not hold the TUN).
pub const HANDSHAKE_DEADLINE: Duration = Duration::from_secs(15);
/// Configuring → Active includes the user answering the VPN consent dialog.
pub const CONSENT_DEADLINE: Duration = Duration::from_secs(120);


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

/// How long the peer may stay in the current pre-Active state.
pub fn phase_deadline(state: State) -> Option<Duration> {
    match state {
        State::Connected | State::Authenticating | State::Negotiated => Some(HANDSHAKE_DEADLINE),
        State::Configuring => Some(CONSENT_DEADLINE),
        State::Active | State::Closed => None,
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

