//! A stand-in for the Android app: reads the bootstrap record the daemon
//! wrote through adb, connects where the reverse mapping points, and runs
//! the app's side of the handshake up to `VPN_READY`.

use std::time::Duration;

use routedroid_proto::auth::{self, Role};
use routedroid_proto::bootstrap;
use routedroid_proto::frame::{Frame, MessageType, read_frame};
use routedroid_proto::messages::{self, Auth, ConfigureVpn, Hello, HelloAck, VpnReady};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use super::fake_adb::FakeAdb;

const CLIENT_NONCE: [u8; 32] = [0x5a; 32];
const MTU: u32 = 65_535;

pub struct FakeApp {
    stream: TcpStream,
    pub configure: ConfigureVpn,
}

impl FakeApp {
    /// What the app does once launched, up to an active VPN.
    pub async fn connect(adb: &FakeAdb, serial: &str) -> Self {
        let record = loop {
            if let Some(record) = adb.record(serial) {
                break bootstrap::decode(&record).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let (_, host_port) = adb
            .reverse(serial)
            .into_iter()
            .find(|(device, _)| *device == record.device_port)
            .expect("the record's port is mapped");
        let stream = TcpStream::connect(("127.0.0.1", host_port)).await.unwrap();
        let mut app = Self {
            stream,
            configure: ConfigureVpn {
                mtu: 0,
                addresses: vec![],
                routes: vec![],
                dns: vec![],
                session_name: String::new(),
            },
        };
        let hello = Hello {
            protocol: 1,
            session: record.session.clone(),
            device_port: record.device_port,
            client_nonce: CLIENT_NONCE,
            app: Some("fake".into()),
        };
        app.send(Frame::json(MessageType::Hello, &hello)).await;
        let ack: HelloAck = app.expect(MessageType::HelloAck).await;
        let transcript = auth::transcript(
            &record.session,
            record.device_port,
            &CLIENT_NONCE,
            &ack.host_nonce,
        );
        assert!(auth::verify(
            &record.secret,
            Role::Host,
            &transcript,
            &ack.host_proof
        ));
        let android_proof = auth::proof(&record.secret, Role::Android, &transcript);
        app.send(Frame::json(MessageType::Auth, &Auth { android_proof }))
            .await;
        app.configure = app.expect(MessageType::ConfigureVpn).await;
        let ready = VpnReady {
            addresses: app.configure.addresses.clone(),
            mtu: app.configure.mtu,
        };
        app.send(Frame::json(MessageType::VpnReady, &ready)).await;
        app
    }

    pub async fn send(&mut self, frame: Frame) {
        self.stream.write_all(&frame.encode()).await.unwrap();
    }

    /// The next frame that is not a keepalive.
    pub async fn next(&mut self) -> Frame {
        loop {
            let frame = read_frame(&mut self.stream, MTU).await.unwrap();
            if frame.message_type != MessageType::Ping {
                return frame;
            }
            self.send(Frame::empty(MessageType::Pong)).await;
        }
    }

    async fn expect<T: messages::Body>(&mut self, message_type: MessageType) -> T {
        let frame = self.next().await;
        assert_eq!(frame.message_type, message_type, "{frame:?}");
        messages::parse(&frame.body).unwrap()
    }
}
