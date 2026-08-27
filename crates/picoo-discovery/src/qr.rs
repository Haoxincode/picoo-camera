//! QR code connect payload — REQ-PICOO-DISCOVERY-003/004.

use picoo_protocol::ALPN;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const QR_PAYLOAD_VERSION: u32 = 1;
pub const DEFAULT_QR_TTL_MS: u64 = 120_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QrConnectPayload {
    pub v: u32,
    pub host: String,
    pub port: u16,
    pub receiver_id: String,
    pub protocol_version: String,
    pub public_key_fingerprint: String,
    pub nonce: String,
    pub expires_at_ms: u64,
}

impl QrConnectPayload {
    pub fn new(
        host: impl Into<String>,
        port: u16,
        receiver_id: impl Into<String>,
        public_key_fingerprint: impl Into<String>,
        nonce: impl Into<String>,
        now_ms: u64,
        ttl_ms: u64,
    ) -> Self {
        Self {
            v: QR_PAYLOAD_VERSION,
            host: host.into(),
            port,
            receiver_id: receiver_id.into(),
            protocol_version: ALPN.into(),
            public_key_fingerprint: public_key_fingerprint.into(),
            nonce: nonce.into(),
            expires_at_ms: now_ms.saturating_add(ttl_ms),
        }
    }

    pub fn encode_json(&self) -> Result<String, QrPayloadError> {
        serde_json::to_string(self).map_err(|e| QrPayloadError::Encode(e.to_string()))
    }

    pub fn decode_json(payload: &str) -> Result<Self, QrPayloadError> {
        let decoded: Self =
            serde_json::from_str(payload).map_err(|e| QrPayloadError::Decode(e.to_string()))?;
        if decoded.v != QR_PAYLOAD_VERSION {
            return Err(QrPayloadError::UnsupportedVersion(decoded.v));
        }
        Ok(decoded)
    }

    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_at_ms
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum QrPayloadError {
    #[error("encode failed: {0}")]
    Encode(String),
    #[error("decode failed: {0}")]
    Decode(String),
    #[error("unsupported QR payload version: {0}")]
    UnsupportedVersion(u32),
    #[error("expired")]
    Expired,
}

pub fn generate_nonce() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_payload_roundtrip() {
        let payload = QrConnectPayload::new(
            "192.168.1.10",
            4433,
            "recv-1",
            "fingerprint-full",
            "nonce-abc",
            1_000,
            DEFAULT_QR_TTL_MS,
        );
        let json = payload.encode_json().expect("encode");
        let parsed = QrConnectPayload::decode_json(&json).expect("decode");
        assert_eq!(parsed, payload);
        assert_eq!(parsed.protocol_version, ALPN);
    }

    #[test]
    fn qr_nonce_expires() {
        let payload = QrConnectPayload::new("127.0.0.1", 1, "r", "fp", "n", 1000, 500);
        assert!(!payload.is_expired(1200));
        assert!(payload.is_expired(1500));
        assert!(payload.is_expired(1501));
    }
}
