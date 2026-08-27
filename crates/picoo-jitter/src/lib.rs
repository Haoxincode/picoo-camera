//! Jitter buffer — REQ-PICOO-SESSION-002.

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

    pub fn push(&mut self, frame: Frame) {
        self.frames.push_back(frame);
        self.enforce_limits();
    }

    pub fn pop_ready(&mut self, now_us: u64) -> Option<Frame> {
        let front = self.frames.front()?;
        if now_us.saturating_sub(front.pts_us) / 1_000 >= self.target_ms {
            return self.frames.pop_front();
        }
        None
    }

    pub fn drop_incomplete_before(&mut self, deadline_us: u64) {
        self.frames
            .retain(|f| f.pts_us + self.max_ms * 1_000 >= deadline_us);
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
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
}
