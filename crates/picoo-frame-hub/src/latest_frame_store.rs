//! Capacity-one in-process decoded-frame store — REQ-PICOO-FRAME-009/010.

use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;

/// Immutable decoded frame shared by Preview and other in-process consumers.
///
/// Pixel ownership stays in `Bytes`, which can take over decoder `Vec<u8>`
/// storage without copying. The outer `Arc` shares the complete frame and its
/// media timeline without borrowing the Receiver session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFrame {
    pub sequence: u64,
    pub stream_generation: u64,
    pub frame_id: u64,
    pub source_pts_us: u64,
    pub received_at_us: u64,
    pub decoded_at: Instant,
    /// Receiver wall-clock timestamp retained for the Shared Frame Ring ABI and
    /// existing frame-age diagnostics. It is not cross-device clock mapping.
    pub timestamp_us: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub rotation: u32,
    pub pixel_data: Bytes,
}

impl VideoFrame {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        stream_generation: u64,
        frame_id: u64,
        source_pts_us: u64,
        received_at_us: u64,
        decoded_at: Instant,
        timestamp_us: u64,
        width: u32,
        height: u32,
        stride: u32,
        rotation: u32,
        pixel_data: Bytes,
    ) -> Self {
        Self {
            sequence: 0,
            stream_generation,
            frame_id,
            source_pts_us,
            received_at_us,
            decoded_at,
            timestamp_us,
            width,
            height,
            stride,
            rotation,
            pixel_data,
        }
    }
}

/// Receiver-owned, latest-only publication point.
///
/// The Receiver reducer is the sole writer, so an atomic swap dependency would
/// add no safety or concurrency benefit here. Consumers clone the returned
/// `Arc`; publishing a new frame never waits for an older consumer to finish.
#[derive(Debug, Default)]
pub struct LatestFrameStore {
    latest: Option<Arc<VideoFrame>>,
    latest_sequence: u64,
}

impl LatestFrameStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(&mut self, mut frame: VideoFrame) -> Arc<VideoFrame> {
        self.latest_sequence = self.latest_sequence.saturating_add(1);
        frame.sequence = self.latest_sequence;
        let frame = Arc::new(frame);
        self.latest = Some(Arc::clone(&frame));
        frame
    }

    pub fn latest(&self) -> Option<&Arc<VideoFrame>> {
        self.latest.as_ref()
    }

    pub fn latest_sequence(&self) -> u64 {
        self.latest_sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(marker: u8) -> VideoFrame {
        VideoFrame::new(
            7,
            marker.into(),
            u64::from(marker) * 1_000,
            u64::from(marker) * 1_100,
            Instant::now(),
            u64::from(marker) * 1_200,
            1280,
            720,
            1280,
            0,
            Bytes::from(vec![marker; 4]),
        )
    }

    #[test]
    fn returns_latest_complete_frame() {
        let mut store = LatestFrameStore::new();
        store.publish(frame(1));
        store.publish(frame(2));

        let latest = store.latest().expect("latest");
        assert_eq!(latest.sequence, 2);
        assert_eq!(latest.pixel_data.as_ref(), &[2, 2, 2, 2]);
    }

    #[test]
    fn held_consumer_frame_never_blocks_latest_publication() {
        let mut store = LatestFrameStore::new();
        let held = store.publish(frame(1));
        let latest = store.publish(frame(2));

        assert_eq!(held.sequence, 1);
        assert_eq!(held.pixel_data.as_ref(), &[1, 1, 1, 1]);
        assert_eq!(latest.sequence, 2);
        assert_eq!(store.latest_sequence(), 2);
    }

    #[test]
    fn consumer_clone_shares_the_complete_frame_and_pixels() {
        let mut store = LatestFrameStore::new();
        let published = store.publish(frame(3));
        let consumer = Arc::clone(store.latest().expect("latest"));

        assert!(Arc::ptr_eq(&published, &consumer));
        assert_eq!(published.pixel_data.as_ptr(), consumer.pixel_data.as_ptr());
    }
}
