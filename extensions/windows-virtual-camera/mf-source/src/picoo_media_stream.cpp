// Picoo virtual camera media stream — REQ-PICOO-VCAM-002.

#include "picoo_media_stream.h"

#include "picoo_com_macros.h"
#include "picoo_media_source.h"
#include "picoo_vcam_ids.h"

#include <cstring>
#include <ksmedia.h>
#include <mferror.h>
#include <mfapi.h>
#include <mfidl.h>

namespace {

HRESULT CreateNv12MediaType(IMFMediaType** out_type, uint32_t width, uint32_t height) {
    Microsoft::WRL::ComPtr<IMFMediaType> media_type;
    RETURN_IF_FAILED(MFCreateMediaType(&media_type));
    RETURN_IF_FAILED(media_type->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Video));
    RETURN_IF_FAILED(media_type->SetGUID(MF_MT_SUBTYPE, MFVideoFormat_NV12));
    RETURN_IF_FAILED(media_type->SetUINT32(MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive));
    RETURN_IF_FAILED(media_type->SetUINT32(MF_MT_ALL_SAMPLES_INDEPENDENT, TRUE));
    RETURN_IF_FAILED(MFSetAttributeSize(media_type.Get(), MF_MT_FRAME_SIZE, width, height));
    RETURN_IF_FAILED(media_type->SetUINT32(MF_MT_DEFAULT_STRIDE, width));
    RETURN_IF_FAILED(MFSetAttributeRatio(media_type.Get(), MF_MT_FRAME_RATE, PICOO_VCAM_FRAME_RATE_NUM,
                                         PICOO_VCAM_FRAME_RATE_DEN));
    RETURN_IF_FAILED(MFSetAttributeRatio(media_type.Get(), MF_MT_PIXEL_ASPECT_RATIO, 1, 1));
    *out_type = media_type.Detach();
    return S_OK;
}

}  // namespace

using Microsoft::WRL::ComPtr;

PicooMediaStream::PicooMediaStream() = default;
PicooMediaStream::~PicooMediaStream() = default;

HRESULT PicooMediaStream::Initialize(PicooMediaSource* source, DWORD stream_id) {
    if (source == nullptr) {
        return E_POINTER;
    }
    source_ = source;
    stream_id_ = stream_id;

    RETURN_IF_FAILED(MFCreateEventQueue(&queue_));

    ComPtr<IMFMediaType> media_type;
    RETURN_IF_FAILED(CreateNv12MediaType(&media_type, PICOO_VCAM_DEFAULT_WIDTH, PICOO_VCAM_DEFAULT_HEIGHT));
    current_type_ = media_type;

    IMFMediaType* types[1] = {media_type.Get()};
    RETURN_IF_FAILED(MFCreateStreamDescriptor(stream_id, 1, types, &descriptor_));

    ComPtr<IMFMediaTypeHandler> handler;
    RETURN_IF_FAILED(descriptor_->GetMediaTypeHandler(&handler));
    RETURN_IF_FAILED(handler->SetCurrentMediaType(media_type.Get()));

    ComPtr<IMFAttributes> attrs;
    RETURN_IF_FAILED(descriptor_->GetAttributes(&attrs));
    RETURN_IF_FAILED(attrs->SetGUID(MF_DEVICESTREAM_STREAM_CATEGORY, PINNAME_VIDEO_CAPTURE));
    RETURN_IF_FAILED(attrs->SetUINT32(MF_DEVICESTREAM_STREAM_ID, stream_id));
    RETURN_IF_FAILED(attrs->SetUINT32(MF_DEVICESTREAM_FRAMESERVER_SHARED, 1));
    RETURN_IF_FAILED(
        attrs->SetUINT32(MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES, MFFrameSourceTypes_Color));

    return S_OK;
}

void PicooMediaStream::Shutdown() {
    if (queue_) {
        queue_->Shutdown();
        queue_.Reset();
    }
    descriptor_.Reset();
    allocator_.Reset();
    current_type_.Reset();
    source_ = nullptr;
}

IFACEMETHODIMP PicooMediaStream::BeginGetEvent(IMFAsyncCallback* callback, IUnknown* state) {
    if (!queue_) {
        return MF_E_SHUTDOWN;
    }
    return queue_->BeginGetEvent(callback, state);
}

IFACEMETHODIMP PicooMediaStream::EndGetEvent(IMFAsyncResult* result, IMFMediaEvent** event) {
    if (!queue_) {
        return MF_E_SHUTDOWN;
    }
    return queue_->EndGetEvent(result, event);
}

IFACEMETHODIMP PicooMediaStream::GetEvent(DWORD flags, IMFMediaEvent** event) {
    if (!queue_) {
        return MF_E_SHUTDOWN;
    }
    return queue_->GetEvent(flags, event);
}

IFACEMETHODIMP PicooMediaStream::QueueEvent(MediaEventType type,
                                            REFGUID extended_type,
                                            HRESULT status,
                                            const PROPVARIANT* value) {
    if (!queue_) {
        return MF_E_SHUTDOWN;
    }
    return queue_->QueueEventParamVar(type, extended_type, status, value);
}

IFACEMETHODIMP PicooMediaStream::GetMediaSource(IMFMediaSource** source) {
    if (!source_) {
        return MF_E_SHUTDOWN;
    }
    return source_->QueryInterface(IID_PPV_ARGS(source));
}

IFACEMETHODIMP PicooMediaStream::GetStreamDescriptor(IMFStreamDescriptor** descriptor) {
    if (!descriptor_) {
        return MF_E_SHUTDOWN;
    }
    return descriptor_.CopyTo(descriptor);
}

IFACEMETHODIMP PicooMediaStream::RequestSample(IUnknown* token) {
    if (!queue_) {
        return MF_E_SHUTDOWN;
    }
    return DeliverSample(token);
}

IFACEMETHODIMP PicooMediaStream::SetStreamState(MF_STREAM_STATE state) {
    if (state == state_) {
        return S_OK;
    }
    switch (state) {
    case MF_STREAM_STATE_RUNNING:
        RETURN_IF_FAILED(EnsureStarted());
        break;
    case MF_STREAM_STATE_STOPPED:
        state_ = MF_STREAM_STATE_STOPPED;
        if (allocator_) {
            allocator_->UninitializeSampleAllocator();
        }
        RETURN_IF_FAILED(queue_->QueueEventParamVar(MEStreamStopped, GUID_NULL, S_OK, nullptr));
        break;
    default:
        return MF_E_INVALID_STATE_TRANSITION;
    }
    return S_OK;
}

IFACEMETHODIMP PicooMediaStream::GetStreamState(MF_STREAM_STATE* state) {
    if (state == nullptr) {
        return E_POINTER;
    }
    *state = state_;
    return S_OK;
}

IFACEMETHODIMP PicooMediaStream::SetDefaultAllocator(DWORD output_stream_id, IUnknown* allocator) {
    if (output_stream_id != stream_id_) {
        return MF_E_INVALIDSTREAMNUMBER;
    }
    allocator_.Reset();
    if (allocator == nullptr) {
        return E_POINTER;
    }
    return allocator->QueryInterface(IID_PPV_ARGS(&allocator_));
}

IFACEMETHODIMP PicooMediaStream::GetAllocatorUsage(DWORD output_stream_id,
                                                   DWORD* input_stream_id,
                                                   MFSampleAllocatorUsage* usage) {
    if (output_stream_id != stream_id_ || usage == nullptr) {
        return E_POINTER;
    }
    if (input_stream_id) {
        *input_stream_id = stream_id_;
    }
    *usage = MFSampleAllocatorUsage_UsesProvidedAllocator;
    return S_OK;
}

HRESULT PicooMediaStream::EnsureStarted() {
    if (state_ == MF_STREAM_STATE_RUNNING) {
        return S_OK;
    }
    if (allocator_) {
        RETURN_IF_FAILED(allocator_->InitializeSampleAllocator(10, current_type_.Get()));
    }
    state_ = MF_STREAM_STATE_RUNNING;
    RETURN_IF_FAILED(queue_->QueueEventParamVar(MEStreamStarted, GUID_NULL, S_OK, nullptr));
    return S_OK;
}

HRESULT PicooMediaStream::CreateManualSample(IMFSample** sample) {
    std::vector<uint8_t> nv12;
    uint32_t width = 0;
    uint32_t height = 0;
    uint32_t stride = 0;
    if (!frames_.AcquireNv12(&nv12, &width, &height, &stride)) {
        return E_FAIL;
    }

    ComPtr<IMFSample> out_sample;
    ComPtr<IMFMediaBuffer> buffer;
    RETURN_IF_FAILED(MFCreateSample(&out_sample));
    RETURN_IF_FAILED(MFCreateMemoryBuffer(static_cast<DWORD>(nv12.size()), &buffer));

    BYTE* dst = nullptr;
    DWORD max_length = 0;
    RETURN_IF_FAILED(buffer->Lock(&dst, &max_length, nullptr));
    if (max_length < nv12.size()) {
        buffer->Unlock();
        return E_FAIL;
    }
    std::memcpy(dst, nv12.data(), nv12.size());
    buffer->Unlock();
    RETURN_IF_FAILED(buffer->SetCurrentLength(static_cast<DWORD>(nv12.size())));

    RETURN_IF_FAILED(out_sample->AddBuffer(buffer.Get()));
    RETURN_IF_FAILED(out_sample->SetSampleTime(MFGetSystemTime()));
    RETURN_IF_FAILED(out_sample->SetSampleDuration(PICOO_VCAM_SAMPLE_DURATION_100NS));

    *sample = out_sample.Detach();
    return S_OK;
}

HRESULT PicooMediaStream::DeliverSample(IUnknown* token) {
    ComPtr<IMFSample> sample;
    if (allocator_) {
        RETURN_IF_FAILED(allocator_->AllocateSample(&sample));
    } else {
        RETURN_IF_FAILED(CreateManualSample(&sample));
    }

    if (allocator_) {
        std::vector<uint8_t> nv12;
        uint32_t width = 0;
        uint32_t height = 0;
        uint32_t stride = 0;
        if (!frames_.AcquireNv12(&nv12, &width, &height, &stride)) {
            return E_FAIL;
        }

        DWORD buffer_count = 0;
        RETURN_IF_FAILED(sample->GetBufferCount(&buffer_count));
        if (buffer_count == 0) {
            return E_FAIL;
        }
        ComPtr<IMFMediaBuffer> buffer;
        RETURN_IF_FAILED(sample->GetBufferByIndex(0, &buffer));
        BYTE* dst = nullptr;
        DWORD max_length = 0;
        RETURN_IF_FAILED(buffer->Lock(&dst, &max_length, nullptr));
        if (max_length < nv12.size()) {
            buffer->Unlock();
            return E_FAIL;
        }
        std::memcpy(dst, nv12.data(), nv12.size());
        buffer->Unlock();
        RETURN_IF_FAILED(buffer->SetCurrentLength(static_cast<DWORD>(nv12.size())));
        RETURN_IF_FAILED(sample->SetSampleTime(MFGetSystemTime()));
        RETURN_IF_FAILED(sample->SetSampleDuration(PICOO_VCAM_SAMPLE_DURATION_100NS));
    }

    if (token) {
        RETURN_IF_FAILED(sample->SetUnknown(MFSampleExtension_Token, token));
    }
    RETURN_IF_FAILED(queue_->QueueEventParamUnk(MEMediaSample, GUID_NULL, S_OK, sample.Get()));
    return S_OK;
}
