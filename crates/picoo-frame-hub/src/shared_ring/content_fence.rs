//! Cross-thread invalidation without waiting for copies — REQ-PICOO-FRAME-013.
use super::{layout::meta_at, mapping::ProducerMapping, SharedFrameKind};
use std::sync::{atomic::Ordering, Arc};

/// Owns the mapping lifetime and exposes only its atomic content generation.
/// It cannot read/write pixels or alter producer ownership and mapping metadata.
#[derive(Clone)]
pub struct RingContentFence {
    mapping: Arc<ProducerMapping>,
}

// SAFETY: The mapping is initialized before this handle is created and retained
// by Arc until all handles/producer are gone. Shared access is exclusively to an
// aligned AtomicU64; no API exposes the mapping, pixels, Shmem or mutable metadata.
unsafe impl Send for RingContentFence {}
unsafe impl Sync for RingContentFence {}

impl RingContentFence {
    pub(super) fn new(mapping: Arc<ProducerMapping>) -> Self {
        Self { mapping }
    }

    pub fn current(&self) -> u64 {
        unsafe { &(*meta_at(self.mapping.as_ptr())).content_generation }.load(Ordering::SeqCst)
    }

    pub fn kind(&self) -> Option<SharedFrameKind> {
        if self.current() == 0 {
            return None;
        }
        let signal =
            unsafe { &(*meta_at(self.mapping.as_ptr())).content_signal }.load(Ordering::SeqCst);
        (signal != 0)
            .then(|| SharedFrameKind::from_wire((signal & 1) as u32))
            .flatten()
    }

    /// Returns the new generation, or zero when permanently exhausted.
    /// No frame memory, OS locks, worker mutexes or GPU tasks are touched.
    pub fn invalidate(&self) -> u64 {
        self.invalidate_as(self.kind().unwrap_or(SharedFrameKind::Placeholder))
    }

    /// Advance the content generation and publish its semantic kind without
    /// creating CPU pixel demand. The kind changes first, so privacy
    /// transitions may fail closed before older pixels are fenced.
    pub fn invalidate_as(&self, kind: SharedFrameKind) -> u64 {
        let signal = unsafe { &(*meta_at(self.mapping.as_ptr())).content_signal };
        let _ = signal.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
            if current == 0 {
                return Some(0);
            }
            let revision = current >> 1;
            revision
                .checked_add(1)
                .filter(|next| *next <= (u64::MAX >> 1))
                .map(|next| (next << 1) | kind.signal_bit())
                .or(Some(0))
        });
        let generation = unsafe { &(*meta_at(self.mapping.as_ptr())).content_generation };
        let previous = generation
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                Some(if current == 0 {
                    0
                } else {
                    current.checked_add(1).unwrap_or(0)
                })
            })
            .expect("update always returns a value");
        if previous == 0 {
            0
        } else {
            previous.checked_add(1).unwrap_or(0)
        }
    }
}
