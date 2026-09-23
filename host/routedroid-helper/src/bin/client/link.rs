//! The client's end of the helper connection: requests that wait for
//! their reply, packets out, and packets or replies in.

use anyhow::{bail, Context, Result};
use routedroid_helper_ipc::{Datagram, Reply, Request, SeqPacket, MAX_DATAGRAM};

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
        Self { conn, buf: vec![0; MAX_DATAGRAM] }
    }

    /// Send `request` and wait for its reply; packets in between are dropped.
    pub async fn request(&mut self, request: &Request) -> Result<Reply> {
        self.conn.send_control(request).await.context("send")?;
        loop {
            if let Incoming::Reply(reply) = self.recv().await? {
                return Ok(reply);
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
