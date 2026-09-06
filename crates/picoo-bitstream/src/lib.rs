//! Codec bitstream boundaries — REQ-PICOO-BITSTREAM-001.
//! No transport, GPU, UI, or software decoder dependencies.

pub mod avc;
mod avc_facts;
pub use avc_facts::{AvcColorFacts, AvcSpsFacts};

mod access_unit;
mod configuration;

pub use access_unit::{
    split_nals, AccessUnit, NalFormat, NalLengthSize, PictureInfo, PictureKind, RandomAccessPoint,
};
pub use configuration::CodecConfiguration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Avc,
    Hevc,
}

#[derive(Debug, thiserror::Error)]
pub enum BitstreamError {
    #[error("bitstream exceeds resource limits")]
    Limit,
    #[error("truncated bitstream")]
    Truncated,
    #[error("malformed bitstream: {0}")]
    Malformed(&'static str),
    #[error("unsupported bitstream: {0}")]
    Unsupported(&'static str),
    #[error("codec configuration: {0}")]
    Configuration(#[from] std::io::Error),
}
