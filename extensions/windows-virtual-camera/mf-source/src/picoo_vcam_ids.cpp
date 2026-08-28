// CLSID definition for PicooVirtualCameraSource — single TU with INITGUID.

#include <initguid.h>

#include "picoo_vcam_ids.h"

// {A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F}
DEFINE_GUID(CLSID_PicooVirtualCameraSource,
            0xa7c4e2f1, 0x8b3d, 0x4c6a, 0x9e, 0x5f, 0x1d, 0x2c, 0x3b, 0x4a, 0x5e, 0x6f);

// REQ-PICOO-VCAM-001: keep UTF-16 friendly name in PE .rdata for CI bundle smoke
// (verify_windows_bundle.ps1 scans the DLL; do not rely on DllRegisterServer locals).
extern "C" const wchar_t kPicooVcamFriendlyNameEmbedded[] = PICOO_VCAM_FRIENDLY_NAME;

extern "C" __declspec(dllexport) const wchar_t* PicooVcamFriendlyName(void) {
    return kPicooVcamFriendlyNameEmbedded;
}
