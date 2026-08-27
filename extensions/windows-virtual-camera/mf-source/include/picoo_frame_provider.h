#pragma once

// NV12 frame provider: Shared Frame Ring poll + last-frame hold + placeholder.
// REQ-PICOO-FRAME-004 / REQ-PICOO-FRAME-005 / PUC-004

#include "picoo_vcam_ids.h"

#include <chrono>
#include <cstdint>
#include <mutex>
#include <vector>

class PicooFrameProvider {
public:
    PicooFrameProvider();
    ~PicooFrameProvider();

    PicooFrameProvider(const PicooFrameProvider&) = delete;
    PicooFrameProvider& operator=(const PicooFrameProvider&) = delete;

    /// Copy latest NV12 into `out`, resizing internal buffer as needed.
    /// Returns true when dimensions are known (always true after first call).
    bool AcquireNv12(std::vector<uint8_t>* out, uint32_t* width, uint32_t* height, uint32_t* stride);

private:
    void EnsurePlaceholder(uint32_t width, uint32_t height);

    std::mutex mutex_;
    std::vector<uint8_t> scratch_;
    std::vector<uint8_t> last_live_;
    bool has_last_live_ = false;
    std::chrono::steady_clock::time_point last_live_at_{};
    uint32_t last_width_ = PICOO_VCAM_DEFAULT_WIDTH;
    uint32_t last_height_ = PICOO_VCAM_DEFAULT_HEIGHT;
    uint32_t last_stride_ = PICOO_VCAM_DEFAULT_WIDTH;

    /// Hold last live frame briefly before branded placeholder (PUC-004 reconnect).
    static constexpr std::chrono::milliseconds kLastFrameHold{500};
};
