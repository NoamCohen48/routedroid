//! The four IPv4 header checks applied before injection (§6). A packet that
//! fails is dropped and counted; it is not a protocol violation.

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PacketError {
    #[error("shorter than an IPv4 header")]
    TooShort,
    #[error("IP version {0}, expected 4")]
    Version(u8),
    #[error("IHL {0} below 5")]
    Ihl(u8),
    #[error("total length {total} does not match body length {body}")]
    TotalLength { total: u16, body: usize },
    #[error("total length {total} shorter than header length {header}")]
    HeaderOverrun { total: u16, header: u16 },
}

pub fn check(packet: &[u8]) -> Result<(), PacketError> {
    if packet.len() < 20 {
        return Err(PacketError::TooShort);
    }
    let version = packet[0] >> 4;
    if version != 4 {
        return Err(PacketError::Version(version));
    }
    let ihl = packet[0] & 0x0f;
    if ihl < 5 {
        return Err(PacketError::Ihl(ihl));
    }
    let total = u16::from_be_bytes([packet[2], packet[3]]);
    if usize::from(total) != packet.len() {
        return Err(PacketError::TotalLength { total, body: packet.len() });
    }
    let header = u16::from(ihl) * 4;
    if total < header {
        return Err(PacketError::HeaderOverrun { total, header });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
