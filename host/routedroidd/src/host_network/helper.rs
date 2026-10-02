//! Talking to the helper: connect and agree on the IPC version, then one
//! request at a time.

use std::path::Path;

use anyhow::{bail, Context};
use routedroid_helper_ipc::{Datagram, Reply, Request, SeqPacket, MAX_DATAGRAM, VERSION};

use crate::fault::{Fault, FaultExt, Kind, Result};

/// A connection that has passed `Hello`.
pub async fn connect(socket: &Path) -> Result<SeqPacket> {
    let conn = SeqPacket::connect(socket)
        .with_context(|| format!("connect to helper socket {}", socket.display()))
        .fault(Kind::Helper)?;
    match request(&conn, &Request::Hello { version: VERSION })
        .await
        .fault(Kind::Helper)?
    {
        Reply::Hello { .. } => Ok(conn),
        Reply::Error { code, message } => Err(Fault::msg(
            Kind::Helper,
            format!("helper refused the handshake: {code:?}: {message}"),
        )),
        other => Err(unexpected(&other)),
    }
}

/// Send `request` and wait for its reply, skipping packets.
pub async fn request(conn: &SeqPacket, request: &Request) -> anyhow::Result<Reply> {
    conn.send_control(request).await.context("send to helper")?;
    let mut buf = vec![0u8; MAX_DATAGRAM];
    loop {
        let Some(datagram) = conn.recv(&mut buf).await.context("recv from helper")? else {
            bail!("helper closed the connection");
        };
        if let Datagram::Control(reply) =
            Datagram::<Reply>::decode(datagram).context("decode helper reply")?
        {
            return Ok(reply);
        }
    }
}

pub fn unexpected(reply: &Reply) -> Fault {
    Fault::msg(Kind::Helper, format!("unexpected helper reply {reply:?}"))
}
