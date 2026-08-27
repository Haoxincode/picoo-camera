#pragma once

// Media Foundation / Kernel Streaming includes in dependency-safe order.

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif

#include <windows.h>

#include <mfapi.h>
#include <mfidl.h>
#include <mfobjects.h>
#include <mftransform.h>
#include <mfvirtualcamera.h>
#include <evr.h>
#include <ks.h>
#include <ksmedia.h>

#include <wrl/client.h>
#include <wrl/implements.h>
