use std::collections::{BTreeSet, VecDeque};

use bytes::Bytes;
use picoo_protocol::VideoPacket;
use picoo_transport::VideoDatagramBatch;

const DEFAULT_MAX_IN_FLIGHT: usize = 4_096;

/// Exact packet identity used by deterministic regression scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DatagramSelector {
    pub stream_epoch: u32,
    pub frame_id: u64,
    pub fragment_index: u16,
    pub fec_parity: bool,
}

#[derive(Debug, Clone)]
pub struct NetworkScript {
    /// Random loss in basis points, using a fixed seed.
    pub loss_basis_points: u16,
    pub seed: u64,
    pub base_delay_us: u64,
    pub jitter_us: u64,
    pub reverse_each_access_unit: bool,
    pub duplicate_every: Option<u64>,
    pub drop_datagrams: BTreeSet<DatagramSelector>,
    /// Drop this many consecutive eligible datagrams, starting at the ordinal.
    pub burst_drop: Option<(u64, u64)>,
    pub max_in_flight: usize,
}

impl Default for NetworkScript {
    fn default() -> Self {
        Self {
            loss_basis_points: 0,
            seed: 0x5049_434f_4f53_494d,
            base_delay_us: 1_000,
            jitter_us: 0,
            reverse_each_access_unit: false,
            duplicate_every: None,
            drop_datagrams: BTreeSet::new(),
            burst_drop: None,
            max_in_flight: DEFAULT_MAX_IN_FLIGHT,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SimDelivery {
    Control {
        connection_generation: u64,
        bytes: Bytes,
    },
    Video {
        connection_generation: u64,
        packet: VideoPacket,
    },
    Disconnected {
        connection_generation: u64,
    },
}

#[derive(Debug, Clone)]
struct ScheduledDelivery {
    due_us: u64,
    order: u64,
    delivery: SimDelivery,
}

/// Bounded deterministic network adapter for reliable control and lossy video.
#[derive(Debug)]
pub struct SimulatedNetwork {
    script: NetworkScript,
    rng: u64,
    ordinal: u64,
    next_order: u64,
    queue: VecDeque<ScheduledDelivery>,
    dropped: u64,
    overflow_dropped: u64,
}

impl SimulatedNetwork {
    pub fn new(script: NetworkScript) -> Self {
        Self {
            rng: script.seed,
            script,
            ordinal: 0,
            next_order: 0,
            queue: VecDeque::new(),
            dropped: 0,
            overflow_dropped: 0,
        }
    }

    pub fn script_mut(&mut self) -> &mut NetworkScript {
        &mut self.script
    }

    pub fn in_flight(&self) -> usize {
        self.queue.len()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn overflow_dropped(&self) -> u64 {
        self.overflow_dropped
    }

    pub fn send_control(
        &mut self,
        now_us: u64,
        connection_generation: u64,
        bytes: Bytes,
        extra_delay_us: u64,
        duplicate: bool,
    ) {
        let delivery = SimDelivery::Control {
            connection_generation,
            bytes,
        };
        self.schedule_control(
            now_us
                .saturating_add(self.script.base_delay_us)
                .saturating_add(extra_delay_us),
            delivery.clone(),
        );
        if duplicate {
            self.schedule_control(
                now_us
                    .saturating_add(self.script.base_delay_us)
                    .saturating_add(extra_delay_us)
                    .saturating_add(1),
                delivery,
            );
        }
    }

    pub fn send_video_batch(
        &mut self,
        now_us: u64,
        connection_generation: u64,
        batch: VideoDatagramBatch,
    ) {
        let mut packets = batch
            .into_datagrams()
            .into_iter()
            .filter_map(|bytes| VideoPacket::decode_bytes(bytes).ok())
            .collect::<Vec<_>>();
        if self.script.reverse_each_access_unit {
            packets.reverse();
        }
        for packet in packets {
            self.ordinal = self.ordinal.saturating_add(1);
            if self.should_drop(&packet) {
                self.dropped = self.dropped.saturating_add(1);
                continue;
            }
            let jitter = if self.script.jitter_us == 0 {
                0
            } else {
                self.next_random() % self.script.jitter_us.saturating_add(1)
            };
            let delivery = SimDelivery::Video {
                connection_generation,
                packet,
            };
            self.schedule(
                now_us
                    .saturating_add(self.script.base_delay_us)
                    .saturating_add(jitter),
                delivery.clone(),
            );
            if self
                .script
                .duplicate_every
                .is_some_and(|every| every > 0 && self.ordinal.is_multiple_of(every))
            {
                self.schedule(
                    now_us
                        .saturating_add(self.script.base_delay_us)
                        .saturating_add(jitter)
                        .saturating_add(1),
                    delivery,
                );
            }
        }
    }

    pub fn disconnect(&mut self, now_us: u64, connection_generation: u64) {
        self.schedule_control(
            now_us.saturating_add(self.script.base_delay_us),
            SimDelivery::Disconnected {
                connection_generation,
            },
        );
    }

    pub fn drain_ready(&mut self, now_us: u64) -> Vec<SimDelivery> {
        let mut ready = Vec::new();
        let mut pending = VecDeque::with_capacity(self.queue.len());
        while let Some(item) = self.queue.pop_front() {
            if item.due_us <= now_us {
                ready.push(item);
            } else {
                pending.push_back(item);
            }
        }
        self.queue = pending;
        ready.sort_by_key(|item| (item.due_us, item.order));
        ready.into_iter().map(|item| item.delivery).collect()
    }

    fn should_drop(&mut self, packet: &VideoPacket) -> bool {
        let selector = DatagramSelector {
            stream_epoch: packet.stream_epoch,
            frame_id: packet.frame_id,
            fragment_index: packet.fragment_index,
            fec_parity: packet
                .flags
                .contains(picoo_protocol::VideoPacketFlags::FEC_PARITY),
        };
        if self.script.drop_datagrams.contains(&selector) {
            return true;
        }
        if self.script.burst_drop.is_some_and(|(start, count)| {
            self.ordinal >= start && self.ordinal < start.saturating_add(count)
        }) {
            return true;
        }
        if self.script.loss_basis_points == 0 {
            return false;
        }
        self.next_random() % 10_000 < u64::from(self.script.loss_basis_points.min(10_000))
    }

    fn next_random(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        self.rng
    }

    fn schedule(&mut self, due_us: u64, delivery: SimDelivery) {
        if self.queue.len() >= self.script.max_in_flight.max(1) {
            self.overflow_dropped = self.overflow_dropped.saturating_add(1);
            return;
        }
        self.next_order = self.next_order.saturating_add(1);
        self.queue.push_back(ScheduledDelivery {
            due_us,
            order: self.next_order,
            delivery,
        });
    }

    fn schedule_control(&mut self, due_us: u64, delivery: SimDelivery) {
        if self.queue.len() >= self.script.max_in_flight.max(1) {
            if let Some(index) = self
                .queue
                .iter()
                .position(|item| matches!(item.delivery, SimDelivery::Video { .. }))
            {
                self.queue.remove(index);
                self.overflow_dropped = self.overflow_dropped.saturating_add(1);
            } else {
                self.overflow_dropped = self.overflow_dropped.saturating_add(1);
                return;
            }
        }
        self.next_order = self.next_order.saturating_add(1);
        self.queue.push_back(ScheduledDelivery {
            due_us,
            order: self.next_order,
            delivery,
        });
    }
}
