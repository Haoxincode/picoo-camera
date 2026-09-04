use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use picoo_frame_hub::{nv12_byte_size, transform_nv12};

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const SAMPLE_TIME: Duration = Duration::from_millis(500);

struct CountingAllocator;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

// SAFETY: every operation delegates unchanged pointers and layouts to the
// process System allocator; only an independent atomic counter is added.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: `layout` is forwarded unchanged to the wrapped allocator.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: both values came from the wrapped allocator.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: all values are forwarded unchanged to the wrapped allocator.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    run("no-op ownership transfer", 0, false);
    run("mirror", 0, true);
    run("rotate 90 + mirror", 90, true);
}

fn run(label: &str, rotation: u32, mirrored: bool) {
    let source = Bytes::from(vec![128_u8; nv12_byte_size(WIDTH, HEIGHT)]);
    ALLOCATIONS.store(0, Ordering::Relaxed);
    let started = Instant::now();
    let mut frames = 0_u64;
    while started.elapsed() < SAMPLE_TIME {
        let output = transform_nv12(WIDTH, HEIGHT, WIDTH, rotation, mirrored, source.clone())
            .expect("valid benchmark frame");
        black_box(output);
        frames += 1;
    }
    let elapsed = started.elapsed().as_secs_f64();
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    let mib_per_second =
        nv12_byte_size(WIDTH, HEIGHT) as f64 * frames as f64 / elapsed / (1024.0 * 1024.0);
    println!(
        "{label}: {:.1} frames/s, {:.2} allocations/frame, {:.1} MiB/s output-equivalent",
        frames as f64 / elapsed,
        allocations as f64 / frames as f64,
        mib_per_second,
    );
}
