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
    last_emitted_pts_us: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    Accepted,
    DroppedLate { keyframe: bool },
}

impl JitterBuffer {
    pub fn new(target_ms: u64, max_ms: u64) -> Self {
        Self {
            target_ms,
            max_ms,
            frames: VecDeque::new(),
            last_emitted_pts_us: None,
        }
    }

    pub fn target_ms(&self) -> u64 {
        self.target_ms
    }

    pub fn set_target_ms(&mut self, target_ms: u64) {
        self.target_ms = target_ms.min(self.max_ms);
    }

    pub fn push(&mut self, frame: Frame) -> PushOutcome {
        if self
            .last_emitted_pts_us
            .is_some_and(|emitted| frame.pts_us <= emitted)
        {
            return PushOutcome::DroppedLate {
                keyframe: frame.keyframe,
            };
        }
        // Complete AUs can themselves arrive out of order because their
        // fragments use QUIC Datagram. Keep media order inside the playout
        // window instead of preserving completion order.
        let index = self
            .frames
            .iter()
            .position(|buffered| buffered.pts_us > frame.pts_us)
            .unwrap_or(self.frames.len());
        self.frames.insert(index, frame);
        self.enforce_limits();
        PushOutcome::Accepted
    }

    pub fn pop_ready(&mut self, now_us: u64) -> Option<Frame> {
        let front = self.frames.front()?;
        if self.target_ms == 0 || now_us.saturating_sub(front.pts_us) / 1_000 >= self.target_ms {
            let frame = self.frames.pop_front()?;
            self.last_emitted_pts_us = Some(frame.pts_us);
            return Some(frame);
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
        self.last_emitted_pts_us = None;
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

    #[test]
    fn orders_cross_access_unit_reassembly_by_pts() {
        let mut buf = JitterBuffer::new(50, 120);
        buf.push(Frame {
            pts_us: 34_000,
            data: Bytes::from_static(b"newer"),
            keyframe: false,
        });
        buf.push(Frame {
            pts_us: 1_000,
            data: Bytes::from_static(b"older"),
            keyframe: true,
        });
        assert_eq!(buf.pop_ready(100_000).unwrap().data, b"older"[..]);
        assert_eq!(buf.pop_ready(100_000).unwrap().data, b"newer"[..]);
    }

    #[test]
    fn drops_an_older_au_that_completes_after_newer_playout() {
        let mut buf = JitterBuffer::new(50, 120);
        assert_eq!(
            buf.push(Frame {
                pts_us: 34_000,
                data: Bytes::from_static(b"newer"),
                keyframe: false,
            }),
            PushOutcome::Accepted
        );
        assert_eq!(buf.pop_ready(100_000).unwrap().data, b"newer"[..]);
        assert_eq!(
            buf.push(Frame {
                pts_us: 1_000,
                data: Bytes::from_static(b"late-key"),
                keyframe: true,
            }),
            PushOutcome::DroppedLate { keyframe: true }
        );
        assert!(buf.pop_ready(100_000).is_none());
    }
}
