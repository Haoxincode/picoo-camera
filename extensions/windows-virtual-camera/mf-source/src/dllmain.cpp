// PicooVirtualCameraSource.dll — REQ-PICOO-VCAM-002 scaffold.
//
// Ring reader validates cross-process NV12 consumption; IMFMediaSource follows.

#include <windows.h>

#include "picoo_ring_reader.h"

namespace {
const char* kDefaultRingName = "picoo-camera-v1";
PicooRingReader* g_ring_reader = nullptr;
}  // namespace

BOOL APIENTRY DllMain(HMODULE module, DWORD reason, LPVOID reserved) {
    (void)module;
    (void)reserved;
    switch (reason) {
    case DLL_PROCESS_ATTACH:
        break;
    case DLL_PROCESS_DETACH:
        if (g_ring_reader != nullptr) {
            picoo_ring_reader_close(g_ring_reader);
            g_ring_reader = nullptr;
        }
        break;
    default:
        break;
    }
    return TRUE;
}

extern "C" __declspec(dllexport) const char* PicooVcamSourceVersion(void) {
    return "PicooVirtualCameraSource/0.1.0-ring-reader";
}

/// Attach to Shared Frame Ring (lazy, idempotent). Returns 1 on success.
extern "C" __declspec(dllexport) int PicooVcamAttachRing(const char* ring_name) {
    if (g_ring_reader != nullptr) {
        return 1;
    }
    const char* name = (ring_name != nullptr && ring_name[0] != '\0') ? ring_name : kDefaultRingName;
    g_ring_reader = picoo_ring_reader_open(name, 0);
    return g_ring_reader != nullptr ? 1 : 0;
}

/// Poll latest NV12 frame. Returns 1 when a new frame is returned.
extern "C" __declspec(dllexport) int PicooVcamPollFrame(PicooRingFrameView* out_frame) {
    if (g_ring_reader == nullptr) {
        if (!PicooVcamAttachRing(nullptr)) {
            return 0;
        }
    }
    return picoo_ring_reader_poll(g_ring_reader, out_frame);
}
