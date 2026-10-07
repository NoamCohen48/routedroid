//! Asking the helper which links could carry phones. Root may connect to
//! its socket whatever the group; the helper reads the policy afresh on
//! every survey, so asking again after writing it checks the new file.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use routedroid_helper_ipc::{
    Datagram, Interface, MAX_DATAGRAM, Reply, Request, SeqPacket, VERSION,
};

const TIMEOUT: Duration = Duration::from_secs(10);

pub async fn interfaces(socket: &Path) -> Result<Vec<Interface>> {
    tokio::time::timeout(TIMEOUT, survey(socket))
        .await
        .with_context(|| format!("the helper did not answer within {TIMEOUT:?}"))?
}

async fn survey(socket: &Path) -> Result<Vec<Interface>> {
    let conn = SeqPacket::connect(socket).with_context(|| {
        format!(
            "cannot reach the helper at {}: is it installed and its socket on? \
             (systemctl enable --now routedroid-helper.socket)",
            socket.display()
        )
    })?;
    match request(&conn, &Request::Hello { version: VERSION }).await? {
        Reply::Hello { .. } => {}
        other => bail!("the helper refused to talk: {other:?}"),
    }
    match request(&conn, &Request::Interfaces).await? {
        Reply::Interfaces { interfaces } => Ok(interfaces),
        Reply::Error { message, .. } => bail!("the helper could not list interfaces: {message}"),
        other => bail!("unexpected helper reply {other:?}"),
    }
}

async fn request(conn: &SeqPacket, request: &Request) -> Result<Reply> {
    conn.send_control(request)
        .await
        .context("send to the helper")?;
    let mut buf = vec![0u8; MAX_DATAGRAM];
    loop {
        let Some(datagram) = conn.recv(&mut buf).await.context("read from the helper")? else {
            bail!("the helper closed the connection");
        };
        if let Datagram::Control(reply) =
            Datagram::<Reply>::decode(datagram).context("decode a helper reply")?
        {
            return Ok(reply);
        }
    }
}
