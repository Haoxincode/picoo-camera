//! Native publication independent of consumer progress — REQ-PICOO-FRAME-012.

use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};

use crate::{FrameIdentity, NativeVideoFrame};

const ORDERED_CAPACITY: usize = 8;
const MAX_ORDERED_AGE: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionEnd {
    QueueFull(FrameIdentity),
    TooOld(FrameIdentity),
    Reset,
    PublisherStopped,
    Cancelled,
}

struct QueuedFrame {
    frame: Arc<NativeVideoFrame>,
    enqueued_at: Instant,
}

struct OrderedPublisher {
    sender: mpsc::SyncSender<QueuedFrame>,
    end: Arc<OnceLock<SubscriptionEnd>>,
}

/// The single supported processed-recording subscription. Preview never polls it.
/// A terminal error invalidates its pending frames and cannot look like success.
pub struct NativeFrameSubscription {
    receiver: Option<mpsc::Receiver<QueuedFrame>>,
    end: Arc<OnceLock<SubscriptionEnd>>,
}

impl NativeFrameSubscription {
    pub fn try_next(&mut self) -> Result<Option<Arc<NativeVideoFrame>>, SubscriptionEnd> {
        self.try_next_at(Instant::now())
    }

    fn try_next_at(
        &mut self,
        now: Instant,
    ) -> Result<Option<Arc<NativeVideoFrame>>, SubscriptionEnd> {
        if let Some(end) = self.end.get() {
            self.receiver = None;
            return Err(*end);
        }
        let Some(receiver) = &self.receiver else {
            return Err(SubscriptionEnd::PublisherStopped);
        };
        match receiver.try_recv() {
            Ok(queued) if now.saturating_duration_since(queued.enqueued_at) <= MAX_ORDERED_AGE => {
                // Cancellation can race the channel read. Downstream work
                // still carries its generation for the coordinator commit gate.
                if let Some(end) = self.end.get() {
                    self.receiver = None;
                    return Err(*end);
                }
                Ok(Some(queued.frame))
            }
            Ok(queued) => {
                let end = SubscriptionEnd::TooOld(queued.frame.identity());
                let _ = self.end.set(end);
                self.receiver = None;
                Err(*self.end.get().expect("subscription ended"))
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.receiver = None;
                Err(*self.end.get().unwrap_or(&SubscriptionEnd::PublisherStopped))
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("a processed-recording subscription is already active")]
pub struct SubscriptionAlreadyActive;

/// Receiver-owned native frame authority. Latest publication is capacity one;
/// the optional ordered consumer uses a bounded standard-library channel.
/// No consumer callback, GPU wait, CPU export, or disk I/O runs during publish.
#[derive(Default)]
pub struct FrameBus {
    latest: Option<Arc<NativeVideoFrame>>,
    ordered: Option<OrderedPublisher>,
}

impl FrameBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn latest(&self) -> Option<&Arc<NativeVideoFrame>> {
        self.latest.as_ref()
    }

    /// Future publications only; a recorder never starts from preview's cache.
    pub fn subscribe_ordered(
        &mut self,
    ) -> Result<NativeFrameSubscription, SubscriptionAlreadyActive> {
        if self
            .ordered
            .as_ref()
            .is_some_and(|ordered| ordered.end.get().is_none())
        {
            return Err(SubscriptionAlreadyActive);
        }
        let (sender, receiver) = mpsc::sync_channel(ORDERED_CAPACITY);
        let end = Arc::new(OnceLock::new());
        self.ordered = Some(OrderedPublisher {
            sender,
            end: Arc::clone(&end),
        });
        Ok(NativeFrameSubscription {
            receiver: Some(receiver),
            end,
        })
    }

    /// Overflow terminates only the ordered consumer, with the rejected identity
    /// returned to its coordinator. Latest preview and other outputs continue.
    pub fn publish(&mut self, frame: NativeVideoFrame) -> Option<SubscriptionEnd> {
        self.publish_at(frame, Instant::now())
    }

    fn publish_at(&mut self, frame: NativeVideoFrame, now: Instant) -> Option<SubscriptionEnd> {
        let frame = Arc::new(frame);
        self.latest = Some(Arc::clone(&frame));
        let ordered = self.ordered.as_ref()?;
        if let Some(end) = ordered.end.get().copied() {
            self.ordered = None;
            return Some(end);
        }
        match ordered.sender.try_send(QueuedFrame {
            frame,
            enqueued_at: now,
        }) {
            Ok(()) => None,
            Err(mpsc::TrySendError::Full(queued)) => {
                let end = SubscriptionEnd::QueueFull(queued.frame.identity());
                let _ = ordered.end.set(end);
                let end = *ordered.end.get().expect("subscription ended");
                self.ordered = None;
                Some(end)
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.ordered = None;
                None
            }
        }
    }

    /// Invalidates queued subscription output. Consumers discard their channel
    /// on the next poll; their already-held image leases remain their responsibility.
    pub fn clear(&mut self) {
        self.latest = None;
        self.end_ordered(SubscriptionEnd::Reset);
    }

    fn end_ordered(&mut self, reason: SubscriptionEnd) {
        if let Some(ordered) = self.ordered.take() {
            let _ = ordered.end.set(reason);
        }
    }
}

impl Drop for FrameBus {
    fn drop(&mut self) {
        self.end_ordered(SubscriptionEnd::PublisherStopped);
    }
}

impl Drop for NativeFrameSubscription {
    fn drop(&mut self) {
        let _ = self.end.set(SubscriptionEnd::Cancelled);
    }
}

#[cfg(test)]
mod tests;
