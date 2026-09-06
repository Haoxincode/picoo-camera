//! H.264 access-unit decoding — REQ-PICOO-MEDIA-005/006/012/023.
//!
//! Each submission carries an immutable token; delayed outputs return their original token.
//! - Windows: Media Foundation (`windows-mf`)
//! - macOS: VideoToolbox through pure Rust Apple framework bindings
//! - Unsupported product targets: explicit unavailable error
//! - Software codec/fixtures: only `test-codecs` or this crate's unit tests

#[cfg(all(target_os = "macos", any(test, feature = "test-codecs")))]
mod native_fixture;
#[cfg(all(windows, any(test, feature = "test-codecs")))]
#[path = "native_fixture/windows.rs"]
mod native_fixture;
#[cfg(all(windows, any(test, feature = "test-codecs", feature = "windows-mf")))]
#[path = "mf/runtime.rs"]
mod windows_runtime;

#[cfg(any(test, feature = "test-codecs"))]
mod stub;

#[cfg(all(windows, feature = "windows-mf"))]
mod mf;

#[cfg(target_os = "macos")]
mod videotoolbox;

#[cfg(all(
    not(windows),
    not(target_vendor = "apple"),
    any(test, feature = "test-codecs")
))]
mod openh264_dec;

#[cfg(any(
    test,
    target_os = "macos",
    all(windows, feature = "windows-mf"),
    all(not(windows), not(target_vendor = "apple"), feature = "test-codecs")
))]
mod configured_picture;

#[cfg(any(test, feature = "test-codecs"))]
mod fixture;
mod submission;
#[cfg(any(test, feature = "test-codecs"))]
pub use fixture::DecodeFixture;
pub use submission::{AccessUnitTimeline, DecodeSubmission, DecodeToken, FrameKind};
use thiserror::Error;

#[cfg(any(test, feature = "test-codecs"))]
pub use stub::StubDecoder;

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("decoder not initialized")]
    NotInitialized,
    #[error("unsupported access unit")]
    UnsupportedAccessUnit,
    #[error("access unit parameter sets differ from committed configuration")]
    ConfigurationMismatch,
    #[error("platform decoder: {0}")]
    Platform(String),
    #[error("output too large: {0} bytes")]
    OutputTooLarge(usize),
}

mod decoded_frame;
#[cfg(target_os = "macos")]
mod native_format;
#[cfg(any(target_os = "macos", all(windows, feature = "windows-mf")))]
mod source_format;
#[cfg(not(any(target_os = "macos", windows)))]
pub use decoded_frame::DecodedFrameStorage;
#[cfg(any(target_os = "macos", windows))]
pub use decoded_frame::NativeDecodedFormat;
pub use decoded_frame::{
    DecodeOutcome, DecodedFrame, DecodedFrameDescription, DecodedOutput, VideoColorMatrix,
    VideoColorRange, VideoPixelFormat,
};

/// Decode on the platform worker that created this instance.
///
/// REQ-PICOO-MEDIA-018: transfer the factory into a worker, never an initialized
/// platform decoder. COM apartments and codec teardown can be thread-affine.
pub trait AccessUnitDecoder {
    fn submit(&mut self, submission: DecodeSubmission<'_>) -> Result<DecodeOutcome, DecodeError>;

    /// Discard all queued output and prediction/reference state.
    ///
    /// Reset must not publish delayed frames. The next
    /// accepted access unit is expected to establish a fresh decode chain
    /// (normally an IDR with the active StreamConfig parameter sets).
    fn reset(&mut self) -> Result<(), DecodeError>;
}

/// Select the native production decoder for desktop targets.
pub fn create_platform_decoder() -> Box<dyn AccessUnitDecoder> {
    create_platform_decoder_impl()
}

#[cfg(target_os = "macos")]
fn create_platform_decoder_impl() -> Box<dyn AccessUnitDecoder> {
    tracing::info!("Using VideoToolbox H.264 decoder");
    Box::new(videotoolbox::VideoToolboxDecoder::new())
}

#[cfg(all(windows, feature = "windows-mf"))]
fn create_platform_decoder_impl() -> Box<dyn AccessUnitDecoder> {
    match mf::MfH264Decoder::new() {
        Ok(decoder) => {
            tracing::info!("Using Media Foundation H.264 decoder");
            Box::new(decoder)
        }
        Err(err) => {
            tracing::error!("MF decoder unavailable: {err}");
            Box::new(UnavailableDecoder(format!(
                "Media Foundation initialization failed: {err}"
            )))
        }
    }
}

#[cfg(any(test, not(target_os = "macos")))]
struct UnavailableDecoder(String);

#[cfg(any(test, not(target_os = "macos")))]
impl AccessUnitDecoder for UnavailableDecoder {
    fn submit(&mut self, _submission: DecodeSubmission<'_>) -> Result<DecodeOutcome, DecodeError> {
        Err(DecodeError::Platform(self.0.clone()))
    }

    fn reset(&mut self) -> Result<(), DecodeError> {
        Ok(())
    }
}

#[cfg(any(
    all(windows, not(feature = "windows-mf")),
    all(not(windows), not(target_os = "macos")),
))]
fn create_platform_decoder_impl() -> Box<dyn AccessUnitDecoder> {
    Box::new(UnavailableDecoder(
        "No native Receiver decoder for this product target/build".into(),
    ))
}

/// Explicit software decoder for core regression tests; the production factory
/// never calls this, even when the test-codecs feature is enabled.
#[cfg(all(
    not(windows),
    not(target_vendor = "apple"),
    any(test, feature = "test-codecs")
))]
pub fn create_test_decoder() -> Result<Box<dyn AccessUnitDecoder>, DecodeError> {
    Ok(Box::new(openh264_dec::OpenH264Decoder::new()?))
}

/// Explicit Windows software fixture; never selected by the product factory.
#[cfg(all(windows, feature = "windows-mf", any(test, feature = "test-codecs")))]
pub fn create_test_decoder() -> Result<Box<dyn AccessUnitDecoder>, DecodeError> {
    Ok(Box::new(mf::MfH264Decoder::software_diagnostic()?))
}

pub fn now_timestamp_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(
        all(windows, not(feature = "windows-mf")),
        all(not(windows), not(target_os = "macos")),
    ))]
    fn production_factory_never_uses_enabled_test_codecs() {
        // REQ-PICOO-MEDIA-024: even a build containing test codecs must fail closed.
        assert_unavailable_after_reset(create_platform_decoder());
    }

    #[test]
    fn unavailable_backend_remains_unavailable_after_reset() {
        assert_unavailable_after_reset(Box::new(UnavailableDecoder("test unavailable".into())));
    }

    fn assert_unavailable_after_reset(mut decoder: Box<dyn AccessUnitDecoder>) {
        assert!(matches!(
            decoder.decode_fixture(b"test-au", None),
            Err(DecodeError::Platform(_))
        ));
        // Reset discards state; an unavailable backend has no state to discard.
        decoder.reset().expect("empty reset");
        assert!(matches!(
            decoder.decode_fixture(b"test-au", None),
            Err(DecodeError::Platform(_))
        ));
    }

    #[test]
    fn stub_decodes_loopback_access_unit() {
        let mut decoder = StubDecoder::new();
        let frame = decoder
            .decode_fixture(b"test-au", None)
            .expect("decode")
            .into_fixture_frame()
            .expect("frame");
        #[cfg(not(any(target_os = "macos", windows)))]
        assert!(!frame.cpu_nv12_bytes().expect("CPU NV12").is_empty());
        #[cfg(target_os = "macos")]
        assert!(frame.native_image().apple().is_some());
        #[cfg(windows)]
        assert!(frame.native_image().windows().is_some());
        assert_eq!(frame.description().width, 1280);
        assert_eq!(frame.description().height, 720);
    }

    #[test]
    #[cfg(all(not(windows), not(target_vendor = "apple")))]
    fn openh264_roundtrip_produces_nv12() {
        use openh264::encoder::Encoder;
        use openh264::formats::YUVBuffer;

        let width = 64usize;
        let height = 64usize;
        let mut planes = vec![128u8; width * height * 3 / 2];
        for y in 0..height {
            for x in 0..width {
                planes[y * width + x] = ((x + y) % 256) as u8;
            }
        }
        let yuv = YUVBuffer::from_vec(planes, width, height);
        let mut encoder = Encoder::new().expect("encoder");
        let bitstream = encoder.encode(&yuv).expect("encode");
        let annex = bitstream.to_vec();
        assert!(
            annex.len() > 64,
            "encoded AU should exceed stub-heuristic size"
        );
        assert!(
            annex.windows(3).any(|w| w == [0, 0, 1]),
            "encoded AU must be Annex-B"
        );

        let mut decoder = create_test_decoder().expect("explicit software test decoder");
        let frame = decoder
            .decode_fixture(
                &picoo_bitstream::canonical_access_unit(
                    picoo_bitstream::Codec::Avc,
                    picoo_bitstream::NalFormat::AnnexB,
                    &annex,
                )
                .unwrap(),
                None,
            )
            .expect("decode")
            .into_fixture_frame()
            .expect("picture");
        let description = frame.description();
        let nv12 = frame.cpu_nv12_bytes().expect("CPU NV12");
        assert_eq!(description.width, width as u32);
        assert_eq!(description.height, height as u32);
        assert_eq!(
            nv12.len(),
            picoo_frame_hub::nv12_byte_size(description.width, description.height)
        );
        // Real decode should not be constant grey placeholder.
        assert!(nv12.iter().any(|b| *b != 16 && *b != 128));
    }

    #[test]
    #[cfg(all(not(windows), not(target_vendor = "apple")))]
    fn openh264_falls_back_to_stub_for_tiny_fixture() {
        let mut decoder = create_test_decoder().expect("explicit software test decoder");
        let frame = decoder
            .decode_fixture(b"test-au", None)
            .expect("decode")
            .into_fixture_frame()
            .expect("frame");
        assert_eq!(frame.description().width, 1280);
        assert_eq!(frame.description().height, 720);
    }

    #[test]
    #[cfg(all(not(windows), not(target_vendor = "apple")))]
    fn openh264_decodes_length_prefixed_au() {
        use openh264::encoder::Encoder;
        use openh264::formats::YUVBuffer;
        use picoo_bitstream::avc::{length_prefixed_to_annex_b, split_annex_b_nals};

        let width = 64usize;
        let height = 64usize;
        let mut planes = vec![128u8; width * height * 3 / 2];
        for y in 0..height {
            for x in 0..width {
                planes[y * width + x] = ((x * 7 + y) % 200 + 20) as u8;
            }
        }
        let yuv = YUVBuffer::from_vec(planes, width, height);
        let mut encoder = Encoder::new().expect("encoder");
        let annex = encoder.encode(&yuv).expect("encode").to_vec();
        // Rebuild as MediaCodec-style length-prefixed AU.
        let mut length_prefixed = Vec::new();
        for nal in split_annex_b_nals(&annex) {
            length_prefixed.extend_from_slice(&(nal.len() as u32).to_be_bytes());
            length_prefixed.extend_from_slice(nal);
        }
        assert!(
            length_prefixed_to_annex_b(&length_prefixed).is_some(),
            "fixture must look like AVCC AU"
        );

        let mut decoder = create_test_decoder().expect("explicit software test decoder");
        let frame = decoder
            .decode_fixture(&length_prefixed, None)
            .expect("decode")
            .into_fixture_frame()
            .expect("picture");
        assert_eq!(frame.description().width, width as u32);
        assert_eq!(frame.description().height, height as u32);
    }
}

#[cfg(any(test, all(windows, feature = "test-codecs")))]
#[path = "mf/nv12.rs"]
mod mf_nv12;
