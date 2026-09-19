//! Cross-process native surface handoff facts.
//!
//! This module contains only the platform-neutral descriptor values that are
//! carried by a future authenticated Windows control channel. It deliberately
//! owns no HANDLE, COM object, or transport lifetime.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsAdapterId {
    low: u32,
    high: i32,
}

impl WindowsAdapterId {
    pub const fn from_luid(low: u32, high: i32) -> Self {
        Self { low, high }
    }

    pub const fn low(self) -> u32 {
        self.low
    }

    pub const fn high(self) -> i32 {
        self.high
    }
}

/// Identity carried with a native surface handoff. These values are facts from
/// the source/output owner, not a software protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsSharedSurfaceIdentity {
    pub source_connection_generation: u64,
    pub stream_epoch: u64,
    pub decoder_generation: u64,
    pub source_frame_id: u64,
    pub resource_generation: u64,
    pub backend_generation: u64,
    pub output_revision: u64,
}

impl WindowsSharedSurfaceIdentity {
    pub const fn is_valid(self) -> bool {
        // Decoder generation and frame/revision counters are allowed to start
        // at zero. Resource/backend generations are lifecycle guards and are
        // published only after their owning resource/backend exists.
        self.resource_generation != 0 && self.backend_generation != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsSharedSurfaceFormat {
    Bgra8,
    Nv12,
}

/// A target-process handle value plus the immutable facts required before an
/// importer may open it. The numeric handle is meaningful only in the target
/// process and is never opened by this value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsSharedSurfaceDescriptor {
    handle_value: u64,
    adapter: WindowsAdapterId,
    width: u32,
    height: u32,
    format: WindowsSharedSurfaceFormat,
    keyed_mutex_key: u64,
    identity: WindowsSharedSurfaceIdentity,
}

pub const WINDOWS_NATIVE_CHANNEL_MAX_IN_FLIGHT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsNativeChannelAck {
    Imported,
    Released,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsNativeOfferState {
    Offered,
    Imported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsNativeChannelError {
    InvalidGeneration,
    InvalidState,
    InvalidDescriptor,
    TooManyInFlight,
    UnknownOffer,
    InvalidAcknowledgement,
}

/// Bounded control messages carried by the Windows native pipe. The message
/// itself never owns a HANDLE; the descriptor's numeric value is valid only in
/// the receiving process after the producer duplicated it into that process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsNativeWireMessage {
    Hello {
        source_connection_generation: u64,
        stream_epoch: u64,
        adapter: WindowsAdapterId,
        resource_generation: u64,
        backend_generation: u64,
        output_revision: u64,
    },
    Ready {
        source_connection_generation: u64,
        stream_epoch: u64,
        resource_generation: u64,
        backend_generation: u64,
        output_revision: u64,
    },
    Offer {
        offer_id: u64,
        descriptor: WindowsSharedSurfaceDescriptor,
    },
    Ack {
        offer_id: u64,
        ack: WindowsNativeChannelAck,
    },
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsNativeWireError {
    Malformed,
    FrameTooLarge,
    InvalidAck,
    InvalidFormat,
}

impl WindowsNativeWireMessage {
    const HELLO: u8 = 1;
    const READY: u8 = 2;
    const OFFER: u8 = 3;
    const ACK: u8 = 4;
    const CLOSE: u8 = 5;
    const MAX_BYTES: usize = 256;

    pub fn encode(self) -> Result<Vec<u8>, WindowsNativeWireError> {
        let mut bytes = Vec::with_capacity(Self::MAX_BYTES);
        match self {
            Self::Hello {
                source_connection_generation,
                stream_epoch,
                adapter,
                resource_generation,
                backend_generation,
                output_revision,
            } => {
                bytes.push(Self::HELLO);
                put_u64(&mut bytes, source_connection_generation);
                put_u64(&mut bytes, stream_epoch);
                put_u32(&mut bytes, adapter.low());
                put_u32(&mut bytes, adapter.high() as u32);
                put_u64(&mut bytes, resource_generation);
                put_u64(&mut bytes, backend_generation);
                put_u64(&mut bytes, output_revision);
            }
            Self::Ready {
                source_connection_generation,
                stream_epoch,
                resource_generation,
                backend_generation,
                output_revision,
            } => {
                bytes.push(Self::READY);
                put_u64(&mut bytes, source_connection_generation);
                put_u64(&mut bytes, stream_epoch);
                put_u64(&mut bytes, resource_generation);
                put_u64(&mut bytes, backend_generation);
                put_u64(&mut bytes, output_revision);
            }
            Self::Offer {
                offer_id,
                descriptor,
            } => {
                bytes.push(Self::OFFER);
                put_u64(&mut bytes, offer_id);
                put_descriptor(&mut bytes, descriptor);
            }
            Self::Ack { offer_id, ack } => {
                bytes.push(Self::ACK);
                put_u64(&mut bytes, offer_id);
                bytes.push(match ack {
                    WindowsNativeChannelAck::Imported => 1,
                    WindowsNativeChannelAck::Released => 2,
                    WindowsNativeChannelAck::Rejected => 3,
                });
            }
            Self::Close => bytes.push(Self::CLOSE),
        }
        if bytes.len() > Self::MAX_BYTES {
            return Err(WindowsNativeWireError::FrameTooLarge);
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WindowsNativeWireError> {
        if bytes.is_empty() || bytes.len() > Self::MAX_BYTES {
            return Err(if bytes.len() > Self::MAX_BYTES {
                WindowsNativeWireError::FrameTooLarge
            } else {
                WindowsNativeWireError::Malformed
            });
        }
        let mut reader = WireReader { bytes, offset: 1 };
        match bytes[0] {
            Self::HELLO => {
                let message = Self::Hello {
                    source_connection_generation: reader.u64()?,
                    stream_epoch: reader.u64()?,
                    adapter: WindowsAdapterId::from_luid(reader.u32()?, reader.u32()? as i32),
                    resource_generation: reader.u64()?,
                    backend_generation: reader.u64()?,
                    output_revision: reader.u64()?,
                };
                reader.finish()?;
                Ok(message)
            }
            Self::READY => {
                let message = Self::Ready {
                    source_connection_generation: reader.u64()?,
                    stream_epoch: reader.u64()?,
                    resource_generation: reader.u64()?,
                    backend_generation: reader.u64()?,
                    output_revision: reader.u64()?,
                };
                reader.finish()?;
                Ok(message)
            }
            Self::OFFER => Ok(Self::Offer {
                offer_id: reader.u64()?,
                descriptor: read_descriptor(&mut reader)?,
            }),
            Self::ACK => {
                let offer_id = reader.u64()?;
                let ack = match reader.byte()? {
                    1 => WindowsNativeChannelAck::Imported,
                    2 => WindowsNativeChannelAck::Released,
                    3 => WindowsNativeChannelAck::Rejected,
                    _ => return Err(WindowsNativeWireError::InvalidAck),
                };
                reader.finish()?;
                Ok(Self::Ack { offer_id, ack })
            }
            Self::CLOSE => {
                reader.finish()?;
                Ok(Self::Close)
            }
            _ => Err(WindowsNativeWireError::Malformed),
        }
    }
}

fn put_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn put_descriptor(bytes: &mut Vec<u8>, descriptor: WindowsSharedSurfaceDescriptor) {
    put_u64(bytes, descriptor.handle_value());
    put_u32(bytes, descriptor.adapter().low());
    put_u32(bytes, descriptor.adapter().high() as u32);
    let (width, height) = descriptor.size();
    put_u32(bytes, width);
    put_u32(bytes, height);
    bytes.push(match descriptor.format() {
        WindowsSharedSurfaceFormat::Bgra8 => 1,
        WindowsSharedSurfaceFormat::Nv12 => 2,
    });
    put_u64(bytes, descriptor.keyed_mutex_key());
    let identity = descriptor.identity();
    put_u64(bytes, identity.source_connection_generation);
    put_u64(bytes, identity.stream_epoch);
    put_u64(bytes, identity.decoder_generation);
    put_u64(bytes, identity.source_frame_id);
    put_u64(bytes, identity.resource_generation);
    put_u64(bytes, identity.backend_generation);
    put_u64(bytes, identity.output_revision);
}

fn read_descriptor(
    reader: &mut WireReader<'_>,
) -> Result<WindowsSharedSurfaceDescriptor, WindowsNativeWireError> {
    let handle_value = reader.u64()?;
    let adapter = WindowsAdapterId::from_luid(reader.u32()?, reader.u32()? as i32);
    let width = reader.u32()?;
    let height = reader.u32()?;
    let format = match reader.byte()? {
        1 => WindowsSharedSurfaceFormat::Bgra8,
        2 => WindowsSharedSurfaceFormat::Nv12,
        _ => return Err(WindowsNativeWireError::InvalidFormat),
    };
    let keyed_mutex_key = reader.u64()?;
    let identity = WindowsSharedSurfaceIdentity {
        source_connection_generation: reader.u64()?,
        stream_epoch: reader.u64()?,
        decoder_generation: reader.u64()?,
        source_frame_id: reader.u64()?,
        resource_generation: reader.u64()?,
        backend_generation: reader.u64()?,
        output_revision: reader.u64()?,
    };
    reader.finish()?;
    WindowsSharedSurfaceDescriptor::new(
        handle_value,
        adapter,
        width,
        height,
        format,
        keyed_mutex_key,
        identity,
    )
    .ok_or(WindowsNativeWireError::Malformed)
}

struct WireReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> WireReader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], WindowsNativeWireError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(WindowsNativeWireError::Malformed)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(WindowsNativeWireError::Malformed)?;
        self.offset = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, WindowsNativeWireError> {
        Ok(*self.take(1)?.first().expect("take(1) returns one byte"))
    }

    fn u32(&mut self) -> Result<u32, WindowsNativeWireError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .expect("take(4) returns four bytes"),
        ))
    }

    fn u64(&mut self) -> Result<u64, WindowsNativeWireError> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .expect("take(8) returns eight bytes"),
        ))
    }

    fn finish(&self) -> Result<(), WindowsNativeWireError> {
        (self.offset == self.bytes.len())
            .then_some(())
            .ok_or(WindowsNativeWireError::Malformed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsNativeChannelCloseReport {
    offers: [(u64, WindowsNativeOfferState); WINDOWS_NATIVE_CHANNEL_MAX_IN_FLIGHT],
    len: usize,
}

impl WindowsNativeChannelCloseReport {
    pub fn outstanding(&self) -> &[(u64, WindowsNativeOfferState)] {
        &self.offers[..self.len]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowsNativeChannelState {
    Disconnected,
    AwaitingReady,
    Ready,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowsNativeOffer {
    id: u64,
    phase: WindowsNativeOfferState,
}

/// Typed state machine for the future authenticated local Windows control
/// channel. It carries only bounded control facts; the named pipe and HANDLE
/// duplication are platform adapters around this contract.
#[derive(Debug)]
pub struct WindowsNativeChannel {
    state: WindowsNativeChannelState,
    source_connection_generation: u64,
    stream_epoch: u64,
    adapter: WindowsAdapterId,
    resource_generation: u64,
    backend_generation: u64,
    output_revision: u64,
    next_offer_id: u64,
    in_flight: [Option<WindowsNativeOffer>; WINDOWS_NATIVE_CHANNEL_MAX_IN_FLIGHT],
}

impl WindowsNativeChannel {
    pub fn new(
        source_connection_generation: u64,
        stream_epoch: u64,
        adapter: WindowsAdapterId,
        resource_generation: u64,
        backend_generation: u64,
        output_revision: u64,
    ) -> Result<Self, WindowsNativeChannelError> {
        if source_connection_generation == 0 || resource_generation == 0 || backend_generation == 0
        {
            return Err(WindowsNativeChannelError::InvalidGeneration);
        }
        Ok(Self {
            state: WindowsNativeChannelState::Disconnected,
            source_connection_generation,
            stream_epoch,
            adapter,
            resource_generation,
            backend_generation,
            output_revision,
            next_offer_id: 1,
            in_flight: [None; WINDOWS_NATIVE_CHANNEL_MAX_IN_FLIGHT],
        })
    }

    pub fn begin_handshake(&mut self) -> Result<(), WindowsNativeChannelError> {
        if self.state != WindowsNativeChannelState::Disconnected {
            return Err(WindowsNativeChannelError::InvalidState);
        }
        self.state = WindowsNativeChannelState::AwaitingReady;
        Ok(())
    }

    pub fn accept_ready(
        &mut self,
        source_connection_generation: u64,
        stream_epoch: u64,
        resource_generation: u64,
        backend_generation: u64,
        output_revision: u64,
    ) -> Result<(), WindowsNativeChannelError> {
        if self.state != WindowsNativeChannelState::AwaitingReady {
            return Err(WindowsNativeChannelError::InvalidState);
        }
        if source_connection_generation != self.source_connection_generation
            || stream_epoch != self.stream_epoch
            || resource_generation != self.resource_generation
            || backend_generation != self.backend_generation
            || output_revision != self.output_revision
        {
            return Err(WindowsNativeChannelError::InvalidGeneration);
        }
        self.state = WindowsNativeChannelState::Ready;
        Ok(())
    }

    pub fn offer_frame(
        &mut self,
        descriptor: WindowsSharedSurfaceDescriptor,
    ) -> Result<u64, WindowsNativeChannelError> {
        if self.state != WindowsNativeChannelState::Ready {
            return Err(WindowsNativeChannelError::InvalidState);
        }
        let identity = descriptor.identity();
        if descriptor.adapter() != self.adapter
            || !matches!(
                descriptor.format(),
                WindowsSharedSurfaceFormat::Bgra8 | WindowsSharedSurfaceFormat::Nv12
            )
            || descriptor.keyed_mutex_key() != 0
            || !identity.is_valid()
            || identity.source_connection_generation != self.source_connection_generation
            || identity.stream_epoch != self.stream_epoch
            || identity.resource_generation != self.resource_generation
            || identity.backend_generation != self.backend_generation
            || identity.output_revision != self.output_revision
        {
            return Err(WindowsNativeChannelError::InvalidDescriptor);
        }
        let slot_index = self
            .in_flight
            .iter_mut()
            .position(|slot| slot.is_none())
            .ok_or(WindowsNativeChannelError::TooManyInFlight)?;
        let offer_id = self.next_offer_id;
        self.next_offer_id = self
            .next_offer_id
            .checked_add(1)
            .ok_or(WindowsNativeChannelError::InvalidGeneration)?;
        self.in_flight[slot_index] = Some(WindowsNativeOffer {
            id: offer_id,
            phase: WindowsNativeOfferState::Offered,
        });
        Ok(offer_id)
    }

    pub fn acknowledge(
        &mut self,
        offer_id: u64,
        ack: WindowsNativeChannelAck,
    ) -> Result<(), WindowsNativeChannelError> {
        if self.state != WindowsNativeChannelState::Ready {
            return Err(WindowsNativeChannelError::InvalidState);
        }
        let slot = self
            .in_flight
            .iter_mut()
            .find(|slot| slot.is_some_and(|offer| offer.id == offer_id))
            .ok_or(WindowsNativeChannelError::UnknownOffer)?;
        let offer = slot.as_mut().expect("offer matched above");
        match (offer.phase, ack) {
            (WindowsNativeOfferState::Offered, WindowsNativeChannelAck::Imported) => {
                offer.phase = WindowsNativeOfferState::Imported;
            }
            (WindowsNativeOfferState::Offered, WindowsNativeChannelAck::Rejected) => {
                *slot = None;
            }
            (WindowsNativeOfferState::Imported, WindowsNativeChannelAck::Released) => {
                *slot = None;
            }
            _ => return Err(WindowsNativeChannelError::InvalidAcknowledgement),
        }
        Ok(())
    }

    pub fn close(&mut self) -> WindowsNativeChannelCloseReport {
        let mut report = WindowsNativeChannelCloseReport {
            offers: [(0, WindowsNativeOfferState::Offered); WINDOWS_NATIVE_CHANNEL_MAX_IN_FLIGHT],
            len: 0,
        };
        for offer in self.in_flight.iter().flatten() {
            report.offers[report.len] = (offer.id, offer.phase);
            report.len += 1;
        }
        self.state = WindowsNativeChannelState::Closed;
        self.in_flight.fill(None);
        report
    }
}

impl WindowsSharedSurfaceDescriptor {
    pub const fn new(
        handle_value: u64,
        adapter: WindowsAdapterId,
        width: u32,
        height: u32,
        format: WindowsSharedSurfaceFormat,
        keyed_mutex_key: u64,
        identity: WindowsSharedSurfaceIdentity,
    ) -> Option<Self> {
        if handle_value == 0
            || width == 0
            || height == 0
            || keyed_mutex_key != 0
            || !identity.is_valid()
        {
            return None;
        }
        Some(Self {
            handle_value,
            adapter,
            width,
            height,
            format,
            keyed_mutex_key,
            identity,
        })
    }

    pub const fn handle_value(self) -> u64 {
        self.handle_value
    }

    pub const fn adapter(self) -> WindowsAdapterId {
        self.adapter
    }

    pub const fn size(self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub const fn format(self) -> WindowsSharedSurfaceFormat {
        self.format
    }

    pub const fn keyed_mutex_key(self) -> u64 {
        self.keyed_mutex_key
    }

    pub const fn identity(self) -> WindowsSharedSurfaceIdentity {
        self.identity
    }
}

#[cfg(test)]
mod tests {
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
    }
}
