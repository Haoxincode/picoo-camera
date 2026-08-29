//! Length-prefixed control stream framing — REQ-PICOO-PROTOCOL-002.
//!
//! Each control message on QUIC stream 0 is sent as `[u32 BE length][protobuf payload]`.

use bytes::Bytes;
use thiserror::Error;

pub const MAX_CONTROL_MESSAGE_SIZE: u32 = 64 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ControlFramingError {
    #[error("control message exceeds max size ({MAX_CONTROL_MESSAGE_SIZE} bytes)")]
    MessageTooLarge,
}

#[derive(Debug, Default)]
pub struct ControlFrameDecoder {
    buffer: Vec<u8>,
}

impl ControlFrameDecoder {
    pub fn push(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        self.buffer.extend_from_slice(chunk);
    }

    pub fn drain_messages(&mut self) -> Result<Vec<Bytes>, ControlFramingError> {
        let mut out = Vec::new();
        loop {
            if self.buffer.len() < 4 {
                break;
            }
            let len = u32::from_be_bytes([
                self.buffer[0],
                self.buffer[1],
                self.buffer[2],
                self.buffer[3],
            ]);
            if len > MAX_CONTROL_MESSAGE_SIZE {
                return Err(ControlFramingError::MessageTooLarge);
            }
            let total = 4 + len as usize;
            if self.buffer.len() < total {
                break;
            }
            out.push(Bytes::copy_from_slice(&self.buffer[4..total]));
            self.buffer.drain(0..total);
        }
        Ok(out)
    }
}

pub fn encode_control_frame(message: &[u8]) -> Result<Vec<u8>, ControlFramingError> {
    let len = u32::try_from(message.len()).map_err(|_| ControlFramingError::MessageTooLarge)?;
    if len > MAX_CONTROL_MESSAGE_SIZE {
        return Err(ControlFramingError::MessageTooLarge);
    }
    let mut out = Vec::with_capacity(4 + message.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(message);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_single_message() {
        let framed = encode_control_frame(b"hello").expect("encode");
        let mut decoder = ControlFrameDecoder::default();
        decoder.push(&framed);
        let messages = decoder.drain_messages().expect("drain");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].as_ref(), b"hello");
    }

    #[test]
    fn decodes_multiple_messages_in_one_chunk() {
        let a = encode_control_frame(b"msg-a").expect("a");
        let b = encode_control_frame(b"msg-b-longer").expect("b");
        let mut chunk = a;
        chunk.extend_from_slice(&b);

        let mut decoder = ControlFrameDecoder::default();
        decoder.push(&chunk);
        let messages = decoder.drain_messages().expect("drain");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].as_ref(), b"msg-a");
        assert_eq!(messages[1].as_ref(), b"msg-b-longer");
    }

    #[test]
    fn handles_partial_length_prefix() {
        let framed = encode_control_frame(b"partial").expect("encode");
        let mut decoder = ControlFrameDecoder::default();
        decoder.push(&framed[..2]);
        assert!(decoder.drain_messages().expect("drain").is_empty());
        decoder.push(&framed[2..]);
        let messages = decoder.drain_messages().expect("drain");
        assert_eq!(messages.len(), 1);
    }
}
