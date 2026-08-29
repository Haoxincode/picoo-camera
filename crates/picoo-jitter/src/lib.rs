//! Jitter buffer — REQ-PICOO-SESSION-002.
//!
//! `now_us` passed to [`JitterBuffer::pop_ready`] / [`JitterBuffer::drop_incomplete_before`]
//! must share the same timeline as [`Frame::pts_us`] (typically a media clock anchored at
//! the first buffered AU). Do not pass wall-clock UNIX microseconds when PTS is relative.

use bytes::Bytes;
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct Frame {
    pub pts_us: u64,
    pub data: Bytes,
    pub keyframe: bool,
}

#[derive(Debug)]
pub struct JitterBuffer {
    target_ms: u64,
    max_ms: u64,
    frames: VecDeque<Frame>,
}

impl JitterBuffer {
    pub fn new(target_ms: u64, max_ms: u64) -> Self {
        Self {
            target_ms,
            max_ms,
            frames: VecDeque::new(),
        }
    }

    pub fn target_ms(&self) -> u64 {
        self.target_ms
    }

    pub fn set_target_ms(&mut self, target_ms: u64) {
        self.target_ms = target_ms.min(self.max_ms);
    }

    pub fn push(&mut self, frame: Frame) {
        self.frames.push_back(frame);
        self.enforce_limits();
    }

    pub fn pop_ready(&mut self, now_us: u64) -> Option<Frame> {
        let front = self.frames.front()?;
        if self.target_ms == 0 || now_us.saturating_sub(front.pts_us) / 1_000 >= self.target_ms {
            return self.frames.pop_front();
        }
        None
    }

    pub fn drop_incomplete_before(&mut self, deadline_us: u64) {
        self.frames
            .retain(|f| f.pts_us + self.max_ms * 1_000 >= deadline_us);
    }

    /// Approximate buffered depth in milliseconds (newest - oldest pts).
    pub fn depth_ms(&self) -> f64 {
        match (self.frames.front(), self.frames.back()) {
            (Some(first), Some(last)) if last.pts_us >= first.pts_us => {
                (last.pts_us - first.pts_us) as f64 / 1_000.0
            }
            (Some(_), Some(_)) => self.target_ms as f64,
            _ => 0.0,
        }
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn clear(&mut self) {
        self.frames.clear();
    }

    fn enforce_limits(&mut self) {
        while self.frames.len() > 8 {
            if let Some(idx) = self.frames.iter().position(|f| !f.keyframe) {
                self.frames.remove(idx);
            } else {
                self.frames.pop_front();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_frame_after_target_delay() {
        let mut buf = JitterBuffer::new(50, 120);
        buf.push(Frame {
            pts_us: 0,
            data: Bytes::from_static(b"f"),
            keyframe: true,
        });
        assert!(buf.pop_ready(40_000).is_none());
        assert!(buf.pop_ready(50_000).is_some());
    }

    #[test]
    fn zero_target_releases_immediately() {
        let mut buf = JitterBuffer::new(0, 120);
        buf.push(Frame {
            pts_us: 1_000_000,
            data: Bytes::from_static(b"f"),
            keyframe: false,
        });
        assert!(buf.pop_ready(0).is_some());
    }

    #[test]
    fn drop_incomplete_keeps_recent_media_pts() {
        let mut buf = JitterBuffer::new(50, 120);
        buf.push(Frame {
            pts_us: 1_000,
            data: Bytes::from_static(b"f"),
            keyframe: true,
        });
        // Media clock just after PTS — must not drop (max 120ms).
        buf.drop_incomplete_before(1_000);
        assert_eq!(buf.len(), 1);
        // Far ahead of PTS+max → drop.
        buf.drop_incomplete_before(1_000 + 120_000 + 1);
        assert!(buf.is_empty());
    }

    #[test]
    fn depth_ms_tracks_pts_span() {
        let mut buf = JitterBuffer::new(50, 120);
        buf.push(Frame {
            pts_us: 1_000,
            data: Bytes::from_static(b"a"),
            keyframe: true,
        });
        buf.push(Frame {
            pts_us: 21_000,
            data: Bytes::from_static(b"b"),
            keyframe: false,
        });
        assert!((buf.depth_ms() - 20.0).abs() < f64::EPSILON);
    }
}
