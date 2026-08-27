//! Sender session: packetization + transport flush.

use picoo_protocol::VideoPacket;
use picoo_transport::{Endpoint, PicooTransport, SessionId};

use crate::{SenderError, SenderPipeline, SenderStats};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionStats {
    pub pipeline: SenderStats,
    pub sent_datagrams: u64,
}

pub struct SenderSession<T: PicooTransport> {
    pipeline: SenderPipeline,
    transport: T,
    session: Option<SessionId>,
    sent_datagrams: u64,
}

impl<T: PicooTransport> SenderSession<T> {
    pub fn new(transport: T) -> Self {
        Self {
            pipeline: SenderPipeline::default(),
            transport,
            session: None,
            sent_datagrams: 0,
        }
    }

    pub fn stats(&self) -> SessionStats {
        SessionStats {
            pipeline: self.pipeline.stats(),
            sent_datagrams: self.sent_datagrams,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.session.is_some()
    }

    pub fn connect(&mut self, endpoint: Endpoint) -> Result<SessionId, SenderError> {
        self.transport
            .connect(endpoint)
            .map_err(SenderError::Transport)
            .map(|session| {
                self.session = Some(session);
                session
            })
    }

    pub fn pump(&mut self) -> Result<(), SenderError> {
        self.transport.pump().map_err(SenderError::Transport)
    }

    pub fn ingest_access_unit(
        &mut self,
        data: &[u8],
        is_keyframe: bool,
        pts_us: u64,
        stream_epoch: u32,
    ) -> Result<usize, SenderError> {
        self.pipeline
            .ingest_access_unit(data, is_keyframe, pts_us, stream_epoch)
    }

    /// Send all pending VideoPackets over QUIC datagrams.
    pub fn flush_pending(&mut self) -> Result<usize, SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let packets: Vec<VideoPacket> = self.pipeline.take_pending_packets();
        let mut sent = 0usize;
        for packet in packets {
            self.transport
                .send_video(session, packet)
                .map_err(SenderError::Transport)?;
            sent += 1;
        }
        self.transport.pump().map_err(SenderError::Transport)?;
        self.sent_datagrams += sent as u64;
        Ok(sent)
    }

    pub fn ingest_and_flush(
        &mut self,
        data: &[u8],
        is_keyframe: bool,
        pts_us: u64,
        stream_epoch: u32,
    ) -> Result<usize, SenderError> {
        self.ingest_access_unit(data, is_keyframe, pts_us, stream_epoch)?;
        self.flush_pending()
    }

    pub fn pending_packets(&self) -> usize {
        self.pipeline.pending_packets().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_testkit::MemoryTransport;

    #[test]
    fn memory_transport_flush_pending() {
        let mut session = SenderSession::new(MemoryTransport::new());
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 1,
            })
            .expect("connect");
        session
            .ingest_access_unit(b"au-bytes", true, 1, 1)
            .expect("ingest");
        let sent = session.flush_pending().expect("flush");
        assert_eq!(sent, 1);
        assert_eq!(session.stats().sent_datagrams, 1);
    }
}
