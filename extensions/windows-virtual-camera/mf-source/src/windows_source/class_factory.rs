use std::ffi::c_void;

use windows::core::{implement, Error, Ref, Result, BOOL, GUID};
use windows::Win32::Foundation::CLASS_E_NOAGGREGATION;
use windows::Win32::System::Com::{
    IAgileObject, IAgileObject_Impl, IClassFactory, IClassFactory_Impl,
};

use super::activator::Activator;
use super::{query_interface, set_server_lock, ObjectTracker};

#[implement(IClassFactory, IAgileObject)]
pub(super) struct ClassFactory {
    _tracker: ObjectTracker,
}

impl ClassFactory {
    pub fn create() -> IClassFactory {
        Self {
            _tracker: ObjectTracker::new(),
        }
        .into()
    }
}

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<'_, windows::core::IUnknown>,
        riid: *const GUID,
        output: *mut *mut c_void,
    ) -> Result<()> {
        if !outer.is_null() {
            return Err(Error::from(CLASS_E_NOAGGREGATION));
        }
        let activator = Activator::create()?;
        unsafe { query_interface(&activator, riid, output) }
    }

    fn LockServer(&self, lock: BOOL) -> Result<()> {
        set_server_lock(lock.as_bool());
        Ok(())
    }
}

impl IAgileObject_Impl for ClassFactory_Impl {}
