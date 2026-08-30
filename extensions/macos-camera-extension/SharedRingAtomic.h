#ifndef PICOO_SHARED_RING_ATOMIC_H
#define PICOO_SHARED_RING_ATOMIC_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct PicooRingFrameLease {
    uint64_t sequence;
    uint64_t timestamp_us;
    uint64_t pixel_offset;
    uint32_t slot_index;
    uint32_t width;
    uint32_t height;
    uint32_t stride;
    uint32_t rotation;
    uint32_t data_length;
    int32_t lock_descriptor;
} PicooRingFrameLease;

bool picoo_ring_validate_layout(void *base, size_t mapped_length);
bool picoo_ring_acquire_latest(const char *ring_path, void *base, size_t mapped_length,
                               PicooRingFrameLease *lease);
void picoo_ring_release(void *base, const PicooRingFrameLease *lease);

#endif
