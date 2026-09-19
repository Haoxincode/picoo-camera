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
            || descriptor.format() != WindowsSharedSurfaceFormat::Bgra8
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
}
