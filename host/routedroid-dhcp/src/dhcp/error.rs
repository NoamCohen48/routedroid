use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    TooShort(usize),
    BadMagic([u8; 4]),
    /// An option's length byte runs past the end of the buffer.
    TruncatedOption {
        code: u8,
        at: usize,
    },
    /// Option code without a length byte.
    DanglingCode {
        code: u8,
        at: usize,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort(n) => write!(f, "DHCP payload of {n} bytes shorter than 240"),
            Self::BadMagic(m) => write!(f, "bad magic cookie {m:02x?}"),
            Self::TruncatedOption { code, at } => {
                write!(f, "option {code} at offset {at} is truncated")
            }
            Self::DanglingCode { code, at } => {
                write!(f, "option {code} at offset {at} has no length byte")
            }
        }
    }
}

impl std::error::Error for ParseError {}
