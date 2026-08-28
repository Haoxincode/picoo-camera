#pragma once

// Media Foundation / Kernel Streaming includes in dependency-safe order.
// Raise NTDDI so Frame Server / virtual-camera APIs are visible in mfidl.h.

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif

#ifndef NTDDI_VERSION
#define NTDDI_VERSION 0x0A00000C
#endif
#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0A00
#endif
#ifndef WINVER
#define WINVER 0x0A00
#endif

#include <sdkddkver.h>
#include <windows.h>

#include <mfapi.h>
#include <mfidl.h>
#include <mfobjects.h>
#include <mftransform.h>
// EVR first: IMFVideoSampleAllocator (Frame Server sample allocator) lives here.
#include <evr.h>
#include <mfvirtualcamera.h>
#include <ks.h>
#include <ksmedia.h>

#include <wrl/client.h>
#include <wrl/implements.h>

// Service GUID used by IMFGetService for sample allocators (mfidl.h on newer SDKs).
#ifndef MF_SAMPLEALLOCATOR_SERVICE
// {BBCD045D-4D8B-49E6-9D72-6C60C22A445B}
EXTERN_C const DECLSPEC_SELECTANY GUID MF_SAMPLEALLOCATOR_SERVICE =
    {0xbbcd045d, 0x4d8b, 0x49e6, {0x9d, 0x72, 0x6c, 0x60, 0xc2, 0x2a, 0x44, 0x5b}};
#endif

// Some CI SDK/header combinations expose Frame Server types incompletely.
// Provide a local IMFVideoSampleAllocator matching the public EVR contract so
// PicooVirtualCameraSource.dll always compiles (QI at runtime still uses the
// system IID).
#if !defined(__IMFVideoSampleAllocator_INTERFACE_DEFINED__) && !defined(PICOO_IMFVideoSampleAllocator_DEFINED)
#define PICOO_IMFVideoSampleAllocator_DEFINED
MIDL_INTERFACE("A792CDBA-A947-4651-AD6C-2EDEFDD97D91")
IMFVideoSampleAllocator : public IUnknown {
    virtual HRESULT STDMETHODCALLTYPE SetDirectXManager(_In_opt_ IUnknown* pManager) = 0;
    virtual HRESULT STDMETHODCALLTYPE UninitializeSampleAllocator() = 0;
    virtual HRESULT STDMETHODCALLTYPE InitializeSampleAllocator(
        _In_ DWORD cRequestedFrames,
        _In_ IMFMediaType* pMediaType) = 0;
    virtual HRESULT STDMETHODCALLTYPE AllocateSample(_COM_Outptr_ IMFSample** ppSample) = 0;
};
#endif
