#pragma once

#include "picoo_media_stream.h"

#include <mfidl.h>
#include <wrl/implements.h>

class PicooMediaSource
    : public Microsoft::WRL::RuntimeClass<
          Microsoft::WRL::RuntimeClassFlags<Microsoft::WRL::ClassicCom>,
          IMFMediaSourceEx,
          IMFGetService,
          IMFSampleAllocatorControl> {
public:
    PicooMediaSource();
    ~PicooMediaSource() override;

    HRESULT Initialize(IMFAttributes* source_attributes);

    // IMFMediaEventGenerator
    IFACEMETHOD(BeginGetEvent)(IMFAsyncCallback* callback, IUnknown* state) override;
    IFACEMETHOD(EndGetEvent)(IMFAsyncResult* result, IMFMediaEvent** event) override;
    IFACEMETHOD(GetEvent)(DWORD flags, IMFMediaEvent** event) override;
    IFACEMETHOD(QueueEvent)(MediaEventType type,
                            REFGUID extended_type,
                            HRESULT status,
                            const PROPVARIANT* value) override;

    // IMFMediaSource
    IFACEMETHOD(GetCharacteristics)(DWORD* characteristics) override;
    IFACEMETHOD(CreatePresentationDescriptor)(IMFPresentationDescriptor** descriptor) override;
    IFACEMETHOD(Start)(IMFPresentationDescriptor* descriptor,
                       const GUID* time_format,
                       const PROPVARIANT* start_position) override;
    IFACEMETHOD(Stop)() override;
    IFACEMETHOD(Pause)() override;
    IFACEMETHOD(Shutdown)() override;

    // IMFMediaSourceEx
    IFACEMETHOD(GetSourceAttributes)(IMFAttributes** attributes) override;
    IFACEMETHOD(GetStreamAttributes)(DWORD stream_id, IMFAttributes** attributes) override;
    IFACEMETHOD(SetD3DManager)(IUnknown* manager) override;
    IFACEMETHOD(SetMediaType)(DWORD stream_id, IMFMediaType* media_type) override;

    // IMFGetService
    IFACEMETHOD(GetService)(REFGUID service, REFIID riid, LPVOID* object) override;

    // IMFSampleAllocatorControl
    IFACEMETHOD(SetDefaultAllocator)(DWORD output_stream_id, IUnknown* allocator) override;
    IFACEMETHOD(GetAllocatorUsage)(DWORD output_stream_id,
                                   DWORD* input_stream_id,
                                   MFSampleAllocatorUsage* usage) override;

private:
    Microsoft::WRL::ComPtr<IMFMediaEventQueue> queue_;
    Microsoft::WRL::ComPtr<IMFPresentationDescriptor> presentation_;
    Microsoft::WRL::ComPtr<IMFAttributes> source_attributes_;
    Microsoft::WRL::ComPtr<PicooMediaStream> stream_;
    bool shutdown_ = false;
};
