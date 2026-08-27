#pragma once

// Output NV12 size policy for MF virtual camera — REQ-PICOO-VCAM-002 / MEDIA-002.
// Header-only so Linux CI can unit-test without Media Foundation.

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Map an incoming frame size to a negotiated VCam profile (720p or 1080p).
/// Frames at/above 1920×1080 select 1080p; everything else selects 720p.
static inline void picoo_select_output_nv12_dims(uint32_t in_w,
                                                uint32_t in_h,
                                                uint32_t* out_w,
                                                uint32_t* out_h) {
    if (out_w == nullptr || out_h == nullptr) {
        return;
    }
    if (in_w >= 1920u && in_h >= 1080u) {
        *out_w = 1920u;
        *out_h = 1080u;
    } else {
        *out_w = 1280u;
        *out_h = 720u;
    }
}

/// True when the MF sample allocator / media type must be rebuilt for `frame_w`×`frame_h`.
static inline int picoo_output_dims_need_update(uint32_t current_w,
                                               uint32_t current_h,
                                               uint32_t frame_w,
                                               uint32_t frame_h) {
    uint32_t want_w = 0;
    uint32_t want_h = 0;
    picoo_select_output_nv12_dims(frame_w, frame_h, &want_w, &want_h);
    return (want_w != current_w || want_h != current_h) ? 1 : 0;
}

#ifdef __cplusplus
}  // extern "C"
#endif
