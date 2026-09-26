//! RequestSample routing between native live content and explicit placeholders.

use super::{cpu, lock, native, SharedStreamState};
use crate::frame_provider::FrameOrigin;
use picoo_frame_hub::SharedFrameKind;
use std::sync::Arc;
use windows::core::{Error, IUnknown, Ref, Result};
use windows::Win32::Media::MediaFoundation::{
    MF_E_MEDIA_SOURCE_WRONGSTATE, MF_STREAM_STATE_RUNNING,
};

pub(super) fn deliver_sample(
    shared: &SharedStreamState,
    token: Ref<'_, IUnknown>,
) -> Result<Option<FrameOrigin>> {
    let (native, frames, output_width, output_height) = {
        let state = lock(shared)?;
        if state.state != MF_STREAM_STATE_RUNNING || state.transitioning {
            return Err(Error::from(MF_E_MEDIA_SOURCE_WRONGSTATE));
        }
        (
            state.native_generation.is_some(),
            Arc::clone(&state.frames),
            state.output_width,
            state.output_height,
        )
    };
    if native {
        let placeholder = frames.content_kind() == SharedFrameKind::Placeholder;
        native::set_placeholder_active(shared, placeholder)?;
        if placeholder {
            frames.set_placeholder_output_active(output_width, output_height, true);
            return cpu::deliver_placeholder_sample(shared, token).map(Some);
        }
        frames.set_placeholder_output_active(output_width, output_height, false);
        native::request_native_sample(shared, token)?;
        return Ok(None);
    }
    cpu::deliver_cpu_sample(shared, token).map(Some)
}
