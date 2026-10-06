//! One method per event the driver's loop can wake up on. Each returns
//! `Some(end)` when the session is over.

use routedroid_proto::frame::{Frame, FrameError, MessageType};
use routedroid_proto::messages::{ErrorBody, ErrorCode};
use routedroid_proto::state::State;
use tracing::{debug, info, warn};

use super::driver::SessionDriver;
use super::timers::{Idle, PhaseTimer};
use super::uplink::Inbound;
use super::{Close, Outbound, SessionEnd};

impl SessionDriver {
    pub(super) async fn on_shutdown(&mut self) -> SessionEnd {
        let _ = self.out_tx.send(Frame::empty(MessageType::Stop)).await;
        SessionEnd::LocalStop
    }

    pub(super) async fn on_idle(&mut self) -> Option<SessionEnd> {
        match self.keepalive.on_idle() {
            Idle::Wait => None,
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
        // Configuring waits on a person answering the consent dialog, not on a protocol step.
        let code = if state == State::Configuring {
            ErrorCode::ConsentTimeout
        } else {
            ErrorCode::ProtocolError
        };
        self.refuse(ErrorBody::new(
            code,
            format!("no progress from {} within {budget:?}", state.name()),
        ))
        .await
    }

    /// What the reader task delivered; `None` means the task is gone.
    pub(super) async fn on_inbound(&mut self, inbound: Option<Inbound>) -> Option<SessionEnd> {
        match inbound.unwrap_or(Inbound::Broken(FrameError::Io("reader task ended".into()))) {
            Inbound::Frame(frame) => self.on_frame(frame).await,
            Inbound::HelperGone(e) => {
                warn!(error = %e, "helper gone");
                Some(SessionEnd::HelperClosed)
            }
            Inbound::Broken(FrameError::Truncated { clean: true }) => Some(SessionEnd::PeerClosed),
            Inbound::Broken(FrameError::Io(e)) => Some(SessionEnd::Transport(e)),
            Inbound::Broken(e @ FrameError::Truncated { clean: false }) => {
                Some(SessionEnd::Transport(e.to_string()))
            }
            Inbound::Broken(e) => Some(
                self.refuse(ErrorBody::new(ErrorCode::ProtocolError, e.to_string()))
                    .await,
            ),
        }
    }

    async fn on_frame(&mut self, frame: Frame) -> Option<SessionEnd> {
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
            match o {
                Outbound::ToPeer(f) => {
                    if self.out_tx.send(f).await.is_err() {
                        return Some(SessionEnd::Transport("TCP writer gone".into()));
                    }
                }
                // Only packets that raced the switch to Active come this way.
                Outbound::ToHelper(packet) => {
                    if let Err(e) = self.uplink.forward(&packet) {
                        warn!(error = %e, "helper gone");
                        return Some(SessionEnd::HelperClosed);
                    }
                }
            }
        }
        // One deadline for the whole handshake, a fresh one for the consent.
        if PhaseTimer::budget(self.machine.state()) != PhaseTimer::budget(state_before) {
            self.phase.reset();
        }
        if self.machine.state() == State::Active && !self.progress.reached_active() {
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
        let _ = self
            .out_tx
            .send(Frame::json(MessageType::Error, &body))
            .await;
        SessionEnd::Refused(body)
    }
}
