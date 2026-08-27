// NV12 frame provider — polls Shared Frame Ring or emits black placeholder.

#include "picoo_frame_provider.h"

#include "picoo_ring_reader.h"
#include "picoo_vcam_ids.h"

#include <algorithm>
#include <cstring>

namespace {

constexpr const char* kDefaultRingName = "picoo-camera-v1";
PicooRingReader* g_ring_reader = nullptr;

PicooRingReader* SharedRingReader() {
    if (g_ring_reader == nullptr) {
        g_ring_reader = picoo_ring_reader_open(kDefaultRingName, 0);
    }
    return g_ring_reader;
}

void FillBlackNv12(uint8_t* dst, uint32_t width, uint32_t height, uint32_t stride) {
    const uint32_t y_size = stride * height;
    const uint32_t uv_height = height / 2;
    std::memset(dst, 0x00, y_size);
    std::memset(dst + y_size, 0x80, stride * uv_height);
}

}  // namespace

PicooFrameProvider::PicooFrameProvider() = default;

PicooFrameProvider::~PicooFrameProvider() {
    if (g_ring_reader != nullptr) {
        picoo_ring_reader_close(g_ring_reader);
        g_ring_reader = nullptr;
    }
}

void PicooFrameProvider::EnsurePlaceholder(uint32_t width, uint32_t height) {
    const uint32_t stride = width;
    const size_t needed = static_cast<size_t>(stride) * height * 3 / 2;
    scratch_.resize(needed);
    FillBlackNv12(scratch_.data(), width, height, stride);
    last_width_ = width;
    last_height_ = height;
    last_stride_ = stride;
}

bool PicooFrameProvider::AcquireNv12(std::vector<uint8_t>* out,
                                     uint32_t* width,
                                     uint32_t* height,
                                     uint32_t* stride) {
    if (out == nullptr || width == nullptr || height == nullptr || stride == nullptr) {
        return false;
    }

    std::lock_guard<std::mutex> lock(mutex_);

    PicooRingFrameView frame{};
    PicooRingReader* reader = SharedRingReader();
    if (reader != nullptr && picoo_ring_reader_poll(reader, &frame) && frame.nv12 != nullptr &&
        frame.nv12_length > 0 && frame.width > 0 && frame.height > 0) {
        out->assign(frame.nv12, frame.nv12 + frame.nv12_length);
        last_width_ = frame.width;
        last_height_ = frame.height;
        last_stride_ = frame.stride > 0 ? frame.stride : frame.width;
        *width = last_width_;
        *height = last_height_;
        *stride = last_stride_;
        return true;
    }

    EnsurePlaceholder(last_width_, last_height_);
    *out = scratch_;
    *width = last_width_;
    *height = last_height_;
    *stride = last_stride_;
    return true;
}
