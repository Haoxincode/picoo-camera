use std::ffi::c_void;
use std::sync::Mutex;

use windows::core::{implement, Interface, Ref, Result, BOOL, GUID, PCWSTR, PWSTR};
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFActivate_Impl, IMFAttributes, IMFAttributes_Impl, IMFMediaSource,
    IMFMediaSourceEx, MFCreateAttributes, MFT_TRANSFORM_CLSID_Attribute, MF_ATTRIBUTES_MATCH_TYPE,
    MF_ATTRIBUTE_TYPE,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{IAgileObject, IAgileObject_Impl};

use super::media_source::MediaSource;
use super::{lock, query_interface, ObjectTracker, PICOO_VCAM_CLSID};

#[implement(IMFActivate, IAgileObject)]
pub(super) struct Activator {
    attributes: IMFAttributes,
    source: Mutex<Option<IMFMediaSourceEx>>,
    _tracker: ObjectTracker,
}

impl Activator {
    pub fn create() -> Result<IMFActivate> {
        unsafe {
            let mut attributes = None;
            MFCreateAttributes(&mut attributes, 1)?;
            let attributes = attributes
                .ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))?;
            attributes.SetGUID(&MFT_TRANSFORM_CLSID_Attribute, &PICOO_VCAM_CLSID)?;
            let source = MediaSource::create(attributes.clone())?;
            Ok(Self {
                attributes,
                source: Mutex::new(Some(source)),
                _tracker: ObjectTracker::new(),
            }
            .into())
        }
    }
}

impl IMFActivate_Impl for Activator_Impl {
    fn ActivateObject(&self, riid: *const GUID, output: *mut *mut c_void) -> Result<()> {
        let source = lock(&self.source)?.as_ref().cloned().ok_or_else(|| {
            windows::core::Error::from(windows::Win32::Media::MediaFoundation::MF_E_SHUTDOWN)
        })?;
        unsafe { query_interface(&source, riid, output) }
    }

    fn ShutdownObject(&self) -> Result<()> {
        if let Some(source) = lock(&self.source)?.as_ref().cloned() {
            let source: IMFMediaSource = source.cast()?;
            unsafe { source.Shutdown()? };
        }
        Ok(())
    }

    fn DetachObject(&self) -> Result<()> {
        let source = lock(&self.source)?.take();
        if let Some(source) = source {
            let source: IMFMediaSource = source.cast()?;
            unsafe { source.Shutdown()? };
        }
        Ok(())
    }
}

impl IMFAttributes_Impl for Activator_Impl {
    fn GetItem(&self, key: *const GUID, value: *mut PROPVARIANT) -> Result<()> {
        unsafe { self.attributes.GetItem(key, Some(value)) }
    }

    fn GetItemType(&self, key: *const GUID) -> Result<MF_ATTRIBUTE_TYPE> {
        unsafe { self.attributes.GetItemType(key) }
    }

    fn CompareItem(&self, key: *const GUID, value: *const PROPVARIANT) -> Result<BOOL> {
        unsafe { self.attributes.CompareItem(key, value) }
    }

    fn Compare(
        &self,
        other: Ref<'_, IMFAttributes>,
        match_type: MF_ATTRIBUTES_MATCH_TYPE,
    ) -> Result<BOOL> {
        unsafe { self.attributes.Compare(other.as_ref(), match_type) }
    }

    fn GetUINT32(&self, key: *const GUID) -> Result<u32> {
        unsafe { self.attributes.GetUINT32(key) }
    }

    fn GetUINT64(&self, key: *const GUID) -> Result<u64> {
        unsafe { self.attributes.GetUINT64(key) }
    }

    fn GetDouble(&self, key: *const GUID) -> Result<f64> {
        unsafe { self.attributes.GetDouble(key) }
    }

    fn GetGUID(&self, key: *const GUID) -> Result<GUID> {
        unsafe { self.attributes.GetGUID(key) }
    }

    fn GetStringLength(&self, key: *const GUID) -> Result<u32> {
        unsafe { self.attributes.GetStringLength(key) }
    }

    fn GetString(
        &self,
        key: *const GUID,
        value: PWSTR,
        capacity: u32,
        length: *mut u32,
    ) -> Result<()> {
        let attributes = &self.attributes;
        unsafe {
            (attributes.vtable().GetString)(attributes.as_raw(), key, value, capacity, length).ok()
        }
    }

    fn GetAllocatedString(
        &self,
        key: *const GUID,
        value: *mut PWSTR,
        length: *mut u32,
    ) -> Result<()> {
        unsafe { self.attributes.GetAllocatedString(key, value, length) }
    }

    fn GetBlobSize(&self, key: *const GUID) -> Result<u32> {
        unsafe { self.attributes.GetBlobSize(key) }
    }

    fn GetBlob(
        &self,
        key: *const GUID,
        buffer: *mut u8,
        capacity: u32,
        length: *mut u32,
    ) -> Result<()> {
        let attributes = &self.attributes;
        unsafe {
            (attributes.vtable().GetBlob)(attributes.as_raw(), key, buffer, capacity, length).ok()
        }
    }

    fn GetAllocatedBlob(
        &self,
        key: *const GUID,
        buffer: *mut *mut u8,
        size: *mut u32,
    ) -> Result<()> {
        unsafe { self.attributes.GetAllocatedBlob(key, buffer, size) }
    }

    fn GetUnknown(
        &self,
        key: *const GUID,
        riid: *const GUID,
        output: *mut *mut c_void,
    ) -> Result<()> {
        let attributes = &self.attributes;
        unsafe { (attributes.vtable().GetUnknown)(attributes.as_raw(), key, riid, output).ok() }
    }

    fn SetItem(&self, key: *const GUID, value: *const PROPVARIANT) -> Result<()> {
        unsafe { self.attributes.SetItem(key, value) }
    }

    fn DeleteItem(&self, key: *const GUID) -> Result<()> {
        unsafe { self.attributes.DeleteItem(key) }
    }

    fn DeleteAllItems(&self) -> Result<()> {
        unsafe { self.attributes.DeleteAllItems() }
    }

    fn SetUINT32(&self, key: *const GUID, value: u32) -> Result<()> {
        unsafe { self.attributes.SetUINT32(key, value) }
    }

    fn SetUINT64(&self, key: *const GUID, value: u64) -> Result<()> {
        unsafe { self.attributes.SetUINT64(key, value) }
    }

    fn SetDouble(&self, key: *const GUID, value: f64) -> Result<()> {
        unsafe { self.attributes.SetDouble(key, value) }
    }

    fn SetGUID(&self, key: *const GUID, value: *const GUID) -> Result<()> {
        unsafe { self.attributes.SetGUID(key, value) }
    }

    fn SetString(&self, key: *const GUID, value: &PCWSTR) -> Result<()> {
        unsafe { self.attributes.SetString(key, *value) }
    }

    fn SetBlob(&self, key: *const GUID, buffer: *const u8, size: u32) -> Result<()> {
        let attributes = &self.attributes;
        unsafe { (attributes.vtable().SetBlob)(attributes.as_raw(), key, buffer, size).ok() }
    }

    fn SetUnknown(&self, key: *const GUID, value: Ref<'_, windows::core::IUnknown>) -> Result<()> {
        unsafe { self.attributes.SetUnknown(key, value.as_ref()) }
    }

    fn LockStore(&self) -> Result<()> {
        unsafe { self.attributes.LockStore() }
    }

    fn UnlockStore(&self) -> Result<()> {
        unsafe { self.attributes.UnlockStore() }
    }

    fn GetCount(&self) -> Result<u32> {
        unsafe { self.attributes.GetCount() }
    }

    fn GetItemByIndex(&self, index: u32, key: *mut GUID, value: *mut PROPVARIANT) -> Result<()> {
        unsafe { self.attributes.GetItemByIndex(index, key, Some(value)) }
    }

    fn CopyAllItems(&self, destination: Ref<'_, IMFAttributes>) -> Result<()> {
        unsafe { self.attributes.CopyAllItems(destination.as_ref()) }
    }
}

impl IAgileObject_Impl for Activator_Impl {}
