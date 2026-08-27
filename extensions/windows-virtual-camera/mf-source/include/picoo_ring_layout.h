#pragma once

// Shared Frame Ring layout — must match crates/picoo-frame-hub/src/shared_ring.rs (REQ-PICOO-FRAME-003).

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define PICOO_RING_MAGIC 0x5049434Fu
#define PICOO_RING_VERSION 1u
#define PICOO_RING_SLOT_COUNT 3u
#define PICOO_RING_META_SIZE 64u
#define PICOO_RING_SLOT_META_SIZE 64u
#define PICOO_RING_DEFAULT_MAX_FRAME_BYTES (1920u * 1080u * 3u / 2u)
#define PICOO_RING_PIXEL_FORMAT_NV12 1u

#define PICOO_RING_READY_EMPTY 0u
#define PICOO_RING_READY_WRITING 1u
#define PICOO_RING_READY_DONE 2u

typedef struct PicooRingMeta {
    uint32_t magic;
    uint32_t version;
    uint32_t slot_count;
    uint32_t max_frame_bytes;
    uint32_t write_index;
    uint32_t _pad_before_sequence;
    uint64_t latest_sequence;
    uint8_t pad[32];
} PicooRingMeta;

typedef struct PicooRingSlotMeta {
    uint64_t sequence;
    uint64_t timestamp_us;
    uint32_t width;
    uint32_t height;
    uint32_t stride;
    uint32_t rotation;
    uint32_t pixel_format;
    uint32_t data_length;
    uint32_t ready_state;
    uint8_t pad[4];
    uint8_t reserved[16];
} PicooRingSlotMeta;

#ifdef __cplusplus
}
#endif
