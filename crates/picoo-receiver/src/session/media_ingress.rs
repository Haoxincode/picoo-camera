//! Receiver-local queue-age enforcement and video fragment ingress.

use std::collections::HashSet;
use std::time::Instant;

use picoo_packet::ReassemblyError;
use picoo_protocol::VideoPacket;
use picoo_transport::ReceivedVideoPacketBatch;

use super::recovery::RecoveryReason;
use super::ReceiverSession;
use crate::ReceiverError;

impl ReceiverSession {
    pub(super) fn discard_stale_video_batch(
        &mut self,
        packets: ReceivedVideoPacketBatch,
    ) -> Result<(), ReceiverError> {
        let mut terminated = HashSet::new();
        for packet in packets {
            let key = (packet.stream_epoch, packet.frame_id);
            if terminated.insert(key)
                && self.reassembly.discard_stale_access_unit(
                    packet.stream_epoch,
                    packet.frame_id,
                    packet.flags,
                )
            {
                self.ingress.receive_queue_expired_access_units = self
                    .ingress
                    .receive_queue_expired_access_units
                    .saturating_add(1);
            }
        }
        if self.reassembly.take_reference_chain_loss() {
            self.enter_decoder_recovery(RecoveryReason::ReferenceAccessUnitLate, true)?;
        }
        Ok(())
    }

    pub(super) fn ingest_video_packet(
        &mut self,
        packet: VideoPacket,
        received_at: Instant,
    ) -> Result<(), ReceiverError> {
        // Enforce the wall-clock deadline before a queued late tail gets a
        // chance to complete an already-expired AU.
        self.expire_reassembly_deadline()?;
        self.ingress.packets_received += 1;
        if !self.video_allowed() {
            self.ingress.packets_dropped_unpaired += 1;
            return Ok(());
        }

        let packet_epoch = packet.stream_epoch;
        let configured_epoch = self
            .current_stream_config
            .as_ref()
            .map(|config| config.stream_epoch);
        let (configured_epoch, mut defer_until_config) = match configured_epoch {
            Some(epoch) => (epoch, false),
            None if self.permit_unpaired_video => (packet_epoch, false),
            None => {
                match self.waiting_for_stream_config_epoch {
                    Some(waiting) if packet_epoch < waiting => return Ok(()),
                    Some(waiting) if packet_epoch == waiting => {}
                    Some(_) | None => {
                        self.pending_stream_config_idr = None;
                        self.waiting_for_stream_config_epoch = Some(packet_epoch);
                    }
                }
                (packet_epoch, true)
            }
        };
        if packet_epoch < configured_epoch {
            return Ok(());
        }
        if packet_epoch > configured_epoch {
            if self
                .waiting_for_stream_config_epoch
                .is_some_and(|waiting| packet_epoch < waiting)
            {
                return Ok(());
            }
            if self.waiting_for_stream_config_epoch != Some(packet_epoch) {
                self.pending_stream_config_idr = None;
                self.waiting_for_stream_config_epoch = Some(packet_epoch);
            }
            defer_until_config = true;
        }

        self.stats_reporter.record_packet(packet.payload.len());
        let recovered_before = self.reassembly.fec_recovered_fragment_count();
        let partial_drops_before = self.reassembly.partial_access_unit_drop_count();
        let gap_drops_before = self.reassembly.whole_access_unit_gap_drop_count();
        let reassembly_result = self.reassembly.ingest_at(packet, received_at);
        let recovered_now = self
            .reassembly
            .fec_recovered_fragment_count()
            .saturating_sub(recovered_before);
        self.ingress.fec_recovered_fragments = self
            .ingress
            .fec_recovered_fragments
            .saturating_add(recovered_now);
        self.ingress.reassembly_partial_access_unit_drops = self
            .ingress
            .reassembly_partial_access_unit_drops
            .saturating_add(
                self.reassembly
                    .partial_access_unit_drop_count()
                    .saturating_sub(partial_drops_before),
            );
        self.ingress.reassembly_whole_access_unit_gap_drops = self
            .ingress
            .reassembly_whole_access_unit_gap_drops
            .saturating_add(
                self.reassembly
                    .whole_access_unit_gap_drop_count()
                    .saturating_sub(gap_drops_before),
            );
        match reassembly_result {
            Ok(Some(access_unit)) => {
                if defer_until_config {
                    if access_unit.keyframe
                        && self.waiting_for_stream_config_epoch == Some(access_unit.stream_epoch)
                    {
                        self.pending_stream_config_idr = Some(access_unit);
                    }
                } else {
                    self.queue_assembled_access_unit(access_unit)?;
                }
            }
            Ok(None) => {}
            // Reassembly owns drop/keyframe-loss accounting. Keep protocol
            // rejects out of the decoder and continue the session.
            Err(ReassemblyError::TooManyFragments)
            | Err(ReassemblyError::DuplicateFragment)
            | Err(ReassemblyError::EpochMismatch)
            | Err(ReassemblyError::InconsistentFrameMetadata)
            | Err(ReassemblyError::InvalidFecParity) => {}
        }
        if self.reassembly.take_reference_chain_loss() && !defer_until_config {
            self.enter_decoder_recovery(RecoveryReason::ReferenceAccessUnitLost, true)?;
        }
        Ok(())
    }
}
