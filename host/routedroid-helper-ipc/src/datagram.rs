//! One SOCK_SEQPACKET datagram: `[kind u8][payload]`. Kind 0x01 carries a
//! JSON control message ([`Request`](crate::Request) or
//! [`Reply`](crate::Reply)), kind 0x10 exactly one raw IPv4 packet.

use serde::Serialize;
use serde::de::DeserializeOwned;

/// The largest IPv4 packet.
pub const MAX_PACKET: usize = 65_535;
/// The largest datagram either side sends: a packet plus its kind byte.
pub const MAX_DATAGRAM: usize = MAX_PACKET + 1;

pub(crate) const KIND_CONTROL: u8 = 0x01;
pub(crate) const KIND_PACKET: u8 = 0x10;

#[derive(Debug, PartialEq, Eq)]
pub enum Datagram<'a, M> {
    Control(M),
    Packet(&'a [u8]),
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("empty datagram")]
    Empty,
    #[error("unknown datagram kind {0:#04x}")]
    UnknownKind(u8),
    #[error("bad control message: {0}")]
    Control(#[from] serde_json::Error),
}

impl<'a, M: DeserializeOwned> Datagram<'a, M> {
    pub fn decode(datagram: &'a [u8]) -> Result<Self, DecodeError> {
        let (&kind, payload) = datagram.split_first().ok_or(DecodeError::Empty)?;
        match kind {
            KIND_CONTROL => Ok(Self::Control(serde_json::from_slice(payload)?)),
            KIND_PACKET => Ok(Self::Packet(payload)),
            other => Err(DecodeError::UnknownKind(other)),
        }
    }
}

/// The bytes of a control datagram.
pub(crate) fn encode_control<M: Serialize>(message: &M) -> Vec<u8> {
    let mut datagram = vec![KIND_CONTROL];
    // Serializing our own message types cannot fail: no maps with non-string keys.
    serde_json::to_writer(&mut datagram, message).expect("control messages serialize");
    datagram
}

#[cfg(test)]
mod tests;
