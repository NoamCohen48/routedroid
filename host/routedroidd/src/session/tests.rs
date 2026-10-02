use std::net::Ipv4Addr;

use routedroid_proto::auth::{self, Secret};
use routedroid_proto::frame::{Frame, MessageType};
use routedroid_proto::messages::{
    Auth, ConfigureVpn, ErrorCode, Hello, HelloAck, Prefix, VpnReady,
};
use routedroid_proto::state::State;

use super::*;

const SECRET: [u8; 32] = [7; 32];
const CLIENT_NONCE: [u8; 32] = [0xaa; 32];
const HOST_NONCE: [u8; 32] = [0xbb; 32];

fn cfg() -> SessionConfig {
    SessionConfig {
        mtu: 1400,
        addresses: vec![Prefix::new(Ipv4Addr::new(10, 0, 0, 2), 32)],
        routes: vec![Prefix::new(Ipv4Addr::UNSPECIFIED, 0)],
        dns: vec![],
        session_name: "test".into(),
        expected_session: "s1".into(),
        expected_device_port: 9000,
    }
}

fn machine() -> Machine {
    Machine::new(cfg(), Secret::new(SECRET), HOST_NONCE)
}

fn hello(session: &str, port: u16, protocol: u32) -> Frame {
    Frame::json(
        MessageType::Hello,
        &Hello {
            protocol,
            session: session.into(),
            device_port: port,
            client_nonce: CLIENT_NONCE,
            app: None,
        },
    )
}

fn auth_frame(secret: &[u8; 32], role: auth::Role) -> Frame {
    let t = auth::transcript("s1", 9000, &CLIENT_NONCE, &HOST_NONCE);
    Frame::json(
        MessageType::Auth,
        &Auth {
            android_proof: auth::proof(&Secret::new(*secret), role, &t),
        },
    )
}

fn refused(r: Result<Vec<Outbound>, Close>) -> ErrorCode {
    match r {
        Err(Close::Refuse(e)) => e.known_code().expect("known code"),
        other => panic!("expected refusal, got {other:?}"),
    }
}

fn to_active(m: &mut Machine) {
    let out = m.handle(hello("s1", 9000, 1)).unwrap();
    let Outbound::ToPeer(ack) = &out[0] else {
        panic!()
    };
    let ack: HelloAck = routedroid_proto::messages::parse(&ack.body).unwrap();
    let t = auth::transcript("s1", 9000, &CLIENT_NONCE, &HOST_NONCE);
    assert!(auth::verify(
        &Secret::new(SECRET),
        auth::Role::Host,
        &t,
        &ack.host_proof
    ));
    let out = m.handle(auth_frame(&SECRET, auth::Role::Android)).unwrap();
    let Outbound::ToPeer(cfgf) = &out[0] else {
        panic!()
    };
    let c: ConfigureVpn = routedroid_proto::messages::parse(&cfgf.body).unwrap();
    assert_eq!(c.addresses[0].address, Ipv4Addr::new(10, 0, 0, 2));
    assert_eq!(m.state(), State::Configuring);
    m.handle(Frame::json(
        MessageType::VpnReady,
        &VpnReady {
            addresses: vec![Prefix::new(Ipv4Addr::new(10, 0, 0, 2), 32)],
            mtu: 1400,
        },
    ))
    .unwrap();
    assert_eq!(m.state(), State::Active);
}

#[test]
fn happy_path_consumes_secret_and_forwards_packets() {
    let mut m = machine();
    to_active(&mut m);
    assert!(m.secret_consumed());
    let pkt = vec![
        0x45, 0, 0, 21, 0, 0, 0, 0, 64, 0xfd, 0, 0, 10, 0, 0, 2, 10, 0, 0, 1, 0,
    ];
    assert_eq!(
        m.handle(Frame::ip_packet(pkt.clone())).unwrap(),
        vec![Outbound::ToHelper(pkt)]
    );
    assert_eq!(
        m.handle(Frame::empty(MessageType::Ping)).unwrap(),
        vec![Outbound::ToPeer(Frame::empty(MessageType::Pong))]
    );
    assert_eq!(
        m.handle(Frame::empty(MessageType::Stop)),
        Err(Close::PeerStop)
    );
    assert_eq!(m.state(), State::Closed);
}

#[test]
fn wrong_protocol_is_refused_with_supported_list() {
    let mut m = machine();
    match m.handle(hello("s1", 9000, 2)) {
        Err(Close::Refuse(e)) => {
            assert_eq!(e.known_code(), Some(ErrorCode::ProtocolUnsupported));
            assert_eq!(e.supported, Some(vec![1]));
        }
        other => panic!("{other:?}"),
    }
    assert!(m.secret_consumed());
}

#[test]
fn session_or_port_mismatch_is_refused() {
    assert_eq!(
        refused(machine().handle(hello("s2", 9000, 1))),
        ErrorCode::SessionMismatch
    );
    assert_eq!(
        refused(machine().handle(hello("s1", 9001, 1))),
        ErrorCode::SessionMismatch
    );
}

#[test]
fn wrong_secret_and_reflection_fail_auth_and_consume_secret() {
    for frame in [
        auth_frame(&[8; 32], auth::Role::Android),
        auth_frame(&SECRET, auth::Role::Host),
    ] {
        let mut m = machine();
        m.handle(hello("s1", 9000, 1)).unwrap();
        assert_eq!(refused(m.handle(frame)), ErrorCode::AuthFailed);
        assert!(m.secret_consumed());
        assert_eq!(m.state(), State::Closed);
    }
}

#[test]
fn out_of_state_messages_are_protocol_errors() {
    let mut m = machine();
    assert_eq!(
        refused(m.handle(auth_frame(&SECRET, auth::Role::Android))),
        ErrorCode::ProtocolError
    );
    let mut m = machine();
    assert_eq!(
        refused(m.handle(Frame::ip_packet(vec![0x45; 21]))),
        ErrorCode::ProtocolError
    );
    let mut m = machine();
    m.handle(hello("s1", 9000, 1)).unwrap();
    assert_eq!(
        refused(m.handle(hello("s1", 9000, 1))),
        ErrorCode::ProtocolError
    );
    let mut m = machine();
    assert_eq!(
        refused(m.handle(Frame::empty(MessageType::Ping))),
        ErrorCode::ProtocolError
    );
}

#[test]
fn malformed_bodies_are_protocol_errors() {
    let mut m = machine();
    assert_eq!(
        refused(m.handle(Frame::new(MessageType::Hello, b"{".to_vec()))),
        ErrorCode::ProtocolError
    );
    let mut m = machine();
    assert_eq!(
        refused(m.handle(Frame::new(
            MessageType::Hello,
            br#"{"protocol":1,"session":"bad id","device_port":1,"client_nonce":"aa"}"#.to_vec()
        ))),
        ErrorCode::ProtocolError
    );
}

#[test]
fn vpn_ready_must_echo_configuration() {
    let mut m = machine();
    m.handle(hello("s1", 9000, 1)).unwrap();
    m.handle(auth_frame(&SECRET, auth::Role::Android)).unwrap();
    let bad = VpnReady {
        addresses: vec![Prefix::new(Ipv4Addr::new(10, 0, 0, 3), 32)],
        mtu: 1400,
    };
    assert_eq!(
        refused(m.handle(Frame::json(MessageType::VpnReady, &bad))),
        ErrorCode::ProtocolError
    );
}

#[test]
fn nothing_after_close() {
    let mut m = machine();
    assert_eq!(
        m.handle(Frame::empty(MessageType::Stop)),
        Err(Close::PeerStop)
    );
    assert_eq!(
        refused(m.handle(Frame::empty(MessageType::Stop))),
        ErrorCode::ProtocolError
    );
}
