use std::ffi::c_void;
use std::sync::Mutex;

use windows::core::{implement, AgileReference, Interface, Ref, Result, BOOL, GUID, PCWSTR, PWSTR};
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFActivate_Impl, IMFAttributes, IMFAttributes_Impl, IMFMediaSource,
    IMFMediaSourceEx, MFCreateAttributes, MFT_TRANSFORM_CLSID_Attribute, MF_ATTRIBUTES_MATCH_TYPE,
    MF_ATTRIBUTE_TYPE, MF_VIRTUALCAMERA_PROVIDE_ASSOCIATED_CAMERA_SOURCES,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{IAgileObject, IAgileObject_Impl};

use super::media_source::MediaSource;
use super::{lock, query_interface, ObjectTracker, PICOO_VCAM_CLSID};

#[implement(IMFActivate, IAgileObject)]
pub(super) struct Activator {
    attributes: AgileReference<IMFAttributes>,
    source: Mutex<Option<AgileReference<IMFMediaSourceEx>>>,
    _tracker: ObjectTracker,
}

impl Activator {
    pub fn create() -> Result<IMFActivate> {
        unsafe {
            let mut attributes = None;
            MFCreateAttributes(&mut attributes, 3)?;
            let attributes = attributes
                .ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))?;
            attributes.SetUINT32(&MF_VIRTUALCAMERA_PROVIDE_ASSOCIATED_CAMERA_SOURCES, 1)?;
            attributes.SetGUID(&MFT_TRANSFORM_CLSID_Attribute, &PICOO_VCAM_CLSID)?;
            let source = MediaSource::create(attributes.clone())?;
            Ok(Self {
                attributes: AgileReference::new(&attributes)?,
                source: Mutex::new(Some(AgileReference::new(&source)?)),
                _tracker: ObjectTracker::new(),
            }
            .into())
        }
    }
}

impl IMFActivate_Impl for Activator_Impl {
    fn ActivateObject(&self, riid: *const GUID, output: *mut *mut c_void) -> Result<()> {
        let source = lock(&self.source)?;
        let source = source
            .as_ref()
            .ok_or_else(|| {
                windows::core::Error::from(windows::Win32::Media::MediaFoundation::MF_E_SHUTDOWN)
            })?
            .resolve()?;
        unsafe { query_interface(&source, riid, output) }
    }

    fn ShutdownObject(&self) -> Result<()> {
        if let Some(source) = lock(&self.source)?.as_ref() {
            let source = source.resolve()?;
            let source: IMFMediaSource = source.cast()?;
            unsafe { source.Shutdown()? };
        }
        Ok(())
    }

    fn DetachObject(&self) -> Result<()> {
        let source = lock(&self.source)?.take();
        if let Some(source) = source {
            let source = source.resolve()?;
            let source: IMFMediaSource = source.cast()?;
            unsafe { source.Shutdown()? };
        }
        Ok(())
    }
}

impl IMFAttributes_Impl for Activator_Impl {
    fn GetItem(&self, key: *const GUID, value: *mut PROPVARIANT) -> Result<()> {
        unsafe { self.attributes.resolve()?.GetItem(key, Some(value)) }
    }

    fn GetItemType(&self, key: *const GUID) -> Result<MF_ATTRIBUTE_TYPE> {
        unsafe { self.attributes.resolve()?.GetItemType(key) }
    }

    fn CompareItem(&self, key: *const GUID, value: *const PROPVARIANT) -> Result<BOOL> {
        unsafe { self.attributes.resolve()?.CompareItem(key, value) }
    }

    fn Compare(
        &self,
        other: Ref<'_, IMFAttributes>,
        match_type: MF_ATTRIBUTES_MATCH_TYPE,
    ) -> Result<BOOL> {
        unsafe {
            self.attributes
                .resolve()?
                .Compare(other.as_ref(), match_type)
        }
    }

    fn GetUINT32(&self, key: *const GUID) -> Result<u32> {
        unsafe { self.attributes.resolve()?.GetUINT32(key) }
    }

    fn GetUINT64(&self, key: *const GUID) -> Result<u64> {
        unsafe { self.attributes.resolve()?.GetUINT64(key) }
    }

    fn GetDouble(&self, key: *const GUID) -> Result<f64> {
        unsafe { self.attributes.resolve()?.GetDouble(key) }
    }

    fn GetGUID(&self, key: *const GUID) -> Result<GUID> {
        unsafe { self.attributes.resolve()?.GetGUID(key) }
    }

    fn GetStringLength(&self, key: *const GUID) -> Result<u32> {
        unsafe { self.attributes.resolve()?.GetStringLength(key) }
    }

    fn GetString(
        &self,
        key: *const GUID,
        value: PWSTR,
        capacity: u32,
        length: *mut u32,
    ) -> Result<()> {
        let attributes = self.attributes.resolve()?;
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
        unsafe {
            self.attributes
                .resolve()?
                .GetAllocatedString(key, value, length)
        }
    }

    fn GetBlobSize(&self, key: *const GUID) -> Result<u32> {
        unsafe { self.attributes.resolve()?.GetBlobSize(key) }
    }

    fn GetBlob(
        &self,
        key: *const GUID,
        buffer: *mut u8,
        capacity: u32,
        length: *mut u32,
    ) -> Result<()> {
        let attributes = self.attributes.resolve()?;
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
        unsafe {
            self.attributes
                .resolve()?
                .GetAllocatedBlob(key, buffer, size)
        }
    }

    fn GetUnknown(
        &self,
        key: *const GUID,
        riid: *const GUID,
        output: *mut *mut c_void,
    ) -> Result<()> {
        let attributes = self.attributes.resolve()?;
        unsafe { (attributes.vtable().GetUnknown)(attributes.as_raw(), key, riid, output).ok() }
    }

    fn SetItem(&self, key: *const GUID, value: *const PROPVARIANT) -> Result<()> {
        unsafe { self.attributes.resolve()?.SetItem(key, value) }
    }

    fn DeleteItem(&self, key: *const GUID) -> Result<()> {
        unsafe { self.attributes.resolve()?.DeleteItem(key) }
    }

    fn DeleteAllItems(&self) -> Result<()> {
        unsafe { self.attributes.resolve()?.DeleteAllItems() }
    }

    fn SetUINT32(&self, key: *const GUID, value: u32) -> Result<()> {
        unsafe { self.attributes.resolve()?.SetUINT32(key, value) }
    }

    fn SetUINT64(&self, key: *const GUID, value: u64) -> Result<()> {
        unsafe { self.attributes.resolve()?.SetUINT64(key, value) }
    }

    fn SetDouble(&self, key: *const GUID, value: f64) -> Result<()> {
        unsafe { self.attributes.resolve()?.SetDouble(key, value) }
    }

    fn SetGUID(&self, key: *const GUID, value: *const GUID) -> Result<()> {
        unsafe { self.attributes.resolve()?.SetGUID(key, value) }
    }

    fn SetString(&self, key: *const GUID, value: &PCWSTR) -> Result<()> {
        unsafe { self.attributes.resolve()?.SetString(key, *value) }
    }

    fn SetBlob(&self, key: *const GUID, buffer: *const u8, size: u32) -> Result<()> {
        let attributes = self.attributes.resolve()?;
        unsafe { (attributes.vtable().SetBlob)(attributes.as_raw(), key, buffer, size).ok() }
    }

    fn SetUnknown(&self, key: *const GUID, value: Ref<'_, windows::core::IUnknown>) -> Result<()> {
        unsafe { self.attributes.resolve()?.SetUnknown(key, value.as_ref()) }
    }

    fn LockStore(&self) -> Result<()> {
        unsafe { self.attributes.resolve()?.LockStore() }
    }

    fn UnlockStore(&self) -> Result<()> {
        unsafe { self.attributes.resolve()?.UnlockStore() }
    }

    fn GetCount(&self) -> Result<u32> {
        unsafe { self.attributes.resolve()?.GetCount() }
    }

    fn GetItemByIndex(&self, index: u32, key: *mut GUID, value: *mut PROPVARIANT) -> Result<()> {
        unsafe {
            self.attributes
                .resolve()?
                .GetItemByIndex(index, key, Some(value))
        }
    }

    fn CopyAllItems(&self, destination: Ref<'_, IMFAttributes>) -> Result<()> {
        unsafe {
            self.attributes
                .resolve()?
                .CopyAllItems(destination.as_ref())
        }
    }
}

impl IAgileObject_Impl for Activator_Impl {}
