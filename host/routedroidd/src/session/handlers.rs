//! One method per event the driver's loop can wake up on. Each returns
//! `Some(end)` when the session is over.

use routedroid_proto::frame::{Frame, FrameError, MessageType};
use routedroid_proto::messages::{ErrorBody, ErrorCode};
use routedroid_proto::state::State;
use tracing::{debug, info};

use super::driver::SessionDriver;
use super::timers::{Idle, PhaseTimer};
use super::{Close, Outbound, SessionEnd};

impl SessionDriver {
    pub(super) async fn on_shutdown(&mut self) -> SessionEnd {
        let _ = self.out_tx.send(Frame::empty(MessageType::Stop)).await;
        SessionEnd::LocalStop
    }

    /// A packet read from the TUN, for the phone.
    pub(super) async fn on_helper_packet(&mut self, pkt: Option<Vec<u8>>) -> Option<SessionEnd> {
        let Some(pkt) = pkt else { return Some(SessionEnd::HelperClosed) };
        self.progress.bump_to_phone();
        if self.out_tx.send(Frame::ip_packet(pkt)).await.is_err() {
            return Some(SessionEnd::Transport("writer gone".into()));
        }
        None
    }

    pub(super) async fn on_idle(&mut self) -> Option<SessionEnd> {
        match self.keepalive.on_idle() {
            Idle::Dead => Some(SessionEnd::KeepaliveTimeout),
            Idle::SendPing => {
                let _ = self.out_tx.send(Frame::empty(MessageType::Ping)).await;
                None
            }
        }
    }

    pub(super) async fn on_phase_deadline(&mut self) -> SessionEnd {
        let state = self.machine.state();
        let budget = PhaseTimer::budget(state).unwrap_or_default();
        self.refuse(ErrorBody::new(
            ErrorCode::ProtocolError,
            format!("no progress from {} within {budget:?}", state.name()),
        ))
        .await
    }

    /// What the reader task delivered: a frame, a read error, or nothing
    /// (the task ended).
    pub(super) async fn on_inbound(&mut self, item: Option<Result<Frame, FrameError>>) -> Option<SessionEnd> {
        match item.unwrap_or(Err(FrameError::Io("reader task ended".into()))) {
            Ok(frame) => self.on_frame(frame).await,
            Err(FrameError::Truncated { clean: true }) => Some(SessionEnd::PeerClosed),
            Err(FrameError::Io(e)) => Some(SessionEnd::Transport(e)),
            Err(e @ FrameError::Truncated { clean: false }) => Some(SessionEnd::Transport(e.to_string())),
            Err(e) => Some(self.refuse(ErrorBody::new(ErrorCode::ProtocolError, e.to_string())).await),
        }
    }

    async fn on_frame(&mut self, frame: Frame) -> Option<SessionEnd> {
        self.keepalive.on_rx();
        let state_before = self.machine.state();
        if frame.message_type != MessageType::IpPacket {
            debug!(%frame.message_type, state = state_before.name(), "control frame");
        }
        let outbound = match self.machine.handle(frame) {
            Ok(outbound) => outbound,
            Err(Close::PeerStop) => return Some(SessionEnd::PeerStop),
            Err(Close::VpnError(e)) => return Some(SessionEnd::VpnError(e)),
            Err(Close::Refuse(e)) => return Some(self.refuse(e).await),
        };
        for o in outbound {
            let delivered = match o {
                Outbound::ToPeer(f) => self.out_tx.send(f).await.is_ok(),
                Outbound::ToHelper(pkt) => {
                    self.progress.bump_from_phone();
                    // Bounded: suspends the TCP reader when the helper lags.
                    self.to_helper.send(pkt).await.is_ok()
                }
            };
            if !delivered {
                break;
            }
        }
        if self.machine.state() != state_before {
            self.phase.reset();
        }
        if self.machine.state() == State::Active && !self.progress.counters.reached_active() {
            self.progress.set_active();
            info!("session Active");
        }
        if self.writer_ended() {
            return Some(SessionEnd::Transport("writer ended".into()));
        }
        None
    }

    /// Send ERROR and end the session with it.
    async fn refuse(&mut self, body: ErrorBody) -> SessionEnd {
        let _ = self.out_tx.send(Frame::json(MessageType::Error, &body)).await;
        SessionEnd::Refused(body)
    }
}
