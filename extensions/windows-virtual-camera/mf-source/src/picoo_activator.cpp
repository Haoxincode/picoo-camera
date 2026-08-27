// Picoo virtual camera IMFActivate — REQ-PICOO-VCAM-002.

#include "picoo_activator.h"

#include "picoo_com_macros.h"
#include "picoo_media_source.h"
#include "picoo_vcam_ids.h"

#include <mferror.h>

using Microsoft::WRL::ComPtr;

PicooActivator::PicooActivator() = default;
PicooActivator::~PicooActivator() = default;

HRESULT PicooActivator::Initialize() {
    RETURN_IF_FAILED(MFCreateAttributes(&attributes_, 3));
    RETURN_IF_FAILED(
        attributes_->SetUINT32(MF_VIRTUALCAMERA_PROVIDE_ASSOCIATED_CAMERA_SOURCES, 1));
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

IFACEMETHODIMP PicooActivator::GetItem(REFGUID key, PROPVARIANT* out_value) {
    DELEGATE_ATTR(GetItem, key, out_value);
}
IFACEMETHODIMP PicooActivator::GetItemType(REFGUID key, MF_ATTRIBUTE_TYPE* type) {
    DELEGATE_ATTR(GetItemType, key, type);
}
IFACEMETHODIMP PicooActivator::CompareItem(REFGUID key, REFPROPVARIANT compare_value, BOOL* result) {
    DELEGATE_ATTR(CompareItem, key, compare_value, result);
}
IFACEMETHODIMP PicooActivator::Compare(IMFAttributes* other, MF_ATTRIBUTES_MATCH_TYPE match, BOOL* result) {
    DELEGATE_ATTR(Compare, other, match, result);
}
IFACEMETHODIMP PicooActivator::GetUINT32(REFGUID key, UINT32* out_value) {
    DELEGATE_ATTR(GetUINT32, key, out_value);
}
IFACEMETHODIMP PicooActivator::GetUINT64(REFGUID key, UINT64* out_value) {
    DELEGATE_ATTR(GetUINT64, key, out_value);
}
IFACEMETHODIMP PicooActivator::GetDouble(REFGUID key, double* out_value) {
    DELEGATE_ATTR(GetDouble, key, out_value);
}
IFACEMETHODIMP PicooActivator::GetGUID(REFGUID key, GUID* out_value) {
    DELEGATE_ATTR(GetGUID, key, out_value);
}
IFACEMETHODIMP PicooActivator::GetStringLength(REFGUID key, UINT32* length) {
    DELEGATE_ATTR(GetStringLength, key, length);
}
IFACEMETHODIMP PicooActivator::GetString(REFGUID key, LPWSTR buffer, UINT32 size, UINT32* length) {
    DELEGATE_ATTR(GetString, key, buffer, size, length);
}
IFACEMETHODIMP PicooActivator::GetAllocatedString(REFGUID key, LPWSTR* out_value, UINT32* length) {
    DELEGATE_ATTR(GetAllocatedString, key, out_value, length);
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
IFACEMETHODIMP PicooActivator::GetUnknown(REFGUID key, REFIID riid, LPVOID* out_value) {
    DELEGATE_ATTR(GetUnknown, key, riid, out_value);
}
IFACEMETHODIMP PicooActivator::SetItem(REFGUID key, REFPROPVARIANT item_value) {
    DELEGATE_ATTR(SetItem, key, item_value);
}
IFACEMETHODIMP PicooActivator::DeleteItem(REFGUID key) {
    DELEGATE_ATTR(DeleteItem, key);
}
IFACEMETHODIMP PicooActivator::DeleteAllItems() {
    DELEGATE_ATTR(DeleteAllItems);
}
IFACEMETHODIMP PicooActivator::SetUINT32(REFGUID key, UINT32 item_value) {
    DELEGATE_ATTR(SetUINT32, key, item_value);
}
IFACEMETHODIMP PicooActivator::SetUINT64(REFGUID key, UINT64 item_value) {
    DELEGATE_ATTR(SetUINT64, key, item_value);
}
IFACEMETHODIMP PicooActivator::SetDouble(REFGUID key, double item_value) {
    DELEGATE_ATTR(SetDouble, key, item_value);
}
IFACEMETHODIMP PicooActivator::SetGUID(REFGUID key, REFGUID item_value) {
    DELEGATE_ATTR(SetGUID, key, item_value);
}
IFACEMETHODIMP PicooActivator::SetString(REFGUID key, LPCWSTR item_value) {
    DELEGATE_ATTR(SetString, key, item_value);
}
IFACEMETHODIMP PicooActivator::SetBlob(REFGUID key, const UINT8* blob, UINT32 size) {
    DELEGATE_ATTR(SetBlob, key, blob, size);
}
IFACEMETHODIMP PicooActivator::SetUnknown(REFGUID key, IUnknown* item_value) {
    DELEGATE_ATTR(SetUnknown, key, item_value);
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
IFACEMETHODIMP PicooActivator::GetItemByIndex(UINT32 index, GUID* key, PROPVARIANT* out_value) {
    DELEGATE_ATTR(GetItemByIndex, index, key, out_value);
}
IFACEMETHODIMP PicooActivator::CopyAllItems(IMFAttributes* destination) {
    DELEGATE_ATTR(CopyAllItems, destination);
}

#undef DELEGATE_ATTR
