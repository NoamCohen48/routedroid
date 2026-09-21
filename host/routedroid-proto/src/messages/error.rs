//! ERROR and VPN_ERROR (§4.6).

use super::*;

/// §4.6 error codes. `Other` keeps unknown codes from a newer peer readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    ProtocolUnsupported,
    ProtocolError,
    AuthFailed,
    SessionMismatch,
    TransportUnsupported,
    VpnPermissionDenied,
    VpnEstablishFailed,
    ConfigRejected,
    Internal,
}

impl ErrorCode {
    pub const ALL: [ErrorCode; 9] = [
        Self::ProtocolUnsupported,
        Self::ProtocolError,
        Self::AuthFailed,
        Self::SessionMismatch,
        Self::TransportUnsupported,
        Self::VpnPermissionDenied,
        Self::VpnEstablishFailed,
        Self::ConfigRejected,
        Self::Internal,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProtocolUnsupported => "protocol_unsupported",
            Self::ProtocolError => "protocol_error",
            Self::AuthFailed => "auth_failed",
            Self::SessionMismatch => "session_mismatch",
            Self::TransportUnsupported => "transport_unsupported",
            Self::VpnPermissionDenied => "vpn_permission_denied",
            Self::VpnEstablishFailed => "vpn_establish_failed",
            Self::ConfigRejected => "config_rejected",
            Self::Internal => "internal",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

/// Body of VPN_ERROR and ERROR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    /// Only with `protocol_unsupported`: the versions the sender speaks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported: Option<Vec<u8>>,
}

impl ErrorBody {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code: code.as_str().to_string(), message: message.into(), supported: None }
    }

    pub fn protocol_unsupported(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::ProtocolUnsupported.as_str().to_string(),
            message: message.into(),
            supported: Some(vec![PROTOCOL_VERSION]),
        }
    }

    pub fn code(&self) -> Option<ErrorCode> {
        ErrorCode::parse(&self.code)
    }
}

impl Body for ErrorBody {
    fn validate(&self) -> Result<(), BodyError> {
        if self.code.is_empty() || !self.code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
            return Err(field("code", "snake_case identifier"));
        }
        if self.message.chars().count() > MAX_ERROR_MESSAGE_LEN {
            return Err(field("message", "at most 512 characters"));
        }
        Ok(())
    }
}
