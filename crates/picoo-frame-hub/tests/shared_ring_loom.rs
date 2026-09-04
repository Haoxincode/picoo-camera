//! Loom model for the Shared Frame Ring ready/reader-lease protocol.
//! REQ-PICOO-STACK-010 / ARCH-PICOO-RUNTIME-001.

use loom::cell::UnsafeCell;
use loom::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use loom::sync::Arc;
use loom::thread;

const READY_WRITING: u32 = 1;
const READY_DONE: u32 = 2;
const WRITER_LEASE: u32 = u32::MAX;

struct SlotModel {
    sequence: AtomicU64,
    ready: AtomicU32,
    readers: AtomicU32,
    payload: UnsafeCell<u64>,
}

// The production slot is shared through an OS mapping. Loom's UnsafeCell
// checks that the ready/lease atomics really exclude mutable payload access.
unsafe impl Sync for SlotModel {}

impl SlotModel {
    fn complete(sequence: u64) -> Self {
        Self {
            sequence: AtomicU64::new(sequence),
            ready: AtomicU32::new(READY_DONE),
            readers: AtomicU32::new(0),
            payload: UnsafeCell::new(sequence),
        }
    }

    fn try_publish(&self, sequence: u64) -> bool {
        if self
            .readers
            .compare_exchange(0, WRITER_LEASE, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return false;
        }
        self.ready.store(READY_WRITING, Ordering::Release);
        self.payload.with_mut(|payload| {
            // SAFETY: WRITER_LEASE excludes every reader before mutable access.
            unsafe { payload.write(sequence) };
        });
        self.sequence.store(sequence, Ordering::Release);
        self.ready.store(READY_DONE, Ordering::Release);
        self.readers.store(0, Ordering::SeqCst);
        true
    }

    fn read_consistent(&self) -> Option<(u64, u64)> {
        if self.ready.load(Ordering::Acquire) != READY_DONE {
            return None;
        }
        let sequence = self.sequence.load(Ordering::Acquire);
        let mut leases = self.readers.load(Ordering::SeqCst);
        loop {
            if leases >= WRITER_LEASE - 1 {
                return None;
            }
            match self.readers.compare_exchange_weak(
                leases,
                leases + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(current) => leases = current,
            }
        }
        if self.ready.load(Ordering::Acquire) != READY_DONE
            || self.sequence.load(Ordering::Acquire) != sequence
        {
            self.readers.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        let payload = self.payload.with(|payload| {
            // SAFETY: This reader owns one lease until the access completes.
            unsafe { payload.read() }
        });
        self.readers.fetch_sub(1, Ordering::SeqCst);
        Some((sequence, payload))
    }
}

#[test]
#[ignore = "exhaustive concurrency model; run with cargo xtask test loom"]
fn shared_ring_ready_and_reader_lease_protocol() {
    let mut model = loom::model::Builder::new();
    model.max_branches = 10_000;
    model.preemption_bound = Some(3);
    model.check(|| {
        let slot = Arc::new(SlotModel::complete(1));
        let writer_slot = Arc::clone(&slot);
        let writer = thread::spawn(move || writer_slot.try_publish(2));
        let reader_slot = Arc::clone(&slot);
        let reader = thread::spawn(move || reader_slot.read_consistent());

        let published = writer.join().expect("writer thread");
        if let Some((sequence, payload)) = reader.join().expect("reader thread") {
            assert_eq!(payload, sequence);
        }
        if published {
            assert_eq!(slot.read_consistent(), Some((2, 2)));
        } else {
            assert_eq!(slot.read_consistent(), Some((1, 1)));
        }
    });
}
