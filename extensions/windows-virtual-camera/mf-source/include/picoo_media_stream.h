#pragma once

#include "picoo_frame_provider.h"

#include <mfidl.h>
#include <wrl/implements.h>

class PicooMediaSource;

class PicooMediaStream
    : public Microsoft::WRL::RuntimeClass<
          Microsoft::WRL::RuntimeClassFlags<Microsoft::WRL::ClassicCom>,
          IMFMediaStream2,
          IMFMediaEventGenerator,
          IMFSampleAllocatorControl> {
public:
    PicooMediaStream();
    ~PicooMediaStream() override;

    HRESULT Initialize(PicooMediaSource* source, DWORD stream_id);

    // IMFMediaEventGenerator
    IFACEMETHOD(BeginGetEvent)(IMFAsyncCallback* callback, IUnknown* state) override;
    IFACEMETHOD(EndGetEvent)(IMFAsyncResult* result, IMFMediaEvent** event) override;
    IFACEMETHOD(GetEvent)(DWORD flags, IMFMediaEvent** event) override;
    IFACEMETHOD(QueueEvent)(MediaEventType type,
                            REFGUID extended_type,
                            HRESULT status,
                            const PROPVARIANT* value) override;

    // IMFMediaStream
    IFACEMETHOD(GetMediaSource)(IMFMediaSource** source) override;
    IFACEMETHOD(GetStreamDescriptor)(IMFStreamDescriptor** descriptor) override;
    IFACEMETHOD(RequestSample)(IUnknown* token) override;

    // IMFMediaStream2
    IFACEMETHOD(SetStreamState)(MF_STREAM_STATE state) override;
    IFACEMETHOD(GetStreamState)(MF_STREAM_STATE* state) override;

    // IMFSampleAllocatorControl
    IFACEMETHOD(SetDefaultAllocator)(DWORD output_stream_id, IUnknown* allocator) override;
    IFACEMETHOD(GetAllocatorUsage)(DWORD output_stream_id,
                                   DWORD* input_stream_id,
                                   MFSampleAllocatorUsage* usage) override;

    void Shutdown();

private:
    HRESULT EnsureStarted();
    HRESULT DeliverSample(IUnknown* token);
    HRESULT CreateManualSample(IMFSample** sample);

    PicooMediaSource* source_ = nullptr;
    DWORD stream_id_ = 0;
    MF_STREAM_STATE state_ = MF_STREAM_STATE_STOPPED;
    Microsoft::WRL::ComPtr<IMFMediaEventQueue> queue_;
    Microsoft::WRL::ComPtr<IMFStreamDescriptor> descriptor_;
    Microsoft::WRL::ComPtr<IMFMediaType> current_type_;
    Microsoft::WRL::ComPtr<IMFSampleAllocatorEx> allocator_;
    PicooFrameProvider frames_;
    GUID subtype_ = MFVideoFormat_NV12;
};
