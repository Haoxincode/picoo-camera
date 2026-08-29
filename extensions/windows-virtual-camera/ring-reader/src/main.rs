//! Poll Shared Frame Ring and print frame stats — validates VCam consumer path on Linux CI.

use std::thread;
use std::time::Duration;

use picoo_frame_hub::{SharedFrameRingConsumer, DEFAULT_MAX_FRAME_BYTES};
use picoo_receiver::DEFAULT_SHARED_RING_NAME;
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let ring_name =
        std::env::var("PICOO_RING_NAME").unwrap_or_else(|_| DEFAULT_SHARED_RING_NAME.into());
    let consumer = loop {
        match SharedFrameRingConsumer::open(&ring_name, DEFAULT_MAX_FRAME_BYTES) {
            Ok(consumer) => break consumer,
            Err(err) => {
                eprintln!("Waiting for Shared Frame Ring `{ring_name}`: {err}");
                thread::sleep(Duration::from_millis(500));
            }
        }
    };

    println!("Attached to Shared Frame Ring `{ring_name}` — polling latest NV12 frames");

    let mut last_sequence = 0u64;
    loop {
        if let Some(view) = consumer.latest_frame() {
            if view.sequence != last_sequence {
                last_sequence = view.sequence;
                println!(
                    "seq={} {}x{} stride={} rotation={} bytes={} ts_us={}",
                    view.sequence,
                    view.width,
                    view.height,
                    view.stride,
                    view.rotation,
                    view.nv12.len(),
                    view.timestamp_us
                );
            }
        }
        thread::sleep(Duration::from_millis(16));
    }
}
