//! Bounded reusable pixel storage — REQ-PICOO-FRAME-010.

use std::sync::{Arc, Mutex, MutexGuard, Weak};

use bytes::Bytes;

use crate::DEFAULT_MAX_FRAME_BYTES;

/// Retain enough storage for the latest frame plus two concurrently held
/// consumers without letting idle sessions keep an unbounded pixel heap.
pub const DEFAULT_FRAME_BUFFER_POOL_BUFFERS: usize = 3;
pub const DEFAULT_FRAME_BUFFER_POOL_BYTES: usize =
    DEFAULT_FRAME_BUFFER_POOL_BUFFERS * DEFAULT_MAX_FRAME_BYTES;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameBufferPoolStats {
    pub allocations: u64,
    pub reuses: u64,
    pub discarded_returns: u64,
    pub retained_buffers: usize,
    pub retained_bytes: usize,
}

/// Non-blocking pool for large decoded/converted frame buffers.
///
/// The limits apply to idle retained storage. When all buffers are still held
/// by slow consumers, checkout allocates another buffer instead of applying
/// backpressure to the Decoder. Excess returns are discarded, restoring the
/// configured bound once consumers release them.
#[derive(Debug, Clone)]
pub struct FrameBufferPool {
    inner: Arc<PoolInner>,
}

#[derive(Debug)]
struct PoolInner {
    max_buffers: usize,
    max_retained_bytes: usize,
    state: Mutex<PoolState>,
}

#[derive(Debug, Default)]
struct PoolState {
    generation: u64,
    buffers: Vec<Vec<u8>>,
    retained_bytes: usize,
    allocations: u64,
    reuses: u64,
    discarded_returns: u64,
}

/// Exclusive writable lease that becomes immutable shared [`Bytes`] after
/// [`FrameBuffer::freeze`]. The backing `Vec` returns to its pool only after
/// the final `Bytes` clone or slice is dropped.
#[derive(Debug)]
pub struct FrameBuffer {
    storage: Option<Vec<u8>>,
    pool: Weak<PoolInner>,
    generation: u64,
}

impl FrameBufferPool {
    pub fn with_limits(max_buffers: usize, max_retained_bytes: usize) -> Self {
        Self {
            inner: Arc::new(PoolInner {
                max_buffers,
                max_retained_bytes,
                state: Mutex::new(PoolState::default()),
            }),
        }
    }

    /// Checkout exact-length initialized storage without waiting for an
    /// outstanding consumer. A retained buffer is preferred by best fit.
    pub fn checkout(&self, length: usize) -> FrameBuffer {
        let (generation, storage) = {
            let mut state = self.inner.lock_state();
            let generation = state.generation;
            let storage = take_best_fit(&mut state, length);
            if storage.is_some() {
                state.reuses = state.reuses.saturating_add(1);
            } else {
                state.allocations = state.allocations.saturating_add(1);
            }
            (generation, storage)
        };
        // Allocation/reallocation can touch several MiB. Keep it outside the
        // pool lock so a consumer dropping its last Bytes view is never held
        // behind that work.
        let storage = match storage {
            None => vec![0_u8; length],
            Some(mut storage) => {
                storage.resize(length, 0);
                storage
            }
        };

        FrameBuffer {
            storage: Some(storage),
            pool: Arc::downgrade(&self.inner),
            generation,
        }
    }

    /// Drop all currently retained allocations and invalidate outstanding
    /// leases so their later return cannot repopulate a closed session.
    pub fn clear(&self) {
        let mut state = self.inner.lock_state();
        state.generation = state.generation.saturating_add(1);
        let buffers = std::mem::take(&mut state.buffers);
        state.retained_bytes = 0;
        drop(state);
        // Large backing allocations are released without holding the mutex.
        drop(buffers);
    }

    pub fn stats(&self) -> FrameBufferPoolStats {
        let state = self.inner.lock_state();
        FrameBufferPoolStats {
            allocations: state.allocations,
            reuses: state.reuses,
            discarded_returns: state.discarded_returns,
            retained_buffers: state.buffers.len(),
            retained_bytes: state.retained_bytes,
        }
    }
}

impl Default for FrameBufferPool {
    fn default() -> Self {
        Self::with_limits(
            DEFAULT_FRAME_BUFFER_POOL_BUFFERS,
            DEFAULT_FRAME_BUFFER_POOL_BYTES,
        )
    }
}

impl FrameBuffer {
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.storage
            .as_mut()
            .expect("frame buffer storage")
            .as_mut()
    }

    /// Transfer this lease into immutable reference-counted bytes. `bytes`
    /// owns the lease metadata and invokes its return path after the last
    /// shared view is released.
    pub fn freeze(self) -> Bytes {
        Bytes::from_owner(self)
    }
}

impl AsRef<[u8]> for FrameBuffer {
    fn as_ref(&self) -> &[u8] {
        self.storage
            .as_ref()
            .expect("frame buffer storage")
            .as_ref()
    }
}

impl Drop for FrameBuffer {
    fn drop(&mut self) {
        let Some(storage) = self.storage.take() else {
            return;
        };
        let Some(pool) = self.pool.upgrade() else {
            return;
        };
        pool.return_storage(storage, self.generation);
    }
}

impl PoolInner {
    fn lock_state(&self) -> MutexGuard<'_, PoolState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn return_storage(&self, storage: Vec<u8>, generation: u64) {
        let capacity = storage.capacity();
        let mut state = self.lock_state();
        let within_generation = generation == state.generation;
        let within_count = state.buffers.len() < self.max_buffers;
        let within_bytes = state
            .retained_bytes
            .checked_add(capacity)
            .is_some_and(|bytes| bytes <= self.max_retained_bytes);
        if !within_generation || !within_count || !within_bytes {
            state.discarded_returns = state.discarded_returns.saturating_add(1);
            return;
        }

        state.retained_bytes += capacity;
        state.buffers.push(storage);
    }
}

fn take_best_fit(state: &mut PoolState, length: usize) -> Option<Vec<u8>> {
    let index = state
        .buffers
        .iter()
        .enumerate()
        .filter(|(_, buffer)| buffer.capacity() >= length)
        .min_by_key(|(_, buffer)| buffer.capacity())
        .map(|(index, _)| index)
        .or_else(|| {
            state
                .buffers
                .iter()
                .enumerate()
                .max_by_key(|(_, buffer)| buffer.capacity())
                .map(|(index, _)| index)
        })?;
    let storage = state.buffers.swap_remove(index);
    state.retained_bytes = state.retained_bytes.saturating_sub(storage.capacity());
    Some(storage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_returns_only_after_last_shared_view_drops() {
        let pool = FrameBufferPool::with_limits(1, 64);
        let mut buffer = pool.checkout(32);
        buffer.as_mut_slice().fill(7);
        let pointer = buffer.as_ref().as_ptr();
        let pixels = buffer.freeze();
        let consumer = pixels.clone();

        drop(pixels);
        assert_eq!(pool.stats().retained_buffers, 0);
        drop(consumer);
        assert_eq!(pool.stats().retained_buffers, 1);

        let reused = pool.checkout(32);
        assert_eq!(reused.as_ref().as_ptr(), pointer);
        assert_eq!(pool.stats().reuses, 1);
    }

    #[test]
    fn exhausted_pool_allocates_without_blocking_then_discards_excess_returns() {
        let pool = FrameBufferPool::with_limits(1, 64);
        let first = pool.checkout(32).freeze();
        let second = pool.checkout(32).freeze();
        assert_eq!(pool.stats().allocations, 2);

        drop(first);
        drop(second);
        assert_eq!(
            pool.stats(),
            FrameBufferPoolStats {
                allocations: 2,
                discarded_returns: 1,
                retained_buffers: 1,
                retained_bytes: 32,
                ..FrameBufferPoolStats::default()
            }
        );
    }

    #[test]
    fn byte_limit_is_enforced_by_capacity_not_visible_length() {
        let pool = FrameBufferPool::with_limits(4, 16);
        let oversized = pool.checkout(17).freeze();
        drop(oversized);

        assert_eq!(pool.stats().retained_buffers, 0);
        assert_eq!(pool.stats().discarded_returns, 1);
    }

    #[test]
    fn clear_invalidates_outstanding_leases() {
        let pool = FrameBufferPool::with_limits(1, 64);
        let outstanding = pool.checkout(32).freeze();
        pool.clear();
        drop(outstanding);

        assert_eq!(pool.stats().retained_buffers, 0);
        assert_eq!(pool.stats().discarded_returns, 1);
    }

    #[test]
    fn smaller_retained_storage_can_grow_for_a_new_stream_size() {
        let pool = FrameBufferPool::with_limits(1, 128);
        drop(pool.checkout(16).freeze());

        let larger = pool.checkout(64);
        assert_eq!(larger.as_ref().len(), 64);
        assert_eq!(pool.stats().allocations, 1);
        assert_eq!(pool.stats().reuses, 1);
    }
}
