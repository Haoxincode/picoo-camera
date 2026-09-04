//! Sender ↔ Receiver protocol simulation over the public QUIC transport boundary.

use std::time::Duration;

use bytes::Bytes;
use picoo_packet::ReassemblyMap;
use picoo_protocol::control::{
    control_envelope::Payload as ControlPayload, ClientHello, ServerHello,
};
use picoo_protocol::{
    decode_control_envelope, encode_control_envelope, VideoPacket, VideoPacketFlags,
};
use picoo_transport::{
    Endpoint, PicooTransport, QuicReceiverTransport, QuicSenderTransport, SessionId,
    TransportError, TransportEvent,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QuicSimulationError {
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("timeout waiting for {0}")]
    Timeout(&'static str),
}

fn wait_for<T>(mut poll: impl FnMut() -> Option<T>) -> Result<T, QuicSimulationError> {
    for _ in 0..500 {
        if let Some(value) = poll() {
            return Ok(value);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Err(QuicSimulationError::Timeout("QUIC event"))
}

fn connect_loopback() -> Result<
    (
        QuicReceiverTransport,
        QuicSenderTransport,
        SessionId,
        SessionId,
    ),
    QuicSimulationError,
> {
    let mut receiver = QuicReceiverTransport::new();
    let addr = receiver.bind(Endpoint {
        host: "127.0.0.1".into(),
        port: 0,
    })?;
    let mut sender = QuicSenderTransport::new();
    let sender_session = sender.connect(Endpoint {
        host: addr.ip().to_string(),
        port: addr.port(),
    })?;

    wait_for(|| match sender.poll_event() {
        Some(TransportEvent::Connected(session)) => Some(session),
        _ => None,
    })?;
    let receiver_session = wait_for(|| match receiver.poll_event() {
        Some(TransportEvent::Connected(session)) => Some(session),
        _ => None,
    })?;
    Ok((receiver, sender, receiver_session, sender_session))
}

/// Run a minimal PCP session: reliable enveloped control plus FEC video datagrams.
pub fn run_quic_protocol_simulation() -> Result<(), QuicSimulationError> {
    let (mut receiver, mut sender, receiver_session, sender_session) = connect_loopback()?;

    let client_hello = ClientHello {
        sender_id: "android-sender".into(),
        device_name: "Pixel Test".into(),
        public_key: vec![1, 2, 3],
        sender_nonce: vec![2; 32],
    };
    sender.send_control(
        sender_session,
        encode_control_envelope(
            ControlPayload::ClientHello(client_hello),
            1,
            sender_session.0,
        ),
    )?;

    let received = wait_for(|| match receiver.poll_event() {
        Some(TransportEvent::ControlMessage(_, data)) => decode_control_envelope(&data)
            .ok()
            .and_then(|envelope| match envelope.payload {
                Some(ControlPayload::ClientHello(hello)) => Some(hello),
                _ => None,
            }),
        _ => None,
    })?;
    assert_eq!(received.sender_id, "android-sender");

    let server_hello = ServerHello {
        receiver_id: "windows-receiver".into(),
        display_name: "Work PC".into(),
        public_key: vec![4, 5, 6],
        pairing_required: true,
        receiver_nonce: vec![3; 32],
        transcript_hash: vec![4; 32],
        identity_signature: vec![5; 64],
    };
    receiver.send_control(
        receiver_session,
        encode_control_envelope(
            ControlPayload::ServerHello(server_hello),
            1,
            sender_session.0,
        ),
    )?;

    let response = wait_for(|| match sender.poll_event() {
        Some(TransportEvent::ControlMessage(_, data)) => decode_control_envelope(&data)
            .ok()
            .and_then(|envelope| match envelope.payload {
                Some(ControlPayload::ServerHello(hello)) => Some(hello),
                _ => None,
            }),
        _ => None,
    })?;
    assert_eq!(response.display_name, "Work PC");
    assert!(response.pairing_required);

    let fragments = [
        VideoPacket {
            flags: VideoPacketFlags::KEYFRAME | VideoPacketFlags::START_OF_ACCESS_UNIT,
            stream_epoch: 1,
            frame_id: 99,
            pts_us: 1,
            fragment_index: 0,
            fragment_count: 2,
            payload: Bytes::from_static(b"abc"),
        },
        VideoPacket {
            flags: VideoPacketFlags::END_OF_ACCESS_UNIT,
            stream_epoch: 1,
            frame_id: 99,
            pts_us: 1,
            fragment_index: 1,
            fragment_count: 2,
            payload: Bytes::from_static(b"def"),
        },
    ];
    let datagrams = fragments
        .into_iter()
        .map(|packet| packet.encode())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| QuicSimulationError::Protocol(error.to_string()))?;
    let batch = picoo_transport::VideoDatagramBatch::new(datagrams)
        .map_err(|error| QuicSimulationError::Protocol(error.to_string()))?;
    sender.send_video_batch(sender_session, batch)?;

    let mut reassembly = ReassemblyMap::new(8, 16);
    let frame = wait_for(|| match receiver.poll_event() {
        Some(TransportEvent::VideoPackets(_, packets)) => packets.into_iter().find_map(|packet| {
            reassembly
                .ingest(packet)
                .ok()
                .flatten()
                .map(|frame| frame.data)
        }),
        _ => None,
    })?;
    assert_eq!(frame.as_ref(), b"abcdef");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quic_sender_receiver_protocol_simulation() {
        run_quic_protocol_simulation().expect("simulation");
    }
}
