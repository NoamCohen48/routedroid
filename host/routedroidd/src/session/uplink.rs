//! Phone → helper. The TCP reader runs on its own task (`read_frame` is not
//! cancellation-safe) and, once the session is Active, injects `IP_PACKET`
//! frames itself instead of routing them through the driver: packets never
//! wait behind control, and control never waits behind packets. Before
//! Active every frame goes to the driver, in order, so the transition to
//! Active cannot reorder a control frame after a packet.
//!
//! Injection never waits: a full helper queue drops the packet (counted),
//! as a router's full queue would; the phone's TCP stack keeps its own
//! backpressure on the transport.

use std::io;
use std::sync::Arc;

use routedroid_proto::frame::{self, Frame, FrameError, MessageType};
use routedroid_proto::ipv4;
use tokio::io::{AsyncRead, BufReader};
use tokio::sync::mpsc;

use super::progress::Counters;
use super::timers::LastRx;

/// Hands one IPv4 packet to the helper without waiting: `Ok(false)` means
/// its queue is full, an error that the helper is gone.
pub type Inject = Arc<dyn Fn(&[u8]) -> io::Result<bool> + Send + Sync>;

/// What the reader hands the driver.
#[derive(Debug)]
pub enum Inbound {
    Frame(Frame),
    /// The last thing the reader will send: the stream ended or broke.
    Broken(FrameError),
    HelperGone(String),
}

#[derive(Clone)]
pub struct Uplink {
    pub inject: Inject,
    pub counters: Arc<Counters>,
}

impl Uplink {
    /// §6 checks, then inject. `Err` only when the helper is gone.
    pub fn forward(&self, packet: &[u8]) -> Result<(), String> {
        if ipv4::check(packet).is_err() {
            Counters::bump(&self.counters.malformed);
            return Ok(());
        }
        match (self.inject)(packet) {
            Ok(true) => {
                let counters = &self.counters;
                Counters::carried(
                    &counters.from_phone,
                    &counters.bytes_from_phone,
                    packet.len(),
                )
            }
            Ok(false) => Counters::bump(&self.counters.congested),
            Err(e) => return Err(format!("inject into helper: {e}")),
        }
        Ok(())
    }
}

/// Read frames until the stream ends, the driver goes away, or the helper does.
pub async fn reader_task<R: AsyncRead + Unpin>(
    rd: R,
    mtu: u32,
    uplink: Uplink,
    last_rx: Arc<LastRx>,
    tx: mpsc::Sender<Inbound>,
) {
    let mut rd = BufReader::with_capacity(64 * 1024, rd);
    loop {
        let inbound = match frame::read_frame(&mut rd, mtu).await {
            Ok(frame) => {
                last_rx.touch();
                if frame.message_type == MessageType::IpPacket && uplink.counters.reached_active() {
                    match uplink.forward(&frame.body) {
                        Ok(()) => continue,
                        Err(e) => Inbound::HelperGone(e),
                    }
                } else {
                    Inbound::Frame(frame)
                }
            }
            Err(e) => Inbound::Broken(e),
        };
        let last = !matches!(inbound, Inbound::Frame(_));
        if tx.send(inbound).await.is_err() || last {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
