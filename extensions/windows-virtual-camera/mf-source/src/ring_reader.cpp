// Shared Frame Ring reader — Windows named mapping via shared_memory flink file.

#include "picoo_ring_reader.h"

#include "picoo_ring_layout.h"

#include <windows.h>

#include <atomic>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

namespace {

constexpr uint32_t kDefaultMaxFrameBytes = PICOO_RING_DEFAULT_MAX_FRAME_BYTES;

size_t RingLayoutSize(uint32_t max_frame_bytes) {
    return PICOO_RING_META_SIZE +
           static_cast<size_t>(PICOO_RING_SLOT_COUNT) *
               (PICOO_RING_SLOT_META_SIZE + max_frame_bytes);
}

size_t SlotOffset(uint32_t max_frame_bytes, size_t index) {
    return PICOO_RING_META_SIZE +
           index * (PICOO_RING_SLOT_META_SIZE + max_frame_bytes);
}

PicooRingMeta* MetaAt(uint8_t* base) {
    return reinterpret_cast<PicooRingMeta*>(base);
}

PicooRingSlotMeta* SlotMetaAt(uint8_t* base, uint32_t max_frame_bytes, size_t index) {
    return reinterpret_cast<PicooRingSlotMeta*>(base + SlotOffset(max_frame_bytes, index));
}

const uint8_t* SlotPixelsAt(const uint8_t* base, uint32_t max_frame_bytes, size_t index) {
    return base + SlotOffset(max_frame_bytes, index) + PICOO_RING_SLOT_META_SIZE;
}

std::wstring Utf8ToWide(const char* text) {
    if (text == nullptr || text[0] == '\0') {
        return L"";
    }
    const int needed =
        MultiByteToWideChar(CP_UTF8, 0, text, -1, nullptr, 0);
    if (needed <= 0) {
        return L"";
    }
    std::wstring wide(static_cast<size_t>(needed), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, text, -1, wide.data(), needed);
    if (!wide.empty() && wide.back() == L'\0') {
        wide.pop_back();
    }
    return wide;
}

bool BuildFlinkPath(const char* ring_name, std::wstring* out_path) {
    wchar_t temp_path[MAX_PATH + 1] = {};
    const DWORD temp_len = GetTempPathW(MAX_PATH, temp_path);
    if (temp_len == 0 || temp_len > MAX_PATH) {
        return false;
    }
    const std::wstring ring_wide = Utf8ToWide(ring_name);
    if (ring_wide.empty()) {
        return false;
    }
    wchar_t path[MAX_PATH + 1] = {};
    const int written = swprintf_s(
        path,
        L"%spicoo-frame-ring-%ls.link",
        temp_path,
        ring_wide.c_str());
    if (written <= 0) {
        return false;
    }
    *out_path = path;
    return true;
}

bool ReadMappingNameFromFlink(const std::wstring& flink_path, std::wstring* mapping_name) {
    HANDLE file = CreateFileW(
        flink_path.c_str(),
        GENERIC_READ,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        nullptr,
        OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL,
        nullptr);
    if (file == INVALID_HANDLE_VALUE) {
        return false;
    }

    char buffer[512] = {};
    DWORD read = 0;
    const BOOL ok = ReadFile(file, buffer, sizeof(buffer) - 1, &read, nullptr);
    CloseHandle(file);
    if (!ok || read == 0) {
        return false;
    }
    buffer[read] = '\0';

    // Trim trailing whitespace / nulls from shared_memory flink payload.
    size_t len = strcspn(buffer, "\r\n\0");
    if (len == 0) {
        return false;
    }
    buffer[len] = '\0';

    *mapping_name = Utf8ToWide(buffer);
    return !mapping_name->empty();
}

struct PicooRingReader {
    HANDLE mapping = nullptr;
    uint8_t* view = nullptr;
    uint32_t max_frame_bytes = 0;
    uint64_t last_sequence = 0;
};

bool ValidateHeader(PicooRingReader* reader) {
    if (reader == nullptr || reader->view == nullptr) {
        return false;
    }
    const PicooRingMeta* meta = MetaAt(reader->view);
    if (meta->magic != PICOO_RING_MAGIC || meta->version != PICOO_RING_VERSION) {
        return false;
    }
    if (meta->max_frame_bytes != reader->max_frame_bytes) {
        return false;
    }
    return true;
}

}  // namespace

extern "C" PicooRingReader* picoo_ring_reader_open(const char* ring_name, uint32_t max_frame_bytes) {
    if (ring_name == nullptr || ring_name[0] == '\0') {
        return nullptr;
    }
    if (max_frame_bytes == 0) {
        max_frame_bytes = kDefaultMaxFrameBytes;
    }

    std::wstring flink_path;
    if (!BuildFlinkPath(ring_name, &flink_path)) {
        return nullptr;
    }

    std::wstring mapping_name;
    if (!ReadMappingNameFromFlink(flink_path, &mapping_name)) {
        return nullptr;
    }

    HANDLE mapping = OpenFileMappingW(FILE_MAP_READ, FALSE, mapping_name.c_str());
    if (mapping == nullptr) {
        return nullptr;
    }

    const size_t map_size = RingLayoutSize(max_frame_bytes);
    uint8_t* view = static_cast<uint8_t*>(MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, map_size));
    if (view == nullptr) {
        CloseHandle(mapping);
        return nullptr;
    }

    PicooRingReader* reader = new PicooRingReader{};
    reader->mapping = mapping;
    reader->view = view;
    reader->max_frame_bytes = max_frame_bytes;

    if (!ValidateHeader(reader)) {
        picoo_ring_reader_close(reader);
        return nullptr;
    }

    return reader;
}

extern "C" void picoo_ring_reader_close(PicooRingReader* reader) {
    if (reader == nullptr) {
        return;
    }
    if (reader->view != nullptr) {
        UnmapViewOfFile(reader->view);
    }
    if (reader->mapping != nullptr) {
        CloseHandle(reader->mapping);
    }
    delete reader;
}

extern "C" int picoo_ring_reader_poll(PicooRingReader* reader, PicooRingFrameView* out_frame) {
    if (reader == nullptr || reader->view == nullptr || out_frame == nullptr) {
        return 0;
    }

    const PicooRingMeta* meta = MetaAt(reader->view);
    const auto* latest_sequence =
        reinterpret_cast<const std::atomic<uint64_t>*>(&meta->latest_sequence);
    const uint64_t target_sequence = latest_sequence->load(std::memory_order_acquire);
    if (target_sequence == 0 || target_sequence == reader->last_sequence) {
        return 0;
    }

    size_t best_index = 0;
    uint64_t best_sequence = 0;
    bool found = false;

    for (size_t i = 0; i < PICOO_RING_SLOT_COUNT; ++i) {
        const PicooRingSlotMeta* slot = SlotMetaAt(reader->view, reader->max_frame_bytes, i);
        const auto* ready_state =
            reinterpret_cast<const std::atomic<uint32_t>*>(&slot->ready_state);
        if (ready_state->load(std::memory_order_acquire) != PICOO_RING_READY_DONE) {
            continue;
        }
        const auto* sequence =
            reinterpret_cast<const std::atomic<uint64_t>*>(&slot->sequence);
        const uint64_t seq = sequence->load(std::memory_order_acquire);
        if (seq == 0) {
            continue;
        }
        if (!found || seq > best_sequence) {
            found = true;
            best_index = i;
            best_sequence = seq;
        }
    }

    if (!found) {
        return 0;
    }

    // Prefer exact latest sequence slot when available.
    for (size_t i = 0; i < PICOO_RING_SLOT_COUNT; ++i) {
        const PicooRingSlotMeta* slot = SlotMetaAt(reader->view, reader->max_frame_bytes, i);
        const auto* ready_state =
            reinterpret_cast<const std::atomic<uint32_t>*>(&slot->ready_state);
        const auto* sequence =
            reinterpret_cast<const std::atomic<uint64_t>*>(&slot->sequence);
        if (sequence->load(std::memory_order_acquire) == target_sequence &&
            ready_state->load(std::memory_order_acquire) == PICOO_RING_READY_DONE) {
            best_index = i;
            best_sequence = target_sequence;
            break;
        }
    }

    const PicooRingSlotMeta* slot = SlotMetaAt(reader->view, reader->max_frame_bytes, best_index);
    const uint8_t* pixels = SlotPixelsAt(reader->view, reader->max_frame_bytes, best_index);

    out_frame->sequence = best_sequence;
    out_frame->timestamp_us = slot->timestamp_us;
    out_frame->width = slot->width;
    out_frame->height = slot->height;
    out_frame->stride = slot->stride;
    out_frame->rotation = slot->rotation;
    out_frame->nv12 = pixels;
    out_frame->nv12_length = slot->data_length;
    reader->last_sequence = best_sequence;
    return 1;
}
