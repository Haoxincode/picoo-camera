// PicooVirtualCameraSource.dll — REQ-PICOO-VCAM-002 scaffold.
//
// IMFMediaSource + Shared Frame Ring consumer lands in follow-up commits.
// This DLL proves the Windows CI build pipeline for the VCam extension.

#include <windows.h>

BOOL APIENTRY DllMain(HMODULE module, DWORD reason, LPVOID reserved) {
    (void)module;
    (void)reserved;
    switch (reason) {
    case DLL_PROCESS_ATTACH:
    case DLL_THREAD_ATTACH:
    case DLL_THREAD_DETACH:
    case DLL_PROCESS_DETACH:
        break;
    }
    return TRUE;
}

extern "C" __declspec(dllexport) const char* PicooVcamSourceVersion(void) {
    return "PicooVirtualCameraSource/0.1.0-scaffold";
}
