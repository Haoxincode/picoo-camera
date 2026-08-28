// PicooVirtualCameraSource.dll — IMFMediaSource COM + Shared Frame Ring — REQ-PICOO-VCAM-002.

#include <windows.h>

#include "picoo_activator.h"
#include "picoo_com_macros.h"
#include "picoo_ring_reader.h"
#include "picoo_vcam_ids.h"

#include <mfapi.h>
#include <objbase.h>
#include <string>
#include <strsafe.h>
#include <wrl/implements.h>

namespace {

HMODULE g_module = nullptr;

extern "C" const wchar_t* PicooVcamFriendlyName(void);

class PicooClassFactory : public Microsoft::WRL::RuntimeClass<
                              Microsoft::WRL::RuntimeClassFlags<Microsoft::WRL::ClassicCom>,
                              IClassFactory> {
public:
    IFACEMETHODIMP CreateInstance(IUnknown* outer, REFIID riid, void** object) override {
        if (object == nullptr) {
            return E_POINTER;
        }
        *object = nullptr;
        if (outer != nullptr) {
            return CLASS_E_NOAGGREGATION;
        }

        Microsoft::WRL::ComPtr<PicooActivator> activator = Microsoft::WRL::Make<PicooActivator>();
        RETURN_IF_FAILED(activator->Initialize());
        return activator->QueryInterface(riid, object);
    }

    IFACEMETHODIMP LockServer(BOOL) override {
        return S_OK;
    }
};

std::wstring GuidToRegistryString(REFGUID guid) {
    wchar_t buffer[64] = {};
    StringFromGUID2(guid, buffer, static_cast<int>(sizeof(buffer) / sizeof(wchar_t)));
    return buffer;
}

}  // namespace

BOOL APIENTRY DllMain(HMODULE module, DWORD reason, LPVOID reserved) {
    (void)reserved;
    switch (reason) {
    case DLL_PROCESS_ATTACH:
        g_module = module;
        DisableThreadLibraryCalls(module);
        break;
    default:
        break;
    }
    return TRUE;
}

extern "C" __declspec(dllexport) const char* PicooVcamSourceVersion(void) {
    return "PicooVirtualCameraSource/0.2.0-imf-media-source";
}

extern "C" __declspec(dllexport) int PicooVcamAttachRing(const char* ring_name) {
    const char* name =
        (ring_name != nullptr && ring_name[0] != '\0') ? ring_name : "picoo-camera-v1";
    PicooRingReader* reader = picoo_ring_reader_open(name, 0);
    if (reader == nullptr) {
        return 0;
    }
    picoo_ring_reader_close(reader);
    return 1;
}

extern "C" __declspec(dllexport) int PicooVcamPollFrame(PicooRingFrameView* out_frame) {
    static PicooRingReader* reader = nullptr;
    if (reader == nullptr) {
        reader = picoo_ring_reader_open("picoo-camera-v1", 0);
    }
    if (reader == nullptr || out_frame == nullptr) {
        return 0;
    }
    return picoo_ring_reader_poll(reader, out_frame);
}

STDAPI DllCanUnloadNow() {
    return S_OK;
}

STDAPI DllGetClassObject(REFCLSID clsid, REFIID riid, void** object) {
    if (object == nullptr) {
        return E_POINTER;
    }
    *object = nullptr;
    if (clsid != CLSID_PicooVirtualCameraSource) {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    Microsoft::WRL::ComPtr<PicooClassFactory> factory = Microsoft::WRL::Make<PicooClassFactory>();
    return factory->QueryInterface(riid, object);
}

STDAPI DllRegisterServer() {
    wchar_t module_path[MAX_PATH] = {};
    if (GetModuleFileNameW(g_module, module_path, MAX_PATH) == 0) {
        return HRESULT_FROM_WIN32(GetLastError());
    }

    const std::wstring clsid = GuidToRegistryString(CLSID_PicooVirtualCameraSource);
    const std::wstring clsid_key = L"Software\\Classes\\CLSID\\" + clsid;
    const std::wstring inproc_key = clsid_key + L"\\InprocServer32";

    HKEY key = nullptr;
    LSTATUS status = RegCreateKeyExW(HKEY_LOCAL_MACHINE, inproc_key.c_str(), 0, nullptr, 0,
                                     KEY_WRITE, nullptr, &key, nullptr);
    if (status != ERROR_SUCCESS) {
        return HRESULT_FROM_WIN32(status);
    }

    status = RegSetValueExW(key, nullptr, 0, REG_SZ,
                            reinterpret_cast<const BYTE*>(module_path),
                            static_cast<DWORD>((wcslen(module_path) + 1) * sizeof(wchar_t)));
    if (status != ERROR_SUCCESS) {
        RegCloseKey(key);
        return HRESULT_FROM_WIN32(status);
    }
    const wchar_t threading[] = L"Both";
    status = RegSetValueExW(key, L"ThreadingModel", 0, REG_SZ,
                            reinterpret_cast<const BYTE*>(threading),
                            static_cast<DWORD>((wcslen(threading) + 1) * sizeof(wchar_t)));
    RegCloseKey(key);
    if (status != ERROR_SUCCESS) {
        return HRESULT_FROM_WIN32(status);
    }

    status = RegCreateKeyExW(HKEY_LOCAL_MACHINE, clsid_key.c_str(), 0, nullptr, 0, KEY_WRITE,
                             nullptr, &key, nullptr);
    if (status != ERROR_SUCCESS) {
        return HRESULT_FROM_WIN32(status);
    }
    const wchar_t* friendly = PicooVcamFriendlyName();
    status = RegSetValueExW(key, nullptr, 0, REG_SZ, reinterpret_cast<const BYTE*>(friendly),
                            static_cast<DWORD>((wcslen(friendly) + 1) * sizeof(wchar_t)));
    RegCloseKey(key);
    if (status != ERROR_SUCCESS) {
        return HRESULT_FROM_WIN32(status);
    }

    return S_OK;
}

STDAPI DllUnregisterServer() {
    const std::wstring clsid = GuidToRegistryString(CLSID_PicooVirtualCameraSource);
    const std::wstring clsid_key = L"Software\\Classes\\CLSID\\" + clsid;
    const LSTATUS status = RegDeleteTreeW(HKEY_LOCAL_MACHINE, clsid_key.c_str());
    if (status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND) {
        return HRESULT_FROM_WIN32(status);
    }
    return S_OK;
}
