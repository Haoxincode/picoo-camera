//! Native source ownership; no CPU materialization variant or pixel getter.

#[cfg(target_os = "macos")]
mod apple;
#[cfg(target_os = "macos")]
pub use apple::{ApplePixelBufferLease, NativeImageError};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{D3D11ImageLease, NativeImageError};

#[derive(Debug, Clone)]
pub enum NativeImage {
    #[cfg(target_os = "macos")]
    Apple(ApplePixelBufferLease),
    #[cfg(target_os = "windows")]
    Windows(D3D11ImageLease),
    // Pure ownership/queue tests only. It holds no pixels and cannot be built
    // by another crate or appear in a production feature graph.
    #[cfg(test)]
    Fake { width: u32, height: u32 },
}

impl NativeImage {
    pub fn width(&self) -> u32 {
        match *self {
            #[cfg(target_os = "macos")]
            Self::Apple(ref image) => image.width(),
            #[cfg(target_os = "windows")]
            Self::Windows(ref image) => image.width(),
            #[cfg(test)]
            Self::Fake { width, .. } => width,
        }
    }

    pub fn height(&self) -> u32 {
        match *self {
            #[cfg(target_os = "macos")]
            Self::Apple(ref image) => image.height(),
            #[cfg(target_os = "windows")]
            Self::Windows(ref image) => image.height(),
            #[cfg(test)]
            Self::Fake { height, .. } => height,
        }
    }

    #[cfg(target_os = "windows")]
    pub fn windows(&self) -> Option<&D3D11ImageLease> {
        match self {
            Self::Windows(image) => Some(image),
            #[cfg(test)]
            Self::Fake { .. } => None,
        }
    }

    #[cfg(target_os = "macos")]
    pub fn apple(&self) -> Option<&ApplePixelBufferLease> {
        match self {
            Self::Apple(image) => Some(image),
            #[cfg(test)]
            Self::Fake { .. } => None,
        }
    }
}
