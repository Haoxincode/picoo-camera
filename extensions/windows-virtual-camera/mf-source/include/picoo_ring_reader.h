#pragma once

// Windows Shared Frame Ring consumer for VCam IMFMediaSource — REQ-PICOO-VCAM-002 / REQ-PICOO-FRAME-003.

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct PicooRingFrameView {
    uint64_t sequence;
    uint64_t timestamp_us;
    uint32_t width;
    uint32_t height;
    uint32_t stride;
    uint32_t rotation;
    const uint8_t* nv12;
    uint32_t nv12_length;
} PicooRingFrameView;

typedef struct PicooRingReader PicooRingReader;

/// Open ring by logical name (matches Rust DEFAULT_SHARED_RING_NAME / PICOO_RING_NAME).
/// Returns NULL on failure.
PicooRingReader* picoo_ring_reader_open(const char* ring_name, uint32_t max_frame_bytes);

void picoo_ring_reader_close(PicooRingReader* reader);

/// Returns 1 when a new frame is available since last poll, 0 otherwise.
int picoo_ring_reader_poll(PicooRingReader* reader, PicooRingFrameView* out_frame);

#ifdef __cplusplus
}
#endif
