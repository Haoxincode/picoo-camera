//! Sender ↔ Receiver protocol simulation over the public QUIC transport boundary.

use std::time::Duration;

use bytes::Bytes;
use picoo_packet::ReassemblyMap;
use picoo_protocol::control::{ClientHello, ServerHello};
use picoo_protocol::{VideoPacket, VideoPacketFlags, ALPN};
use picoo_transport::{
    Endpoint, PicooTransport, QuicReceiverTransport, QuicSenderTransport, SessionId,
    TransportError, TransportEvent,
};
use prost::Message;
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

/// Run a minimal PCP/4 session: reliable control messages plus FEC video datagrams.
pub fn run_quic_protocol_simulation() -> Result<(), QuicSimulationError> {
    let (mut receiver, mut sender, receiver_session, sender_session) = connect_loopback()?;

    let client_hello = ClientHello {
        sender_id: "android-sender".into(),
        device_name: "Pixel Test".into(),
        protocol_version: ALPN.into(),
        public_key: vec![1, 2, 3],
    };
    let mut hello_bytes = Vec::new();
    client_hello
        .encode(&mut hello_bytes)
        .map_err(|error| QuicSimulationError::Protocol(error.to_string()))?;
    sender.send_control(sender_session, Bytes::from(hello_bytes))?;

    let received = wait_for(|| match receiver.poll_event() {
        Some(TransportEvent::ControlMessage(_, data)) => ClientHello::decode(data).ok(),
        _ => None,
    })?;
    assert_eq!(received.sender_id, "android-sender");

    let server_hello = ServerHello {
        receiver_id: "windows-receiver".into(),
        display_name: "Work PC".into(),
        protocol_version: ALPN.into(),
        public_key: vec![4, 5, 6],
        pairing_required: true,
    };
    let mut response = Vec::new();
    server_hello
        .encode(&mut response)
        .map_err(|error| QuicSimulationError::Protocol(error.to_string()))?;
    receiver.send_control(receiver_session, Bytes::from(response))?;

    let response = wait_for(|| match sender.poll_event() {
        Some(TransportEvent::ControlMessage(_, data)) => ServerHello::decode(data).ok(),
        _ => None,
    })?;
    assert_eq!(response.display_name, "Work PC");
    assert!(response.pairing_required);

    let fragments = [
        VideoPacket {
            version: VideoPacket::VERSION,
            flags: VideoPacketFlags::KEYFRAME | VideoPacketFlags::START_OF_ACCESS_UNIT,
            stream_epoch: 1,
            frame_id: 99,
            pts_us: 1,
            fragment_index: 0,
            fragment_count: 2,
            payload: Bytes::from_static(b"abc"),
        },
        VideoPacket {
            version: VideoPacket::VERSION,
            flags: VideoPacketFlags::END_OF_ACCESS_UNIT,
            stream_epoch: 1,
            frame_id: 99,
            pts_us: 1,
            fragment_index: 1,
            fragment_count: 2,
            payload: Bytes::from_static(b"def"),
        },
    ];
    for packet in fragments {
        sender.send_video(sender_session, packet)?;
    }

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
