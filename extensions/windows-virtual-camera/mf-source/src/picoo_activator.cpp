// Picoo virtual camera IMFActivate — REQ-PICOO-VCAM-002.

#include "picoo_activator.h"

#include "picoo_com_macros.h"
#include "picoo_media_source.h"
#include "picoo_vcam_ids.h"

#include <mfapi.h>
#include <mferror.h>

using Microsoft::WRL::ComPtr;

PicooActivator::PicooActivator() = default;
PicooActivator::~PicooActivator() = default;

HRESULT PicooActivator::Initialize() {
    RETURN_IF_FAILED(MFCreateAttributes(&attributes_, 3));
    RETURN_IF_FAILED(attributes_->SetUINT32(MF_VIRTUALCAMERA_PROVIDE_ASSOCIATED_CAMERA_SOURCES, 1));
    RETURN_IF_FAILED(attributes_->SetGUID(MFT_TRANSFORM_CLSID_Attribute, CLSID_PicooVirtualCameraSource));

    source_ = Microsoft::WRL::Make<PicooMediaSource>();
    RETURN_IF_FAILED(source_->Initialize(attributes_.Get()));
    return S_OK;
}

IFACEMETHODIMP PicooActivator::ActivateObject(REFIID riid, void** object) {
    if (object == nullptr) {
        return E_POINTER;
    }
    *object = nullptr;
    if (!source_) {
        return MF_E_SHUTDOWN;
    }
    return source_->QueryInterface(riid, object);
}

IFACEMETHODIMP PicooActivator::ShutdownObject() {
    if (source_) {
        source_->Shutdown();
    }
    return S_OK;
}

IFACEMETHODIMP PicooActivator::DetachObject() {
    source_.Reset();
    return S_OK;
}

#define DELEGATE_ATTR(method, ...) \
    if (!attributes_) { \
        return MF_E_SHUTDOWN; \
    } \
    return attributes_->method(__VA_ARGS__)

IFACEMETHODIMP PicooActivator::GetItem(REFGUID key, PROPVARIANT* value) {
    DELEGATE_ATTR(GetItem, key, value);
}
IFACEMETHODIMP PicooActivator::GetItemType(REFGUID key, MF_ATTRIBUTE_TYPE* type) {
    DELEGATE_ATTR(GetItemType, key, type);
}
IFACEMETHODIMP PicooActivator::CompareItem(REFGUID key, REFPROPVARIANT value, BOOL* result) {
    DELEGATE_ATTR(CompareItem, key, value, result);
}
IFACEMETHODIMP PicooActivator::Compare(IMFAttributes* other, MF_ATTRIBUTES_MATCH_TYPE match, BOOL* result) {
    DELEGATE_ATTR(Compare, other, match, result);
}
IFACEMETHODIMP PicooActivator::GetUINT32(REFGUID key, UINT32* value) {
    DELEGATE_ATTR(GetUINT32, key, value);
}
IFACEMETHODIMP PicooActivator::GetUINT64(REFGUID key, UINT64* value) {
    DELEGATE_ATTR(GetUINT64, key, value);
}
IFACEMETHODIMP PicooActivator::GetDouble(REFGUID key, double* value) {
    DELEGATE_ATTR(GetDouble, key, value);
}
IFACEMETHODIMP PicooActivator::GetGUID(REFGUID key, GUID* value) {
    DELEGATE_ATTR(GetGUID, key, value);
}
IFACEMETHODIMP PicooActivator::GetStringLength(REFGUID key, UINT32* length) {
    DELEGATE_ATTR(GetStringLength, key, length);
}
IFACEMETHODIMP PicooActivator::GetString(REFGUID key, LPWSTR value, UINT32 size, UINT32* length) {
    DELEGATE_ATTR(GetString, key, value, size, length);
}
IFACEMETHODIMP PicooActivator::GetAllocatedString(REFGUID key, LPWSTR* value, UINT32* length) {
    DELEGATE_ATTR(GetAllocatedString, key, value, length);
}
IFACEMETHODIMP PicooActivator::GetBlobSize(REFGUID key, UINT32* size) {
    DELEGATE_ATTR(GetBlobSize, key, size);
}
IFACEMETHODIMP PicooActivator::GetBlob(REFGUID key, UINT8* blob, UINT32 size, UINT32* length) {
    DELEGATE_ATTR(GetBlob, key, blob, size, length);
}
IFACEMETHODIMP PicooActivator::GetAllocatedBlob(REFGUID key, UINT8** blob, UINT32* size) {
    DELEGATE_ATTR(GetAllocatedBlob, key, blob, size);
}
IFACEMETHODIMP PicooActivator::GetUnknown(REFGUID key, REFIID riid, LPVOID* value) {
    DELEGATE_ATTR(GetUnknown, key, riid, value);
}
IFACEMETHODIMP PicooActivator::SetItem(REFGUID key, REFPROPVARIANT value) {
    DELEGATE_ATTR(SetItem, key, value);
}
IFACEMETHODIMP PicooActivator::DeleteItem(REFGUID key) {
    DELEGATE_ATTR(DeleteItem, key);
}
IFACEMETHODIMP PicooActivator::DeleteAllItems() {
    DELEGATE_ATTR(DeleteAllItems);
}
IFACEMETHODIMP PicooActivator::SetUINT32(REFGUID key, UINT32 value) {
    DELEGATE_ATTR(SetUINT32, key, value);
}
IFACEMETHODIMP PicooActivator::SetUINT64(REFGUID key, UINT64 value) {
    DELEGATE_ATTR(SetUINT64, key, value);
}
IFACEMETHODIMP PicooActivator::SetDouble(REFGUID key, double value) {
    DELEGATE_ATTR(SetDouble, key, value);
}
IFACEMETHODIMP PicooActivator::SetGUID(REFGUID key, REFGUID value) {
    DELEGATE_ATTR(SetGUID, key, value);
}
IFACEMETHODIMP PicooActivator::SetString(REFGUID key, LPCWSTR value) {
    DELEGATE_ATTR(SetString, key, value);
}
IFACEMETHODIMP PicooActivator::SetBlob(REFGUID key, const UINT8* blob, UINT32 size) {
    DELEGATE_ATTR(SetBlob, key, blob, size);
}
IFACEMETHODIMP PicooActivator::SetUnknown(REFGUID key, IUnknown* value) {
    DELEGATE_ATTR(SetUnknown, key, value);
}
IFACEMETHODIMP PicooActivator::LockStore() {
    DELEGATE_ATTR(LockStore);
}
IFACEMETHODIMP PicooActivator::UnlockStore() {
    DELEGATE_ATTR(UnlockStore);
}
IFACEMETHODIMP PicooActivator::GetCount(UINT32* count) {
    DELEGATE_ATTR(GetCount, count);
}
IFACEMETHODIMP PicooActivator::GetItemByIndex(UINT32 index, GUID* key, PROPVARIANT* value) {
    DELEGATE_ATTR(GetItemByIndex, index, key, value);
}
IFACEMETHODIMP PicooActivator::CopyAllItems(IMFAttributes* destination) {
    DELEGATE_ATTR(CopyAllItems, destination);
}

#undef DELEGATE_ATTR
