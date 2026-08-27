//! Sender ↔ Receiver protocol simulation over QUIC loopback.

use std::time::Duration;

use bytes::Bytes;
use picoo_packet::ReassemblyMap;
use picoo_protocol::control::{ClientHello, ServerHello};
use picoo_protocol::{VideoPacket, VideoPacketFlags, ALPN};
use picoo_transport::{establish_loopback, QuicLoopback, CONTROL_STREAM_ID};
use prost::Message;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QuicSimulationError {
    #[error("transport: {0}")]
    Transport(#[from] picoo_transport::QuicTransportError),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("video: {0}")]
    Video(#[from] picoo_protocol::VideoPacketError),
    #[error("timeout waiting for {0}")]
    Timeout(&'static str),
}

fn pump<F, T>(pair: &mut QuicLoopback, max: usize, mut poll: F) -> Result<T, QuicSimulationError>
where
    F: FnMut(&mut QuicLoopback) -> Option<T>,
{
    for _ in 0..max {
        pair.client.drive()?;
        pair.server.drive()?;
        if let Some(value) = poll(pair) {
            return Ok(value);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Err(QuicSimulationError::Timeout("condition"))
}

/// Run a minimal PCP/1 session: ClientHello/ServerHello on stream 0, video via datagram.
pub fn run_quic_protocol_simulation() -> Result<(), QuicSimulationError> {
    let mut pair = establish_loopback()?;

    let client_hello = ClientHello {
        sender_id: "android-sender".into(),
        device_name: "Pixel Test".into(),
        protocol_version: ALPN.into(),
        public_key: vec![1, 2, 3],
    };
    let mut hello_bytes = Vec::new();
    client_hello
        .encode(&mut hello_bytes)
        .map_err(|e| QuicSimulationError::Protocol(format!("encode ClientHello: {e}")))?;

    pair.client.send_stream(CONTROL_STREAM_ID, &hello_bytes)?;

    let (stream_id, data) = pump(&mut pair, 200, |p| p.server.recv_stream().ok().flatten())?;
    assert_eq!(stream_id, CONTROL_STREAM_ID);

    let received = ClientHello::decode(data.as_slice())
        .map_err(|e| QuicSimulationError::Protocol(format!("decode ClientHello: {e}")))?;
    assert_eq!(received.sender_id, "android-sender");

    let server_hello = ServerHello {
        receiver_id: "windows-receiver".into(),
        display_name: "Work PC".into(),
        protocol_version: ALPN.into(),
        public_key: vec![4, 5, 6],
        pairing_required: true,
    };
    let mut ack = Vec::new();
    server_hello
        .encode(&mut ack)
        .map_err(|e| QuicSimulationError::Protocol(format!("encode ServerHello: {e}")))?;
    pair.server.send_stream(CONTROL_STREAM_ID, &ack)?;

    let (_, data) = pump(&mut pair, 200, |p| p.client.recv_stream().ok().flatten())?;
    let ack = ServerHello::decode(data.as_slice())
        .map_err(|e| QuicSimulationError::Protocol(format!("decode ServerHello: {e}")))?;
    assert_eq!(ack.display_name, "Work PC");
    assert!(ack.pairing_required);

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

    for packet in &fragments {
        pair.client.send_dgram(&packet.encode()?)?;
        pair.client.drive()?;
        pair.server.drive()?;
    }

    let mut reassembly = ReassemblyMap::new(8, 16);

    let frame = pump(&mut pair, 200, |p| {
        if let Ok(Some(raw)) = p.server.recv_dgram() {
            if let Ok(packet) = VideoPacket::decode(&raw) {
                if let Ok(Some(frame)) = reassembly.ingest(packet) {
                    return Some(frame);
                }
            }
        }
        None
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
