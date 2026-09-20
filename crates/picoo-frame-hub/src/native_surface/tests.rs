use super::*;

#[test]
fn descriptor_requires_resource_and_backend_lifetimes() {
    let identity = WindowsSharedSurfaceIdentity {
        source_connection_generation: 1,
        stream_epoch: 1,
        decoder_generation: 0,
        source_frame_id: 0,
        resource_generation: 1,
        backend_generation: 1,
        output_revision: 0,
    };
    assert!(WindowsSharedSurfaceDescriptor::new(
        1,
        WindowsAdapterId::from_luid(1, 2),
        64,
        32,
        WindowsSharedSurfaceFormat::Bgra8,
        0,
        identity,
    )
    .is_some());
    assert!(WindowsSharedSurfaceDescriptor::new(
        0,
        WindowsAdapterId::from_luid(1, 2),
        64,
        32,
        WindowsSharedSurfaceFormat::Bgra8,
        0,
        identity,
    )
    .is_none());
    assert!(WindowsSharedSurfaceDescriptor::new(
        1,
        WindowsAdapterId::from_luid(1, 2),
        64,
        32,
        WindowsSharedSurfaceFormat::Bgra8,
        1,
        identity,
    )
    .is_none());
}

#[test]
fn channel_is_bounded_and_rejects_stale_generation() {
    let identity = WindowsSharedSurfaceIdentity {
        source_connection_generation: 7,
        stream_epoch: 1,
        decoder_generation: 0,
        source_frame_id: 0,
        resource_generation: 11,
        backend_generation: 3,
        output_revision: 0,
    };
    let descriptor = WindowsSharedSurfaceDescriptor::new(
        1,
        WindowsAdapterId::from_luid(1, 2),
        64,
        32,
        WindowsSharedSurfaceFormat::Bgra8,
        0,
        identity,
    )
    .unwrap();
    let mut channel =
        WindowsNativeChannel::new(7, 1, WindowsAdapterId::from_luid(1, 2), 11, 3, 0).unwrap();
    channel.begin_handshake().unwrap();
    assert_eq!(
        channel.accept_ready(7, 1, 10, 3, 0),
        Err(WindowsNativeChannelError::InvalidGeneration)
    );
    channel.accept_ready(7, 1, 11, 3, 0).unwrap();
    let wrong_adapter = WindowsSharedSurfaceDescriptor::new(
        2,
        WindowsAdapterId::from_luid(9, 9),
        64,
        32,
        WindowsSharedSurfaceFormat::Bgra8,
        0,
        identity,
    )
    .unwrap();
    assert_eq!(
        channel.offer_frame(wrong_adapter),
        Err(WindowsNativeChannelError::InvalidDescriptor)
    );
    let offers = (0..WINDOWS_NATIVE_CHANNEL_MAX_IN_FLIGHT)
        .map(|_| channel.offer_frame(descriptor).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        channel.offer_frame(descriptor),
        Err(WindowsNativeChannelError::TooManyInFlight)
    );
    channel
        .acknowledge(offers[2], WindowsNativeChannelAck::Rejected)
        .unwrap();
    let replacement = channel.offer_frame(descriptor).unwrap();
    channel
        .acknowledge(offers[0], WindowsNativeChannelAck::Imported)
        .unwrap();
    assert_eq!(
        channel.acknowledge(offers[0], WindowsNativeChannelAck::Rejected),
        Err(WindowsNativeChannelError::InvalidAcknowledgement)
    );
    channel
        .acknowledge(offers[0], WindowsNativeChannelAck::Released)
        .unwrap();
    let replacement_two = channel.offer_frame(descriptor).unwrap();
    channel
        .acknowledge(offers[1], WindowsNativeChannelAck::Imported)
        .unwrap();
    let close_report = channel.close();
    assert_eq!(close_report.outstanding().len(), 3);
    assert!(close_report
        .outstanding()
        .contains(&(offers[1], WindowsNativeOfferState::Imported)));
    assert!(close_report
        .outstanding()
        .contains(&(replacement, WindowsNativeOfferState::Offered)));
    assert!(close_report
        .outstanding()
        .contains(&(replacement_two, WindowsNativeOfferState::Offered)));
    assert_eq!(
        channel.acknowledge(offers[1], WindowsNativeChannelAck::Released),
        Err(WindowsNativeChannelError::InvalidState)
    );
}

#[test]
fn wire_messages_round_trip_and_reject_trailing_bytes() {
    let identity = WindowsSharedSurfaceIdentity {
        source_connection_generation: 7,
        stream_epoch: 1,
        decoder_generation: 9,
        source_frame_id: 42,
        resource_generation: 11,
        backend_generation: 3,
        output_revision: 4,
    };
    let descriptor = WindowsSharedSurfaceDescriptor::new(
        0xfeed,
        WindowsAdapterId::from_luid(1, -2),
        1280,
        720,
        WindowsSharedSurfaceFormat::Bgra8,
        0,
        identity,
    )
    .unwrap();
    let messages = [
        WindowsNativeWireMessage::Demand {
            width: 1280,
            height: 720,
            output_revision: 4,
        },
        WindowsNativeWireMessage::Hello {
            source_connection_generation: 7,
            stream_epoch: 1,
            adapter: WindowsAdapterId::from_luid(1, -2),
            resource_generation: 11,
            backend_generation: 3,
            output_revision: 4,
        },
        WindowsNativeWireMessage::Ready {
            source_connection_generation: 7,
            stream_epoch: 1,
            resource_generation: 11,
            backend_generation: 3,
            output_revision: 4,
        },
        WindowsNativeWireMessage::Offer {
            offer_id: 12,
            descriptor,
        },
        WindowsNativeWireMessage::Ack {
            offer_id: 12,
            ack: WindowsNativeChannelAck::Imported,
        },
        WindowsNativeWireMessage::Close,
    ];
    for message in messages {
        let encoded = message.encode().unwrap();
        assert_eq!(WindowsNativeWireMessage::decode(&encoded), Ok(message));
    }

    let mut malformed = WindowsNativeWireMessage::Close.encode().unwrap();
    malformed.push(0);
    assert_eq!(
        WindowsNativeWireMessage::decode(&malformed),
        Err(WindowsNativeWireError::Malformed)
    );

    assert_eq!(
        WindowsNativeWireMessage::Demand {
            width: 720,
            height: 1280,
            output_revision: 4,
        }
        .encode(),
        Err(WindowsNativeWireError::Malformed)
    );
}
