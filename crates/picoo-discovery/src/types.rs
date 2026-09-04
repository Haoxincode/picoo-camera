//! mDNS advertisement record — whitelisted fields only.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SERVICE_TYPE: &str = "_picoocam._udp.local.";

/// Allowed TXT record keys for `_picoocam._udp.local` broadcasts.
pub const ALLOWED_TXT_KEYS: &[&str] = &[
    "receiver_id",
    "display_name",
    "platform",
    "quic_port",
    "pairing_state",
    "public_key_fingerprint_prefix",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiverPlatform {
    Windows,
    Macos,
}

impl ReceiverPlatform {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "windows" => Some(Self::Windows),
            "macos" => Some(Self::Macos),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingState {
    Open,
    PairedOnly,
}

impl PairingState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::PairedOnly => "paired_only",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "paired_only" => Some(Self::PairedOnly),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiverAdvertisement {
    pub receiver_id: String,
    pub display_name: String,
    pub platform: ReceiverPlatform,
    pub quic_port: u16,
    pub pairing_state: PairingState,
    pub public_key_fingerprint_prefix: String,
}

impl ReceiverAdvertisement {
    pub fn new(
        receiver_id: impl Into<String>,
        display_name: impl Into<String>,
        platform: ReceiverPlatform,
        quic_port: u16,
        public_key_fingerprint_prefix: impl Into<String>,
    ) -> Self {
        Self {
            receiver_id: receiver_id.into(),
            display_name: display_name.into(),
            platform,
            quic_port,
            pairing_state: PairingState::Open,
            public_key_fingerprint_prefix: public_key_fingerprint_prefix.into(),
        }
    }

    pub fn with_pairing_state(mut self, state: PairingState) -> Self {
        self.pairing_state = state;
        self
    }

    /// TXT pairing_state for A→W V1: stay `open` so new Senders can still pair.
    /// Re-advertise is still required when trust/name changes so TXT stays fresh.
    pub fn pairing_state_for_v1_receiver(_trusted_count: usize) -> PairingState {
        PairingState::Open
    }

    pub fn to_txt_properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("receiver_id", self.receiver_id.clone()),
            ("display_name", self.display_name.clone()),
            ("platform", self.platform.as_str().into()),
            ("quic_port", self.quic_port.to_string()),
            ("pairing_state", self.pairing_state.as_str().into()),
            (
                "public_key_fingerprint_prefix",
                self.public_key_fingerprint_prefix.clone(),
            ),
        ]
    }

    pub fn from_txt_properties(
        properties: &[(String, String)],
    ) -> Result<Self, AdvertisementError> {
        for (key, _) in properties {
            if !ALLOWED_TXT_KEYS.contains(&key.as_str()) {
                return Err(AdvertisementError::UnknownField(key.clone()));
            }
        }

        let lookup = |key: &str| -> Option<String> {
            properties
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };

        let receiver_id =
            lookup("receiver_id").ok_or(AdvertisementError::MissingField("receiver_id".into()))?;
        let display_name = lookup("display_name")
            .ok_or(AdvertisementError::MissingField("display_name".into()))?;
        let platform =
            lookup("platform").ok_or(AdvertisementError::MissingField("platform".into()))?;
        let platform =
            ReceiverPlatform::parse(&platform).ok_or(AdvertisementError::InvalidPlatform)?;
        let quic_port = lookup("quic_port")
            .ok_or(AdvertisementError::MissingField("quic_port".into()))?
            .parse::<u16>()
            .map_err(|_| AdvertisementError::InvalidPort)?;
        if quic_port == 0 {
            return Err(AdvertisementError::InvalidPort);
        }
        let pairing_state = lookup("pairing_state")
            .ok_or(AdvertisementError::MissingField("pairing_state".into()))?;
        let pairing_state =
            PairingState::parse(&pairing_state).ok_or(AdvertisementError::InvalidPairingState)?;
        let public_key_fingerprint_prefix = lookup("public_key_fingerprint_prefix").ok_or(
            AdvertisementError::MissingField("public_key_fingerprint_prefix".into()),
        )?;

        Ok(Self {
            receiver_id,
            display_name,
            platform,
            quic_port,
            pairing_state,
            public_key_fingerprint_prefix,
        })
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AdvertisementError {
    #[error("missing field: {0}")]
    MissingField(String),
    #[error("unknown field: {0}")]
    UnknownField(String),
    #[error("invalid quic_port")]
    InvalidPort,
    #[error("invalid platform")]
    InvalidPlatform,
    #[error("invalid pairing_state")]
    InvalidPairingState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txt_roundtrip_whitelist_only() {
        let ad = ReceiverAdvertisement::new(
            "recv-1",
            "Work PC",
            ReceiverPlatform::Macos,
            4433,
            "ab12cd34",
        );
        let props: Vec<(String, String)> = ad
            .to_txt_properties()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        let parsed = ReceiverAdvertisement::from_txt_properties(&props).expect("parse");
        assert_eq!(parsed, ad);
    }

    #[test]
    fn with_pairing_state_sets_txt() {
        let ad = ReceiverAdvertisement::new(
            "recv-1",
            "Work PC",
            ReceiverPlatform::Windows,
            4433,
            "ab12cd34",
        )
        .with_pairing_state(PairingState::PairedOnly);
        let props = ad.to_txt_properties();
        assert_eq!(
            props
                .iter()
                .find(|(k, _)| *k == "pairing_state")
                .map(|(_, v)| v.as_str()),
            Some("paired_only")
        );
        assert_eq!(
            ReceiverAdvertisement::pairing_state_for_v1_receiver(3),
            PairingState::Open
        );
    }

    #[test]
    fn rejects_unknown_txt_field() {
        let props = vec![
            ("receiver_id".into(), "r".into()),
            ("display_name".into(), "n".into()),
            ("platform".into(), "windows".into()),
            ("quic_port".into(), "1".into()),
            ("pairing_state".into(), "open".into()),
            ("public_key_fingerprint_prefix".into(), "fp".into()),
            ("video_state".into(), "secret".into()),
        ];
        assert_eq!(
            ReceiverAdvertisement::from_txt_properties(&props),
            Err(AdvertisementError::UnknownField("video_state".into()))
        );
    }

    #[test]
    fn rejects_zero_quic_port() {
        let props = vec![
            ("receiver_id".into(), "r".into()),
            ("display_name".into(), "n".into()),
            ("platform".into(), "windows".into()),
            ("quic_port".into(), "0".into()),
            ("pairing_state".into(), "open".into()),
            ("public_key_fingerprint_prefix".into(), "fp".into()),
        ];
        assert_eq!(
            ReceiverAdvertisement::from_txt_properties(&props),
            Err(AdvertisementError::InvalidPort)
        );
    }

    #[test]
    fn rejects_unknown_platform() {
        let props = vec![
            ("receiver_id".into(), "r".into()),
            ("display_name".into(), "n".into()),
            ("platform".into(), "linux".into()),
            ("quic_port".into(), "4433".into()),
            ("pairing_state".into(), "open".into()),
            ("public_key_fingerprint_prefix".into(), "fp".into()),
        ];
        assert_eq!(
            ReceiverAdvertisement::from_txt_properties(&props),
            Err(AdvertisementError::InvalidPlatform)
        );
    }
}
