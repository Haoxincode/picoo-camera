#pragma once

// Picoo Camera virtual camera COM identifiers — REQ-PICOO-VCAM-002.

#include <initguid.h>

// {A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F}
DEFINE_GUID(CLSID_PicooVirtualCameraSource,
            0xa7c4e2f1, 0x8b3d, 0x4c6a, 0x9e, 0x5f, 0x1d, 0x2c, 0x3b, 0x4a, 0x5e, 0x6f);

#define PICOO_VCAM_FRIENDLY_NAME L"Picoo Camera"
#define PICOO_VCAM_DEFAULT_WIDTH 1280u
#define PICOO_VCAM_DEFAULT_HEIGHT 720u
#define PICOO_VCAM_FRAME_RATE_NUM 30u
#define PICOO_VCAM_FRAME_RATE_DEN 1u
#define PICOO_VCAM_SAMPLE_DURATION_100NS 333333u
