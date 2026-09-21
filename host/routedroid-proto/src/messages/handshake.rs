//! HELLO, HELLO_ACK and AUTH (§4.1–4.3).

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u8,
    pub session: String,
    pub device_port: u16,
    /// 32 random bytes, lowercase hex.
    pub client_nonce: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
}

impl Body for Hello {
    /// Note: `protocol` is deliberately *not* checked here; the host must
    /// answer a wrong version with ERROR `protocol_unsupported` (§9), which
    /// is a session decision, not a parse failure.
    fn validate(&self) -> Result<(), BodyError> {
        if !valid_session(&self.session) {
            return Err(field("session", "1-40 characters from [A-Za-z0-9._-]"));
        }
        if self.device_port == 0 {
            return Err(field("device_port", "must be 1-65535"));
        }
        check_hex("client_nonce", &self.client_nonce, HEX_NONCE_LEN)?;
        if self.app.as_ref().is_some_and(|a| a.chars().count() > MAX_APP_LEN) {
            return Err(field("app", "at most 64 characters"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloAck {
    pub protocol: u8,
    pub mtu: u32,
    /// 32 random bytes, lowercase hex.
    pub host_nonce: String,
    /// HMAC-SHA256(secret, "host" || transcript), lowercase hex.
    pub host_proof: String,
}

impl Body for HelloAck {
    fn validate(&self) -> Result<(), BodyError> {
        if self.protocol != PROTOCOL_VERSION {
            return Err(field("protocol", format!("must be {PROTOCOL_VERSION}")));
        }
        if self.mtu < MIN_MTU || self.mtu > MAX_PACKET_BODY {
            return Err(field("mtu", format!("must be {MIN_MTU}-{MAX_PACKET_BODY}")));
        }
        check_hex("host_nonce", &self.host_nonce, HEX_NONCE_LEN)?;
        check_hex("host_proof", &self.host_proof, HEX_PROOF_LEN)
    }
}

/// AUTH, Android → host: proves the phone holds the shell-delivered secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Auth {
    /// HMAC-SHA256(secret, "android" || transcript), lowercase hex.
    pub android_proof: String,
}

impl Body for Auth {
    fn validate(&self) -> Result<(), BodyError> {
        check_hex("android_proof", &self.android_proof, HEX_PROOF_LEN)
    }
}
