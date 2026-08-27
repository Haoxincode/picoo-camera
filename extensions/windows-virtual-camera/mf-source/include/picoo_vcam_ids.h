#pragma once

// Picoo Camera virtual camera COM identifiers — REQ-PICOO-VCAM-002.
// Do NOT include <initguid.h> here: it breaks subsequent Windows SDK headers when
// this file is pulled into multiple translation units (cguid.h / __uuidof cascade).

#include <guiddef.h>

// {A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F}
EXTERN_C const GUID CLSID_PicooVirtualCameraSource;

#define PICOO_VCAM_FRIENDLY_NAME L"Picoo Camera"
#define PICOO_VCAM_DEFAULT_WIDTH 1280u
#define PICOO_VCAM_DEFAULT_HEIGHT 720u
#define PICOO_VCAM_FRAME_RATE_NUM 30u
#define PICOO_VCAM_FRAME_RATE_DEN 1u
#define PICOO_VCAM_SAMPLE_DURATION_100NS 333333u
