use std::sync::{Arc, Mutex};

use super::{
    classify_native_origin, next_session_revision, take_offered_handle, write_native_ack,
    NativeAckSink,
};
use crate::frame_provider::FrameOrigin;
use picoo_frame_hub::{
    WindowsNativeChannelAck, WindowsNativeWireMessage, WindowsSharedSurfaceIdentity,
};

#[test]
fn native_session_revision_never_wraps_into_an_old_identity() {
    assert_eq!(next_session_revision(7), Some(8));
    assert_eq!(next_session_revision(u64::MAX), None);
}

#[test]
fn repeated_native_source_identity_is_counted_as_cached() {
    let identity = WindowsSharedSurfaceIdentity {
        source_connection_generation: 3,
        stream_epoch: 4,
        decoder_generation: 5,
        source_frame_id: 6,
        resource_generation: 7,
        backend_generation: 8,
        output_revision: 9,
    };
    let mut previous = None;
    assert_eq!(
        classify_native_origin(&mut previous, identity),
        FrameOrigin::Fresh
    );
    assert_eq!(
        classify_native_origin(&mut previous, identity),
        FrameOrigin::Cached
    );
    let next = WindowsSharedSurfaceIdentity {
        source_frame_id: 10,
        ..identity
    };
    assert_eq!(
        classify_native_origin(&mut previous, next),
        FrameOrigin::Fresh
    );
}

#[derive(Default)]
struct AckCapture(Mutex<Vec<u8>>);

impl NativeAckSink for AckCapture {
    fn write_ack_frame(&self, bytes: &[u8]) {
        *self.0.lock().expect("capture") = bytes.to_vec();
    }
}

#[test]
fn rejected_offer_writes_through_an_already_locked_pipe_without_relocking_it() {
    let pipe = Arc::new(Mutex::new(AckCapture::default()));
    let pipe_guard = pipe.lock().expect("pipe serialization lock");
    write_native_ack(&*pipe_guard, 41, WindowsNativeChannelAck::Rejected);
    let bytes = pipe_guard.0.lock().expect("captured bytes").clone();
    drop(pipe_guard);

    assert_eq!(
        WindowsNativeWireMessage::decode(&bytes).expect("ack frame"),
        WindowsNativeWireMessage::Ack {
            offer_id: 41,
            ack: WindowsNativeChannelAck::Rejected,
        }
    );
}

#[test]
fn wire_handle_must_name_a_real_non_pseudo_handle_before_raii_adoption() {
    assert!(take_offered_handle(0).is_err());
    assert!(take_offered_handle(u64::MAX).is_err());
    assert!(take_offered_handle(0xdead).is_err());
}
