//! JSON control bodies from protocol/phase0-draft.md. Field order matters for
//! the golden encodings, so keep it exactly as in the draft.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u8,
    pub session: String,
    pub device_port: u16,
    /// 32 random bytes, lowercase hex.
    pub client_nonce: String,
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

/// AUTH, Android -> Host: proves the phone holds the shell-delivered secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Auth {
    /// HMAC-SHA256(secret, "android" || transcript), lowercase hex.
    pub android_proof: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefix {
    pub address: String,
    pub prefix: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigureVpn {
    pub mtu: u32,
    pub addresses: Vec<Prefix>,
    pub routes: Vec<Prefix>,
    pub dns: Vec<String>,
    pub session_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VpnReady {
    pub addresses: Vec<String>,
    pub mtu: u32,
}

/// Body of VPN_ERROR and ERROR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

impl ErrorBody {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_string(), message: message.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_serialize_in_draft_field_order() {
        let ack = HelloAck { protocol: 0, mtu: 1400, host_nonce: "bb".into(), host_proof: "cc".into() };
        assert_eq!(
            serde_json::to_string(&ack).unwrap(),
            r#"{"protocol":0,"mtu":1400,"host_nonce":"bb","host_proof":"cc"}"#
        );
        let auth: Auth = serde_json::from_str(r#"{"android_proof":"dd"}"#).unwrap();
        assert_eq!(auth.android_proof, "dd");

        let cfg = ConfigureVpn {
            mtu: 1400,
            addresses: vec![Prefix { address: "192.168.10.74".into(), prefix: 32 }],
            routes: vec![Prefix { address: "0.0.0.0".into(), prefix: 0 }],
            dns: vec!["192.168.10.1".into()],
            session_name: "Routedroid Phase 0".into(),
        };
        assert_eq!(
            serde_json::to_string(&cfg).unwrap(),
            r#"{"mtu":1400,"addresses":[{"address":"192.168.10.74","prefix":32}],"routes":[{"address":"0.0.0.0","prefix":0}],"dns":["192.168.10.1"],"session_name":"Routedroid Phase 0"}"#
        );

        let hello: Hello =
            serde_json::from_str(r#"{"protocol":0,"session":"abc","device_port":9000,"client_nonce":"aa"}"#).unwrap();
        assert_eq!(hello, Hello { protocol: 0, session: "abc".into(), device_port: 9000, client_nonce: "aa".into() });

        let ready: VpnReady = serde_json::from_str(r#"{"addresses":["192.168.10.74/32"],"mtu":1400}"#).unwrap();
        assert_eq!(ready.mtu, 1400);
        assert_eq!(ready.addresses, vec!["192.168.10.74/32".to_string()]);
    }
}
