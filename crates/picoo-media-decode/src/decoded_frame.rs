//! Decoder output ownership. Mac output cannot be materialized as CPU pixels.

#[cfg(not(target_os = "macos"))]
use bytes::Bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoPixelFormat {
    Nv12,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoColorMatrix {
    Bt709,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoColorRange {
    Limited,
}

/// Geometry and interpretation of the immutable native output, not CPU layout.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeDecodedFormat {
    pub coded_size: picoo_frame_hub::ImageSize,
    pub visible_rect: picoo_frame_hub::VisibleRect,
    pub pixel_aspect_ratio: picoo_frame_hub::PixelAspectRatio,
    pub chroma_siting: picoo_frame_hub::ChromaSiting,
}

/// Pixel interpretation independent of the backing storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedFrameDescription {
    #[cfg(target_os = "macos")]
    pub native_format: NativeDecodedFormat,
    pub width: u32,
    pub height: u32,
    #[cfg(not(target_os = "macos"))]
    pub stride: u32,
    pub rotation: u32,
    pub pixel_format: VideoPixelFormat,
    pub color_matrix: VideoColorMatrix,
    pub color_range: VideoColorRange,
}

/// Non-Apple adapter storage, pending its native D3D11 replacement.
#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedFrameStorage {
    CpuNv12(Bytes),
}

#[derive(Debug, Clone)]
pub struct DecodedFrame {
    description: DecodedFrameDescription,
    timestamp_us: u64,
    #[cfg(not(target_os = "macos"))]
    storage: DecodedFrameStorage,
    #[cfg(target_os = "macos")]
    image: picoo_frame_hub::NativeImage,
}

impl DecodedFrame {
    /// Explicit test/diagnostic input upload; never a product decode fallback.
    #[cfg(any(test, feature = "test-codecs"))]
    pub fn fixture_nv12(
        width: u32,
        height: u32,
        stride: u32,
        rotation: u32,
        timestamp_us: u64,
        pixels: bytes::Bytes,
    ) -> Result<Self, crate::DecodeError> {
        #[cfg(target_os = "macos")]
        {
            Ok(Self::native(
                crate::native_fixture::upload(width, height, stride, &pixels)?,
                NativeDecodedFormat {
                    coded_size: picoo_frame_hub::ImageSize { width, height },
                    visible_rect: picoo_frame_hub::VisibleRect {
                        x: 0,
                        y: 0,
                        width,
                        height,
                    },
                    pixel_aspect_ratio: picoo_frame_hub::PixelAspectRatio {
                        numerator: 1,
                        denominator: 1,
                    },
                    chroma_siting: picoo_frame_hub::ChromaSiting::Left,
                },
                rotation,
                timestamp_us,
            ))
        }
        #[cfg(not(target_os = "macos"))]
        {
            Ok(Self::cpu_nv12(
                width,
                height,
                stride,
                rotation,
                timestamp_us,
                pixels,
            ))
        }
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn native(
        image: picoo_frame_hub::NativeImage,
        native_format: NativeDecodedFormat,
        rotation: u32,
        timestamp_us: u64,
    ) -> Self {
        Self {
            description: DecodedFrameDescription {
                native_format,
                width: image.width(),
                height: image.height(),
                rotation,
                pixel_format: VideoPixelFormat::Nv12,
                color_matrix: VideoColorMatrix::Bt709,
                color_range: VideoColorRange::Limited,
            },
            timestamp_us,
            image,
        }
    }

    #[cfg(target_os = "macos")]
    pub fn native_image(&self) -> &picoo_frame_hub::NativeImage {
        &self.image
    }

    #[cfg(target_os = "macos")]
    pub fn into_native_image(self) -> picoo_frame_hub::NativeImage {
        self.image
    }

    #[cfg(not(target_os = "macos"))]
    #[allow(clippy::too_many_arguments)]
    pub fn cpu_nv12(
        width: u32,
        height: u32,
        stride: u32,
        rotation: u32,
        timestamp_us: u64,
        nv12: Bytes,
    ) -> Self {
        Self {
            description: DecodedFrameDescription {
                width,
                height,
                stride,
                rotation,
                pixel_format: VideoPixelFormat::Nv12,
                color_matrix: VideoColorMatrix::Bt709,
                color_range: VideoColorRange::Limited,
            },
            timestamp_us,
            storage: DecodedFrameStorage::CpuNv12(nv12),
        }
    }

    pub fn description(&self) -> DecodedFrameDescription {
        self.description
    }

    pub fn timestamp_us(&self) -> u64 {
        self.timestamp_us
    }

    #[cfg(not(target_os = "macos"))]
    pub fn storage(&self) -> &DecodedFrameStorage {
        &self.storage
    }

    #[cfg(not(target_os = "macos"))]
    pub fn cpu_nv12_bytes(&self) -> Option<&Bytes> {
        match &self.storage {
            DecodedFrameStorage::CpuNv12(bytes) => Some(bytes),
        }
    }

    pub fn set_rotation(&mut self, rotation: u32) {
        self.description.rotation = rotation;
    }

    #[cfg(not(target_os = "macos"))]
    pub fn into_cpu_nv12(self) -> Bytes {
        match self.storage {
            DecodedFrameStorage::CpuNv12(bytes) => bytes,
        }
    }
}

/// Result of submitting one access unit to a platform decoder.
#[derive(Debug, Clone)]
pub struct DecodeOutcome {
    pub frames: Vec<DecodedOutput>,
    /// True only when this AU contained an IDR and the platform accepted it
    /// without reporting a drop. Receiver uses this to leave AwaitingRefresh.
    pub refresh_accepted: bool,
}

impl DecodeOutcome {
    #[cfg(any(test, feature = "test-codecs"))]
    pub fn into_fixture_frame(self) -> Option<DecodedFrame> {
        assert!(
            self.frames.len() <= 1,
            "fixture expected at most one output"
        );
        self.frames.into_iter().next().map(|output| output.frame)
    }

    pub fn frame(
        token: std::sync::Arc<crate::DecodeToken>,
        frame: DecodedFrame,
        refresh_accepted: bool,
    ) -> Self {
        Self {
            frames: vec![DecodedOutput { token, frame }],
            refresh_accepted,
        }
    }

    pub fn accepted_without_frame(refresh_accepted: bool) -> Self {
        Self {
            frames: Vec::new(),
            refresh_accepted,
        }
    }
}

/// A decoded picture belongs to its original submission, even when returned later.
#[derive(Debug, Clone)]
pub struct DecodedOutput {
    pub token: std::sync::Arc<crate::DecodeToken>,
    pub frame: DecodedFrame,
}
