#pragma once

#include <mfidl.h>
#include <wrl/implements.h>

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
    IFACEMETHOD(GetItem)(REFGUID key, PROPVARIANT* value) override;
    IFACEMETHOD(GetItemType)(REFGUID key, MF_ATTRIBUTE_TYPE* type) override;
    IFACEMETHOD(CompareItem)(REFGUID key, REFPROPVARIANT value, BOOL* result) override;
    IFACEMETHOD(Compare)(IMFAttributes* other, MF_ATTRIBUTES_MATCH_TYPE match, BOOL* result) override;
    IFACEMETHOD(GetUINT32)(REFGUID key, UINT32* value) override;
    IFACEMETHOD(GetUINT64)(REFGUID key, UINT64* value) override;
    IFACEMETHOD(GetDouble)(REFGUID key, double* value) override;
    IFACEMETHOD(GetGUID)(REFGUID key, GUID* value) override;
    IFACEMETHOD(GetStringLength)(REFGUID key, UINT32* length) override;
    IFACEMETHOD(GetString)(REFGUID key, LPWSTR value, UINT32 size, UINT32* length) override;
    IFACEMETHOD(GetAllocatedString)(REFGUID key, LPWSTR* value, UINT32* length) override;
    IFACEMETHOD(GetBlobSize)(REFGUID key, UINT32* size) override;
    IFACEMETHOD(GetBlob)(REFGUID key, UINT8* blob, UINT32 size, UINT32* length) override;
    IFACEMETHOD(GetAllocatedBlob)(REFGUID key, UINT8** blob, UINT32* size) override;
    IFACEMETHOD(GetUnknown)(REFGUID key, REFIID riid, LPVOID* value) override;
    IFACEMETHOD(SetItem)(REFGUID key, REFPROPVARIANT value) override;
    IFACEMETHOD(DeleteItem)(REFGUID key) override;
    IFACEMETHOD(DeleteAllItems)() override;
    IFACEMETHOD(SetUINT32)(REFGUID key, UINT32 value) override;
    IFACEMETHOD(SetUINT64)(REFGUID key, UINT64 value) override;
    IFACEMETHOD(SetDouble)(REFGUID key, double value) override;
    IFACEMETHOD(SetGUID)(REFGUID key, REFGUID value) override;
    IFACEMETHOD(SetString)(REFGUID key, LPCWSTR value) override;
    IFACEMETHOD(SetBlob)(REFGUID key, const UINT8* blob, UINT32 size) override;
    IFACEMETHOD(SetUnknown)(REFGUID key, IUnknown* value) override;
    IFACEMETHOD(LockStore)() override;
    IFACEMETHOD(UnlockStore)() override;
    IFACEMETHOD(GetCount)(UINT32* count) override;
    IFACEMETHOD(GetItemByIndex)(UINT32 index, GUID* key, PROPVARIANT* value) override;
    IFACEMETHOD(CopyAllItems)(IMFAttributes* destination) override;

private:
    Microsoft::WRL::ComPtr<IMFAttributes> attributes_;
    Microsoft::WRL::ComPtr<PicooMediaSource> source_;
};
