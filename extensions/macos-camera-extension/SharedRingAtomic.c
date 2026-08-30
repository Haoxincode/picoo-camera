#include "SharedRingAtomic.h"

#include <fcntl.h>
#include <stdio.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <unistd.h>

enum {
    PICOO_RING_MAGIC = 0x5049434F,
    PICOO_RING_VERSION = 2,
    PICOO_RING_META_SIZE = 64,
    PICOO_RING_SLOT_COUNT = 3,
    PICOO_RING_SLOT_META_SIZE = 64,
    PICOO_RING_READY_DONE = 2,
    PICOO_RING_PIXEL_FORMAT_NV12 = 1,
    PICOO_RING_WRITER_LEASE = UINT32_MAX,
};

typedef struct PicooRingMeta {
    uint32_t magic;
    uint32_t version;
    uint32_t slot_count;
    uint32_t max_frame_bytes;
    _Atomic uint32_t write_index;
    uint32_t alignment_padding;
    _Atomic uint64_t latest_sequence;
    uint8_t padding[32];
} PicooRingMeta;

typedef struct PicooSlotMeta {
    _Atomic uint64_t sequence;
    uint64_t timestamp_us;
    uint32_t width;
    uint32_t height;
    uint32_t stride;
    uint32_t rotation;
    uint32_t pixel_format;
    uint32_t data_length;
    _Atomic uint32_t ready_state;
    _Atomic uint32_t reader_count;
    uint8_t padding[16];
} PicooSlotMeta;

_Static_assert(sizeof(PicooRingMeta) == PICOO_RING_META_SIZE,
               "Picoo RingMeta layout drifted");
_Static_assert(sizeof(PicooSlotMeta) == PICOO_RING_SLOT_META_SIZE,
               "Picoo SlotMeta layout drifted");
_Static_assert(offsetof(PicooRingMeta, latest_sequence) == 24,
               "Picoo RingMeta sequence offset drifted");
_Static_assert(offsetof(PicooSlotMeta, ready_state) == 40,
               "Picoo SlotMeta ready offset drifted");
_Static_assert(offsetof(PicooSlotMeta, reader_count) == 44,
               "Picoo SlotMeta lease offset drifted");

static PicooSlotMeta *picoo_slot(void *base, uint32_t max_frame_bytes,
                                 uint32_t index) {
    size_t offset = PICOO_RING_META_SIZE +
                    (size_t)index * (PICOO_RING_SLOT_META_SIZE + max_frame_bytes);
    return (PicooSlotMeta *)((uint8_t *)base + offset);
}

static bool picoo_acquire_reader(PicooSlotMeta *slot) {
    uint32_t state = atomic_load_explicit(&slot->reader_count, memory_order_seq_cst);
    while (state < PICOO_RING_WRITER_LEASE - 1) {
        if (atomic_compare_exchange_weak_explicit(
                &slot->reader_count, &state, state + 1, memory_order_seq_cst,
                memory_order_seq_cst)) {
            return true;
        }
    }
    return false;
}

static int picoo_lock_slot(const char *ring_path, uint32_t index) {
    if (ring_path == NULL) {
        return -1;
    }
    size_t path_length = strlen(ring_path) + 32;
    char *lock_path = malloc(path_length);
    if (lock_path == NULL) {
        return -1;
    }
    int written = snprintf(lock_path, path_length, "%s.slot-%u.lock", ring_path, index);
    if (written < 0 || (size_t)written >= path_length) {
        free(lock_path);
        return -1;
    }
    int descriptor = open(lock_path, O_RDWR | O_CREAT | O_CLOEXEC, 0600);
    free(lock_path);
    if (descriptor < 0) {
        return -1;
    }
    if (flock(descriptor, LOCK_SH | LOCK_NB) != 0) {
        close(descriptor);
        return -1;
    }
    return descriptor;
}

static void picoo_unlock_slot(int descriptor) {
    if (descriptor >= 0) {
        flock(descriptor, LOCK_UN);
        close(descriptor);
    }
}

bool picoo_ring_validate_layout(void *base, size_t mapped_length) {
    if (base == NULL || mapped_length < PICOO_RING_META_SIZE) {
        return false;
    }

    PicooRingMeta *ring = (PicooRingMeta *)base;
    if (ring->magic != PICOO_RING_MAGIC || ring->version != PICOO_RING_VERSION ||
        ring->slot_count != PICOO_RING_SLOT_COUNT || ring->max_frame_bytes == 0) {
        return false;
    }
    size_t required_length = PICOO_RING_META_SIZE +
                             (size_t)ring->slot_count *
                                 (PICOO_RING_SLOT_META_SIZE + ring->max_frame_bytes);
    if (required_length != mapped_length) {
        return false;
    }
    return true;
}

bool picoo_ring_acquire_latest(const char *ring_path, void *base, size_t mapped_length,
                               PicooRingFrameLease *lease) {
    if (lease == NULL || !picoo_ring_validate_layout(base, mapped_length)) {
        return false;
    }

    PicooRingMeta *ring = (PicooRingMeta *)base;
    uint32_t candidate_indices[PICOO_RING_SLOT_COUNT];
    uint64_t candidate_sequences[PICOO_RING_SLOT_COUNT];
    uint32_t candidate_count = 0;
    for (uint32_t index = 0; index < ring->slot_count; index++) {
        PicooSlotMeta *slot = picoo_slot(base, ring->max_frame_bytes, index);
        if (atomic_load_explicit(&slot->ready_state, memory_order_acquire) !=
            PICOO_RING_READY_DONE) {
            continue;
        }
        uint64_t sequence = atomic_load_explicit(&slot->sequence, memory_order_acquire);
        if (sequence != 0) {
            candidate_indices[candidate_count] = index;
            candidate_sequences[candidate_count] = sequence;
            candidate_count++;
        }
    }
    for (uint32_t left = 0; left < candidate_count; left++) {
        for (uint32_t right = left + 1; right < candidate_count; right++) {
            if (candidate_sequences[right] > candidate_sequences[left]) {
                uint64_t sequence = candidate_sequences[left];
                candidate_sequences[left] = candidate_sequences[right];
                candidate_sequences[right] = sequence;
                uint32_t index = candidate_indices[left];
                candidate_indices[left] = candidate_indices[right];
                candidate_indices[right] = index;
            }
        }
    }

    for (uint32_t candidate = 0; candidate < candidate_count; candidate++) {
        uint32_t best_index = candidate_indices[candidate];
        uint64_t best_sequence = candidate_sequences[candidate];
        int lock_descriptor = picoo_lock_slot(ring_path, best_index);
        if (lock_descriptor < 0) {
            continue;
        }
        PicooSlotMeta *slot = picoo_slot(base, ring->max_frame_bytes, best_index);
        uint32_t lease_state = atomic_load_explicit(&slot->reader_count, memory_order_seq_cst);
        if (lease_state == PICOO_RING_WRITER_LEASE) {
            // A shared lock for this slot proves its writer is no longer alive.
            atomic_store_explicit(&slot->reader_count, 0, memory_order_seq_cst);
        }
        if (atomic_load_explicit(&slot->ready_state, memory_order_acquire) !=
                PICOO_RING_READY_DONE ||
            atomic_load_explicit(&slot->sequence, memory_order_acquire) != best_sequence ||
            !picoo_acquire_reader(slot)) {
            picoo_unlock_slot(lock_descriptor);
            continue;
        }
        if (atomic_load_explicit(&slot->ready_state, memory_order_acquire) !=
                PICOO_RING_READY_DONE ||
            atomic_load_explicit(&slot->sequence, memory_order_acquire) != best_sequence) {
            atomic_fetch_sub_explicit(&slot->reader_count, 1, memory_order_seq_cst);
            picoo_unlock_slot(lock_descriptor);
            continue;
        }

        size_t pixel_offset = PICOO_RING_META_SIZE +
                              (size_t)best_index *
                                  (PICOO_RING_SLOT_META_SIZE + ring->max_frame_bytes) +
                              PICOO_RING_SLOT_META_SIZE;
        if (slot->pixel_format != PICOO_RING_PIXEL_FORMAT_NV12 ||
            slot->data_length > ring->max_frame_bytes ||
            pixel_offset + slot->data_length > mapped_length) {
            atomic_fetch_sub_explicit(&slot->reader_count, 1, memory_order_seq_cst);
            picoo_unlock_slot(lock_descriptor);
            continue;
        }

        lease->sequence = best_sequence;
        lease->timestamp_us = slot->timestamp_us;
        lease->pixel_offset = pixel_offset;
        lease->slot_index = best_index;
        lease->width = slot->width;
        lease->height = slot->height;
        lease->stride = slot->stride;
        lease->rotation = slot->rotation;
        lease->data_length = slot->data_length;
        lease->lock_descriptor = lock_descriptor;
        return true;
    }
    return false;
}

void picoo_ring_release(void *base, const PicooRingFrameLease *lease) {
    if (base == NULL || lease == NULL) {
        return;
    }
    PicooRingMeta *ring = (PicooRingMeta *)base;
    if (ring->magic != PICOO_RING_MAGIC || lease->slot_index >= ring->slot_count) {
        picoo_unlock_slot(lease->lock_descriptor);
        return;
    }
    PicooSlotMeta *slot = picoo_slot(base, ring->max_frame_bytes, lease->slot_index);
    atomic_fetch_sub_explicit(&slot->reader_count, 1, memory_order_seq_cst);
    picoo_unlock_slot(lease->lock_descriptor);
}
