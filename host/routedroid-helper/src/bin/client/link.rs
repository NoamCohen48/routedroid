//! The client's end of the helper connection: requests that wait for
//! their reply, packets out, and packets or replies in.

use anyhow::{Context, Result, bail};
use routedroid_helper_ipc::{Datagram, Lease, MAX_DATAGRAM, Reply, Request, SeqPacket};

pub struct Link {
    conn: SeqPacket,
    buf: Vec<u8>,
}

/// What `recv` saw.
pub enum Incoming<'a> {
    Reply(Reply),
    Packet(&'a [u8]),
}

impl Link {
    pub fn new(conn: SeqPacket) -> Self {
        Self {
            conn,
            buf: vec![0; MAX_DATAGRAM],
        }
    }

    /// Send `request` and wait for its reply; packets in between are
    /// dropped, and lease renewals printed.
    pub async fn request(&mut self, request: &Request) -> Result<Reply> {
        self.conn.send_control(request).await.context("send")?;
        loop {
            match self.recv().await? {
                Incoming::Reply(Reply::Lease { lease }) => print_lease(&lease),
                Incoming::Reply(reply) => return Ok(reply),
                Incoming::Packet(_) => {}
            }
        }
    }

    pub async fn send_packet(&self, packet: &[u8]) -> Result<()> {
        self.conn.send_packet(packet).await.context("send packet")
    }

    pub async fn recv(&mut self) -> Result<Incoming<'_>> {
        let Some(datagram) = self.conn.recv(&mut self.buf).await.context("recv")? else {
            bail!("helper closed the connection");
        };
        match Datagram::<Reply>::decode(datagram).context("decode")? {
            Datagram::Control(reply) => Ok(Incoming::Reply(reply)),
            Datagram::Packet(packet) => Ok(Incoming::Packet(packet)),
        }
    }
}

/// One line for the rigs to grep.
pub fn print_lease(lease: &Lease) {
    let dns: Vec<String> = lease.dns.iter().map(ToString::to_string).collect();
    println!(
        "LEASE server={} router={} dns={} expires_at={}",
        lease.server,
        lease.router.map_or("-".into(), |r| r.to_string()),
        dns.join(","),
        lease.expires_at
    );
}
