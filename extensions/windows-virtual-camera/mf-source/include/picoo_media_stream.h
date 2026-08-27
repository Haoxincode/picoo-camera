#pragma once

#include "picoo_frame_provider.h"
#include "picoo_mf_headers.h"

#include <vector>

class PicooMediaSource;

class PicooMediaStream
    : public Microsoft::WRL::RuntimeClass<
          Microsoft::WRL::RuntimeClassFlags<Microsoft::WRL::ClassicCom>,
          IMFMediaStream2,
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
                            const PROPVARIANT* event_value) override;

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
    HRESULT CreateManualSample(const std::vector<uint8_t>& nv12,
                               uint32_t width,
                               uint32_t height,
                               IMFSample** sample);
    /// Follow midstream resolution (720p ↔ 1080p) so allocator buffers match NV12 size.
    HRESULT EnsureOutputFormat(uint32_t frame_w, uint32_t frame_h);

    PicooMediaSource* source_ = nullptr;
    DWORD stream_id_ = 0;
    MF_STREAM_STATE state_ = MF_STREAM_STATE_STOPPED;
    Microsoft::WRL::ComPtr<IMFMediaEventQueue> queue_;
    Microsoft::WRL::ComPtr<IMFStreamDescriptor> descriptor_;
    Microsoft::WRL::ComPtr<IMFMediaType> current_type_;
    Microsoft::WRL::ComPtr<IMFSampleAllocatorEx> allocator_;
    PicooFrameProvider frames_;
    uint32_t output_width_ = PICOO_VCAM_DEFAULT_WIDTH;
    uint32_t output_height_ = PICOO_VCAM_DEFAULT_HEIGHT;
};
