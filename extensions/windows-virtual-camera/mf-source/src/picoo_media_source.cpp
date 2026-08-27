// Picoo virtual camera media source — REQ-PICOO-VCAM-002.

#include "picoo_media_source.h"

#include "picoo_com_macros.h"
#include "picoo_media_stream.h"

#include <mferror.h>

using Microsoft::WRL::ComPtr;

PicooMediaSource::PicooMediaSource() = default;
PicooMediaSource::~PicooMediaSource() = default;

HRESULT PicooMediaSource::Initialize(IMFAttributes* source_attributes) {
    RETURN_IF_FAILED(MFCreateEventQueue(&queue_));
    source_attributes_ = source_attributes;

    stream_ = Microsoft::WRL::Make<PicooMediaStream>();
    RETURN_IF_FAILED(stream_->Initialize(this, 0));

    IMFStreamDescriptor* descriptors[1] = {nullptr};
    RETURN_IF_FAILED(stream_->GetStreamDescriptor(&descriptors[0]));
    RETURN_IF_FAILED(MFCreatePresentationDescriptor(1, descriptors, &presentation_));
    if (descriptors[0]) {
        descriptors[0]->Release();
    }

    // IMFPresentationDescriptor inherits IMFAttributes — set keys directly.
    RETURN_IF_FAILED(presentation_->SetUINT32(MF_PD_TOTAL_FILE_DURATION, 0));

    return S_OK;
}

IFACEMETHODIMP PicooMediaSource::BeginGetEvent(IMFAsyncCallback* callback, IUnknown* state) {
    if (shutdown_ || !queue_) {
        return MF_E_SHUTDOWN;
    }
    return queue_->BeginGetEvent(callback, state);
}

IFACEMETHODIMP PicooMediaSource::EndGetEvent(IMFAsyncResult* result, IMFMediaEvent** event) {
    if (shutdown_ || !queue_) {
        return MF_E_SHUTDOWN;
    }
    return queue_->EndGetEvent(result, event);
}

IFACEMETHODIMP PicooMediaSource::GetEvent(DWORD flags, IMFMediaEvent** event) {
    if (shutdown_ || !queue_) {
        return MF_E_SHUTDOWN;
    }
    return queue_->GetEvent(flags, event);
}

IFACEMETHODIMP PicooMediaSource::QueueEvent(MediaEventType type,
                                            REFGUID extended_type,
                                            HRESULT status,
                                            const PROPVARIANT* event_value) {
    if (shutdown_ || !queue_) {
        return MF_E_SHUTDOWN;
    }
    return queue_->QueueEventParamVar(type, extended_type, status, event_value);
}

IFACEMETHODIMP PicooMediaSource::GetCharacteristics(DWORD* characteristics) {
    if (characteristics == nullptr) {
        return E_POINTER;
    }
    *characteristics = MFMEDIASOURCE_IS_LIVE;
    return S_OK;
}

IFACEMETHODIMP PicooMediaSource::CreatePresentationDescriptor(IMFPresentationDescriptor** descriptor) {
    if (descriptor == nullptr) {
        return E_POINTER;
    }
    if (shutdown_ || !presentation_) {
        return MF_E_SHUTDOWN;
    }
    return presentation_.CopyTo(descriptor);
}

IFACEMETHODIMP PicooMediaSource::Start(IMFPresentationDescriptor* descriptor,
                                       const GUID* /*time_format*/,
                                       const PROPVARIANT* /*start_position*/) {
    if (shutdown_ || !stream_ || !queue_) {
        return MF_E_SHUTDOWN;
    }

    // Ensure stream 0 is selected on the caller's presentation descriptor (Frame Server).
    if (descriptor != nullptr) {
        BOOL selected = FALSE;
        ComPtr<IMFStreamDescriptor> selected_sd;
        RETURN_IF_FAILED(descriptor->GetStreamDescriptorByIndex(0, &selected, &selected_sd));
        if (!selected) {
            RETURN_IF_FAILED(descriptor->SelectStream(0));
        }
    }

    // MENewStream on first Start; MEUpdatedStream on subsequent Starts (REQ-PICOO-VCAM-002).
    if (!stream_presented_) {
        RETURN_IF_FAILED(queue_->QueueEventParamUnk(MENewStream, GUID_NULL, S_OK, stream_.Get()));
        stream_presented_ = true;
    } else {
        RETURN_IF_FAILED(
            queue_->QueueEventParamUnk(MEUpdatedStream, GUID_NULL, S_OK, stream_.Get()));
    }

    RETURN_IF_FAILED(stream_->SetStreamState(MF_STREAM_STATE_RUNNING));
    RETURN_IF_FAILED(queue_->QueueEventParamVar(MESourceStarted, GUID_NULL, S_OK, nullptr));
    return S_OK;
}

IFACEMETHODIMP PicooMediaSource::Stop() {
    if (shutdown_ || !stream_) {
        return MF_E_SHUTDOWN;
    }
    RETURN_IF_FAILED(stream_->SetStreamState(MF_STREAM_STATE_STOPPED));
    RETURN_IF_FAILED(queue_->QueueEventParamVar(MESourceStopped, GUID_NULL, S_OK, nullptr));
    return S_OK;
}

IFACEMETHODIMP PicooMediaSource::Pause() {
    return MF_E_INVALID_STATE_TRANSITION;
}

IFACEMETHODIMP PicooMediaSource::Shutdown() {
    if (shutdown_) {
        return S_OK;
    }
    shutdown_ = true;
    stream_presented_ = false;
    if (stream_) {
        stream_->Shutdown();
        stream_.Reset();
    }
    if (queue_) {
        queue_->Shutdown();
        queue_.Reset();
    }
    presentation_.Reset();
    source_attributes_.Reset();
    return S_OK;
}

IFACEMETHODIMP PicooMediaSource::GetSourceAttributes(IMFAttributes** attributes) {
    if (attributes == nullptr) {
        return E_POINTER;
    }
    if (!source_attributes_) {
        *attributes = nullptr;
        return MF_E_ATTRIBUTENOTFOUND;
    }
    return source_attributes_.CopyTo(attributes);
}

IFACEMETHODIMP PicooMediaSource::GetStreamAttributes(DWORD stream_id, IMFAttributes** attributes) {
    if (attributes == nullptr) {
        return E_POINTER;
    }
    if (stream_id != 0 || !stream_) {
        return MF_E_INVALIDSTREAMNUMBER;
    }
    ComPtr<IMFStreamDescriptor> descriptor;
    RETURN_IF_FAILED(stream_->GetStreamDescriptor(&descriptor));
    // Stream descriptor is already IMFAttributes.
    return descriptor->QueryInterface(IID_PPV_ARGS(attributes));
}

IFACEMETHODIMP PicooMediaSource::SetD3DManager(IUnknown* /*manager*/) {
    return S_OK;
}

IFACEMETHODIMP PicooMediaSource::GetService(REFGUID service, REFIID riid, LPVOID* object) {
    if (object == nullptr) {
        return E_POINTER;
    }
    *object = nullptr;
    if (InlineIsEqualGUID(service, MF_SAMPLEALLOCATOR_SERVICE)) {
        return stream_->QueryInterface(riid, object);
    }
    return MF_E_UNSUPPORTED_SERVICE;
}

IFACEMETHODIMP PicooMediaSource::SetDefaultAllocator(DWORD output_stream_id, IUnknown* allocator) {
    if (!stream_) {
        return MF_E_SHUTDOWN;
    }
    return stream_->SetDefaultAllocator(output_stream_id, allocator);
}

IFACEMETHODIMP PicooMediaSource::GetAllocatorUsage(DWORD output_stream_id,
                                                   DWORD* input_stream_id,
                                                   MFSampleAllocatorUsage* usage) {
    if (!stream_) {
        return MF_E_SHUTDOWN;
    }
    return stream_->GetAllocatorUsage(output_stream_id, input_stream_id, usage);
}
