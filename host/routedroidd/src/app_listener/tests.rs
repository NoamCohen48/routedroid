use std::net::SocketAddr;

use super::*;

fn expected() -> Expected {
    Expected {
        session: "s1".into(),
        device_port: 9000,
        mtu: 1400,
    }
}

fn hello(session: &str) -> Vec<u8> {
    let hello = Hello {
        protocol: 1,
        session: session.into(),
        device_port: 9000,
        client_nonce: [0xaa; 32],
        app: None,
    };
    Frame::json(MessageType::Hello, &hello).encode()
}

async fn connect(addr: SocketAddr, bytes: &[u8]) -> TcpStream {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(bytes).await.unwrap();
    stream
}

async fn listener() -> (AppListener, SocketAddr) {
    let listener = AppListener::bind().await.unwrap();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, listener.port()));
    (listener, addr)
}

#[tokio::test]
async fn squatters_do_not_keep_the_app_out() {
    let (listener, addr) = listener().await;
    let accepting =
        tokio::spawn(async move { listener.accept(Duration::from_secs(30), &expected()).await });
    // Silent squatters fill every screening slot but the last…
    let mut silent = Vec::new();
    for _ in 0..MAX_SCREENING - 1 {
        silent.push(connect(addr, b"").await);
    }
    // …another app guesses wrong, one speaks garbage…
    let mut wrong = connect(addr, &hello("s2")).await;
    let mut garbage = connect(addr, &[0xff; 8]).await;
    // …and the app still gets in.
    let _app = connect(addr, &hello("s1")).await;
    let candidate = accepting.await.unwrap().unwrap_or_else(|e| panic!("{e}"));
    let first: Hello = messages::parse(&candidate.hello.body).unwrap();
    assert_eq!(first.session, "s1");
    for refused in [&mut wrong, &mut garbage] {
        assert_eq!(
            frame::read_frame(refused, 1400).await.unwrap().message_type,
            MessageType::Error
        );
    }
}

#[tokio::test(start_paused = true)]
async fn nobody_in_time_is_a_timeout() {
    let (listener, _) = listener().await;
    let error = listener
        .accept(Duration::from_secs(1), &expected())
        .await
        .err()
        .unwrap();
    assert_eq!(error.kind(), Kind::Vpn);
}
