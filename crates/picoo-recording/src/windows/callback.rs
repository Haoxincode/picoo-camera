use ::windows::{
    core::{implement, Result, HRESULT},
    Win32::Media::MediaFoundation::{IMFSinkWriterCallback, IMFSinkWriterCallback_Impl},
};
use std::{ffi::c_void, sync::mpsc::SyncSender};

pub(super) enum Event {
    Marker,
    Finalized(HRESULT),
}

#[implement(IMFSinkWriterCallback)]
pub(super) struct Completion {
    pub send: SyncSender<Event>,
}

impl IMFSinkWriterCallback_Impl for Completion_Impl {
    fn OnFinalize(&self, result: HRESULT) -> Result<()> {
        let _ = self.send.try_send(Event::Finalized(result));
        Ok(())
    }
    fn OnMarker(&self, _stream: u32, _context: *const c_void) -> Result<()> {
        let _ = self.send.try_send(Event::Marker);
        Ok(())
    }
}
