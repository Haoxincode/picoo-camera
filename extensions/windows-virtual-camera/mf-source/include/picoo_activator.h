#pragma once

#include "picoo_mf_headers.h"

class PicooMediaSource;

class PicooActivator
    : public Microsoft::WRL::RuntimeClass<
          Microsoft::WRL::RuntimeClassFlags<Microsoft::WRL::ClassicCom>,
          IMFActivate> {
public:
    PicooActivator();
    ~PicooActivator() override;

    HRESULT Initialize();

    // IMFActivate
    IFACEMETHOD(ActivateObject)(REFIID riid, void** object) override;
    IFACEMETHOD(ShutdownObject)() override;
    IFACEMETHOD(DetachObject)() override;

    // IMFAttributes — delegate to internal bag
    IFACEMETHOD(GetItem)(REFGUID key, PROPVARIANT* out_value) override;
    IFACEMETHOD(GetItemType)(REFGUID key, MF_ATTRIBUTE_TYPE* type) override;
    IFACEMETHOD(CompareItem)(REFGUID key, REFPROPVARIANT compare_value, BOOL* result) override;
    IFACEMETHOD(Compare)(IMFAttributes* other, MF_ATTRIBUTES_MATCH_TYPE match, BOOL* result) override;
    IFACEMETHOD(GetUINT32)(REFGUID key, UINT32* out_value) override;
    IFACEMETHOD(GetUINT64)(REFGUID key, UINT64* out_value) override;
    IFACEMETHOD(GetDouble)(REFGUID key, double* out_value) override;
    IFACEMETHOD(GetGUID)(REFGUID key, GUID* out_value) override;
    IFACEMETHOD(GetStringLength)(REFGUID key, UINT32* length) override;
    IFACEMETHOD(GetString)(REFGUID key, LPWSTR buffer, UINT32 size, UINT32* length) override;
    IFACEMETHOD(GetAllocatedString)(REFGUID key, LPWSTR* out_value, UINT32* length) override;
    IFACEMETHOD(GetBlobSize)(REFGUID key, UINT32* size) override;
    IFACEMETHOD(GetBlob)(REFGUID key, UINT8* blob, UINT32 size, UINT32* length) override;
    IFACEMETHOD(GetAllocatedBlob)(REFGUID key, UINT8** blob, UINT32* size) override;
    IFACEMETHOD(GetUnknown)(REFGUID key, REFIID riid, LPVOID* out_value) override;
    IFACEMETHOD(SetItem)(REFGUID key, REFPROPVARIANT item_value) override;
    IFACEMETHOD(DeleteItem)(REFGUID key) override;
    IFACEMETHOD(DeleteAllItems)() override;
    IFACEMETHOD(SetUINT32)(REFGUID key, UINT32 item_value) override;
    IFACEMETHOD(SetUINT64)(REFGUID key, UINT64 item_value) override;
    IFACEMETHOD(SetDouble)(REFGUID key, double item_value) override;
    IFACEMETHOD(SetGUID)(REFGUID key, REFGUID item_value) override;
    IFACEMETHOD(SetString)(REFGUID key, LPCWSTR item_value) override;
    IFACEMETHOD(SetBlob)(REFGUID key, const UINT8* blob, UINT32 size) override;
    IFACEMETHOD(SetUnknown)(REFGUID key, IUnknown* item_value) override;
    IFACEMETHOD(LockStore)() override;
    IFACEMETHOD(UnlockStore)() override;
    IFACEMETHOD(GetCount)(UINT32* count) override;
    IFACEMETHOD(GetItemByIndex)(UINT32 index, GUID* key, PROPVARIANT* out_value) override;
    IFACEMETHOD(CopyAllItems)(IMFAttributes* destination) override;

private:
    Microsoft::WRL::ComPtr<IMFAttributes> attributes_;
    Microsoft::WRL::ComPtr<PicooMediaSource> source_;
};
