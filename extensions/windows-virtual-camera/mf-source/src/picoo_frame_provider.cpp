// NV12 frame provider — polls Shared Frame Ring or emits branded placeholder.
// REQ-PICOO-FRAME-004

#include "picoo_frame_provider.h"

#include "picoo_ring_reader.h"
#include "picoo_vcam_ids.h"

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

// Minimal 5x7 glyphs — keep in sync with crates/picoo-frame-hub/src/placeholder.rs
const uint8_t* GlyphBits(char c) {
    static const uint8_t space[7] = {0, 0, 0, 0, 0, 0, 0};
    static const uint8_t period[7] = {0, 0, 0, 0, 0, 0x0C, 0x0C};
    static const uint8_t A[7] = {0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11};
    static const uint8_t C[7] = {0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E};
    static const uint8_t e[7] = {0, 0, 0x0E, 0x11, 0x1F, 0x10, 0x0E};
    static const uint8_t f[7] = {0x06, 0x09, 0x08, 0x1E, 0x08, 0x08, 0x08};
    static const uint8_t g[7] = {0, 0, 0x0F, 0x11, 0x0F, 0x01, 0x0E};
    static const uint8_t h[7] = {0x10, 0x10, 0x1E, 0x11, 0x11, 0x11, 0x11};
    static const uint8_t i[7] = {0x04, 0, 0x0C, 0x04, 0x04, 0x04, 0x0E};
    static const uint8_t m[7] = {0, 0, 0x1B, 0x15, 0x15, 0x15, 0x15};
    static const uint8_t n[7] = {0, 0, 0x1E, 0x11, 0x11, 0x11, 0x11};
    static const uint8_t o[7] = {0, 0, 0x0E, 0x11, 0x11, 0x11, 0x0E};
    static const uint8_t P[7] = {0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10};
    static const uint8_t r[7] = {0, 0, 0x16, 0x19, 0x10, 0x10, 0x10};
    static const uint8_t t[7] = {0x08, 0x08, 0x1E, 0x08, 0x08, 0x09, 0x06};
    static const uint8_t W[7] = {0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11};
    static const uint8_t y[7] = {0, 0, 0x11, 0x11, 0x0F, 0x01, 0x0E};
    static const uint8_t unknown[7] = {0x1F, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1F};

    switch (c) {
    case ' ':
        return space;
    case '.':
        return period;
    case 'A':
    case 'a':
        return A;
    case 'C':
    case 'c':
        return C;
    case 'e':
        return e;
    case 'f':
        return f;
    case 'g':
        return g;
    case 'h':
        return h;
    case 'i':
        return i;
    case 'm':
        return m;
    case 'n':
        return n;
    case 'o':
        return o;
    case 'P':
    case 'p':
        return P;
    case 'r':
        return r;
    case 't':
        return t;
    case 'W':
    case 'w':
        return W;
    case 'y':
        return y;
    default:
        return unknown;
    }
}

void BlitGlyph(uint8_t* y_plane,
               uint32_t width,
               char ch,
               uint32_t origin_x,
               uint32_t origin_y,
               uint32_t scale,
               uint8_t luma) {
    const uint8_t* bits = GlyphBits(ch);
    for (uint32_t row = 0; row < 7; ++row) {
        for (uint32_t col = 0; col < 5; ++col) {
            if (((bits[row] >> (4 - col)) & 1) == 0) {
                continue;
            }
            for (uint32_t dy = 0; dy < scale; ++dy) {
                for (uint32_t dx = 0; dx < scale; ++dx) {
                    const uint32_t x = origin_x + col * scale + dx;
                    const uint32_t y = origin_y + row * scale + dy;
                    if (x >= width) {
                        continue;
                    }
                    y_plane[y * width + x] = luma;
                }
            }
        }
    }
}

void BlitText(uint8_t* y_plane,
              uint32_t width,
              const char* text,
              uint32_t origin_x,
              uint32_t origin_y,
              uint32_t scale,
              uint8_t luma) {
    uint32_t cursor = origin_x;
    for (const char* p = text; *p != '\0'; ++p) {
        BlitGlyph(y_plane, width, *p, cursor, origin_y, scale, luma);
        cursor += 6 * scale;
    }
}

uint32_t TextPixelWidth(const char* text, uint32_t scale) {
    uint32_t count = 0;
    for (const char* p = text; *p != '\0'; ++p) {
        ++count;
    }
    return count * 6 * scale;
}

void DrawBrandedPlaceholder(uint8_t* nv12, uint32_t width, uint32_t height, uint32_t stride) {
    FillBlackNv12(nv12, width, height, stride);
    const uint32_t scale = (width >= 1280) ? 4u : 2u;
    const char* brand = "Picoo Camera";
    const char* waiting = "Waiting for phone...";
    const uint32_t brand_x = (width - TextPixelWidth(brand, scale)) / 2;
    const uint32_t wait_x = (width - TextPixelWidth(waiting, scale)) / 2;
    const uint32_t brand_y = height / 2 - 10 * scale;
    const uint32_t wait_y = height / 2 + 4 * scale;
    BlitText(nv12, stride, brand, brand_x, brand_y, scale, 220);
    BlitText(nv12, stride, waiting, wait_x, wait_y, scale, 180);
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
    DrawBrandedPlaceholder(scratch_.data(), width, height, stride);
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
