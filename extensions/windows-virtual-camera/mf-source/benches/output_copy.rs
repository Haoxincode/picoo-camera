use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use picoo_virtual_camera_source::{copy_prepared_frame, nv12_len};

struct CountingAllocator;
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

// SAFETY: allocation pointers and layouts are delegated unchanged to System.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: `layout` is forwarded unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: values came from System.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: values are forwarded unchanged.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    let frame_len = nv12_len(1920, 1080).expect("1080p NV12");
    let prepared: Arc<[u8]> = vec![128_u8; frame_len].into();
    let mut media_foundation_buffer = vec![0_u8; frame_len];
    ALLOCATIONS.store(0, Ordering::Relaxed);
    let started = Instant::now();
    let mut frames = 0_u64;
    while started.elapsed() < Duration::from_millis(500) {
        let cached = Arc::clone(&prepared);
        copy_prepared_frame(black_box(&cached), black_box(&mut media_foundation_buffer))
            .expect("final copy");
        black_box((&cached, media_foundation_buffer.as_ptr()));
        frames += 1;
    }
    let elapsed = started.elapsed().as_secs_f64();
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    println!(
        "1080p cached RequestSample boundary: {:.1} frames/s, {:.2} allocations/frame, one {}-byte final copy/frame",
        frames as f64 / elapsed,
        allocations as f64 / frames as f64,
        frame_len,
    );
}
