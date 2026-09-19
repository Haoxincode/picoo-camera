//! Legal macOS Camera Extension sink transport.
//!
//! The host is a Core Media IO output client: it discovers Picoo's `.sink`
//! stream, enqueues CMIO image samples, and retains each sample until DAL
//! removes its queue token. The extension owns the independent source clock.

use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::{size_of, MaybeUninit};
use std::ptr::{self, NonNull};
use std::sync::{Mutex, OnceLock};

use objc2_core_foundation::{CFRetained, CFString};
use objc2_core_media::{
    CMClock, CMFormatDescription, CMSampleBuffer, CMSampleTimingInfo, CMSimpleQueue, CMTime,
    CMTimeFlags, CMVideoFormatDescriptionCreateForImageBuffer,
    CMVideoFormatDescriptionGetDimensions,
};
use objc2_core_video::{CVPixelBuffer, CVPixelBufferGetHeight, CVPixelBufferGetWidth};

const DEVICE_UID: &str = "com.haoxincode.picoo-camera.virtual-camera";
const SYSTEM_OBJECT: u32 = 1;
const SCOPE_GLOBAL: u32 = fourcc(b"glob");
const SCOPE_OUTPUT: u32 = fourcc(b"outp");
const ELEMENT_MAIN: u32 = 0;
const PROPERTY_DEVICES: u32 = fourcc(b"dev#");
const PROPERTY_DEVICE_UID: u32 = fourcc(b"uid ");
const PROPERTY_STREAMS: u32 = fourcc(b"stm#");
const PROPERTY_DIRECTION: u32 = fourcc(b"sdir");
const PROPERTY_FORMAT: u32 = fourcc(b"pft ");
const PROPERTY_FRAME_RATE: u32 = fourcc(b"nfrt");
const PROPERTY_QUEUE_SIZE: u32 = fourcc(b"pmoq");
const PROPERTY_STARTUP_BUFFERS: u32 = fourcc(b"pmos");
const OUTPUT_DIRECTION: u32 = 0;
const QUEUE_CAPACITY: u32 = 3;

const fn fourcc(value: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*value)
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

type QueueAltered = unsafe extern "C" fn(u32, *mut c_void, *mut c_void);

#[link(name = "CoreMediaIO", kind = "framework")]
unsafe extern "C" {
    fn CMIOObjectGetPropertyDataSize(
        object: u32,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        data_size: *mut u32,
    ) -> i32;
    fn CMIOObjectGetPropertyData(
        object: u32,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        data_size: u32,
        data_used: *mut u32,
        data: *mut c_void,
    ) -> i32;
    fn CMIOObjectSetPropertyData(
        object: u32,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        data_size: u32,
        data: *const c_void,
    ) -> i32;
    fn CMIOStreamCopyBufferQueue(
        stream: u32,
        callback: Option<QueueAltered>,
        refcon: *mut c_void,
        queue: *mut *mut CMSimpleQueue,
    ) -> i32;
    fn CMIODeviceStartStream(device: u32, stream: u32) -> i32;
    fn CMIODeviceStopStream(device: u32, stream: u32) -> i32;
    fn CMIOSampleBufferCreateForImageBuffer(
        allocator: *const c_void,
        image: *const CVPixelBuffer,
        format: *const CMFormatDescription,
        timing: *const CMSampleTimingInfo,
        sequence: u64,
        discontinuity: u32,
        sample: *mut *mut CMSampleBuffer,
    ) -> i32;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SinkLayout {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) frames_per_second: u32,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum CmioSinkError {
    #[error("Picoo Camera CMIO device is unavailable")]
    DeviceUnavailable,
    #[error("Picoo Camera CMIO sink stream is unavailable")]
    StreamUnavailable,
    #[error("another Picoo CMIO sink producer is already active")]
    ProducerBusy,
    #[error("unsupported CMIO sink layout {0}x{1} at {2}fps")]
    UnsupportedLayout(u32, u32, u32),
    #[error("CMIO {0} failed with OSStatus {1}")]
    Platform(&'static str, i32),
    #[error("CMIO sink sample sequence exhausted")]
    SequenceExhausted,
    #[error("CMIO sink queue is full")]
    QueueFull,
    #[error("CMIO returned an invalid object")]
    InvalidObject,
}

#[derive(Default)]
struct CallbackState {
    owner: Option<u64>,
    next_owner: u64,
    inflight: HashMap<usize, InflightSample>,
}

struct InflightSample {
    _sample: CFRetained<CMSampleBuffer>,
}

// SAFETY: Samples are fully constructed before insertion and never mutated.
// CMIO removes the opaque pointer on its callback thread; that thread only
// drops the immutable retained owner after the system has consumed the token.
unsafe impl Send for InflightSample {}

// CMIO owns the callback address for a process-global device queue. Keeping one
// bounded state object for the process avoids a callback/refcon use-after-free
// while individual receiver sessions start and stop.
static CALLBACK_STATE: OnceLock<Mutex<CallbackState>> = OnceLock::new();

fn callback_state() -> &'static Mutex<CallbackState> {
    CALLBACK_STATE.get_or_init(|| Mutex::new(CallbackState::default()))
}

unsafe extern "C" fn queue_altered(_stream: u32, token: *mut c_void, refcon: *mut c_void) {
    let owner = refcon as usize as u64;
    if token.is_null() || owner == 0 {
        return;
    }
    let mut state = callback_state().lock().unwrap();
    if state.owner == Some(owner) {
        state.inflight.remove(&(token as usize));
    }
}

pub(super) struct MacCmioSink {
    device: u32,
    stream: u32,
    queue: CFRetained<CMSimpleQueue>,
    owner: u64,
    sequence: u64,
    stopped: bool,
}

impl MacCmioSink {
    pub(super) fn connect() -> Result<Self, CmioSinkError> {
        let owner = {
            let mut state = callback_state().lock().unwrap();
            if state.owner.is_some() {
                return Err(CmioSinkError::ProducerBusy);
            }
            state.next_owner = state
                .next_owner
                .checked_add(1)
                .ok_or(CmioSinkError::SequenceExhausted)?;
            let owner = state.next_owner;
            state.owner = Some(owner);
            owner
        };
        match Self::connect_owned(owner) {
            Ok(sink) => Ok(sink),
            Err(error) => {
                let mut state = callback_state().lock().unwrap();
                if state.owner == Some(owner) {
                    state.owner = None;
                }
                Err(error)
            }
        }
    }

    fn connect_owned(owner: u64) -> Result<Self, CmioSinkError> {
        let owner_refcon = usize::try_from(owner).map_err(|_| CmioSinkError::SequenceExhausted)?;
        let device = find_device()?;
        let streams = property_u32_vec(
            device,
            PropertyAddress {
                selector: PROPERTY_STREAMS,
                scope: SCOPE_OUTPUT,
                element: ELEMENT_MAIN,
            },
        )?;
        let mut sinks = streams.into_iter().filter(|stream| {
            property_u32(
                *stream,
                PropertyAddress {
                    selector: PROPERTY_DIRECTION,
                    scope: SCOPE_GLOBAL,
                    element: ELEMENT_MAIN,
                },
            )
            .is_ok_and(|direction| direction == OUTPUT_DIRECTION)
        });
        let stream = sinks.next().ok_or(CmioSinkError::StreamUnavailable)?;
        if sinks.next().is_some() {
            return Err(CmioSinkError::StreamUnavailable);
        }
        set_property(stream, PROPERTY_QUEUE_SIZE, QUEUE_CAPACITY)?;
        set_property(stream, PROPERTY_STARTUP_BUFFERS, 1_u32)?;
        let configured_capacity = property_u32(
            stream,
            PropertyAddress {
                selector: PROPERTY_QUEUE_SIZE,
                scope: SCOPE_GLOBAL,
                element: ELEMENT_MAIN,
            },
        )?;
        let configured_startup = property_u32(
            stream,
            PropertyAddress {
                selector: PROPERTY_STARTUP_BUFFERS,
                scope: SCOPE_GLOBAL,
                element: ELEMENT_MAIN,
            },
        )?;
        if configured_capacity != QUEUE_CAPACITY || configured_startup != 1 {
            let _ = unregister_queue_callback(stream);
            return Err(CmioSinkError::InvalidObject);
        }
        let mut raw_queue = ptr::null_mut();
        checked("copy buffer queue", unsafe {
            CMIOStreamCopyBufferQueue(
                stream,
                Some(queue_altered),
                owner_refcon as *mut c_void,
                &mut raw_queue,
            )
        })?;
        let raw_queue = match NonNull::new(raw_queue) {
            Some(raw_queue) => raw_queue,
            None => {
                // The callback is registered as part of CopyBufferQueue even
                // when the provider returns no queue object. Remove it before
                // the outer owner cleanup allows a reconnect attempt.
                let _ = unregister_queue_callback(stream);
                return Err(CmioSinkError::InvalidObject);
            }
        };
        // SAFETY: CopyBufferQueue returned a +1 CoreFoundation object.
        let queue = unsafe { CFRetained::from_raw(raw_queue) };
        let capacity = unsafe { queue.capacity() };
        if capacity != QUEUE_CAPACITY as i32 {
            let _ = unregister_queue_callback(stream);
            return Err(CmioSinkError::InvalidObject);
        }
        if let Err(error) = checked("start stream", unsafe {
            CMIODeviceStartStream(device, stream)
        }) {
            let _ = unregister_queue_callback(stream);
            return Err(error);
        }
        Ok(Self {
            device,
            stream,
            queue,
            owner,
            sequence: 0,
            stopped: false,
        })
    }

    pub(super) fn layout(&self) -> Result<SinkLayout, CmioSinkError> {
        let format = property_cf::<CMFormatDescription>(
            self.stream,
            PropertyAddress {
                selector: PROPERTY_FORMAT,
                scope: SCOPE_GLOBAL,
                element: ELEMENT_MAIN,
            },
        )?;
        let dimensions = unsafe { CMVideoFormatDescriptionGetDimensions(&format) };
        let frame_rate = property_f64(
            self.stream,
            PropertyAddress {
                selector: PROPERTY_FRAME_RATE,
                scope: SCOPE_GLOBAL,
                element: ELEMENT_MAIN,
            },
        )?;
        let fps = if (frame_rate - 30.0).abs() < f64::EPSILON {
            30
        } else if (frame_rate - 60.0).abs() < f64::EPSILON {
            60
        } else {
            0
        };
        let width = u32::try_from(dimensions.width).unwrap_or(0);
        let height = u32::try_from(dimensions.height).unwrap_or(0);
        if !matches!(
            (width, height, fps),
            (1280, 720, 30 | 60) | (1920, 1080, 30 | 60)
        ) {
            return Err(CmioSinkError::UnsupportedLayout(width, height, fps));
        }
        Ok(SinkLayout {
            width,
            height,
            frames_per_second: fps,
        })
    }

    pub(super) fn submit(
        &mut self,
        image: &CVPixelBuffer,
        discontinuity: bool,
    ) -> Result<(), CmioSinkError> {
        if !self.has_capacity() {
            return Err(CmioSinkError::QueueFull);
        }
        let layout = self.layout()?;
        let width = u32::try_from(CVPixelBufferGetWidth(image)).unwrap_or(0);
        let height = u32::try_from(CVPixelBufferGetHeight(image)).unwrap_or(0);
        if width != layout.width || height != layout.height {
            return Err(CmioSinkError::UnsupportedLayout(
                width,
                height,
                layout.frames_per_second,
            ));
        }
        let sequence = self.sequence;
        let next_sequence = self
            .sequence
            .checked_add(1)
            .ok_or(CmioSinkError::SequenceExhausted)?;
        let sample = make_sample(image, layout.frames_per_second, sequence, discontinuity)?;
        let token = NonNull::new(
            CFRetained::<CMSampleBuffer>::as_ptr(&sample)
                .as_ptr()
                .cast::<c_void>(),
        )
        .ok_or(CmioSinkError::InvalidObject)?;
        {
            let mut state = callback_state().lock().unwrap();
            if state.owner != Some(self.owner)
                || state.inflight.contains_key(&(token.as_ptr() as usize))
            {
                return Err(CmioSinkError::InvalidObject);
            }
            state
                .inflight
                .insert(token.as_ptr() as usize, InflightSample { _sample: sample });
        }
        let status = unsafe { self.queue.enqueue(token) };
        if status != 0 {
            callback_state()
                .lock()
                .unwrap()
                .inflight
                .remove(&(token.as_ptr() as usize));
            return if status == -12773 {
                Err(CmioSinkError::QueueFull)
            } else {
                Err(CmioSinkError::Platform("enqueue sample", status))
            };
        }
        self.sequence = next_sequence;
        Ok(())
    }

    pub(super) fn has_capacity(&self) -> bool {
        let capacity = unsafe { self.queue.capacity() };
        let count = unsafe { self.queue.count() };
        capacity == QUEUE_CAPACITY as i32 && (0..capacity).contains(&count) && count < capacity
    }

    fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        let stopped = unsafe { CMIODeviceStopStream(self.device, self.stream) } == 0;
        let unregistered = unregister_queue_callback(self.stream).is_ok();
        let reset = stopped && unregistered && unsafe { self.queue.reset() } == 0;
        if reset {
            // Stop/unregister excludes the CMIO reader before this
            // unsynchronized reset. The process-global callback storage
            // remains alive regardless.
            let mut state = callback_state().lock().unwrap();
            if state.owner == Some(self.owner) {
                state.inflight.clear();
                state.owner = None;
            }
        } else {
            // A failed stop/unregister/reset cannot prove that DAL no longer
            // reads queue tokens. Retain both queue and samples and fail
            // closed for this process instead of freeing an in-flight token.
            tracing::error!(
                stopped,
                unregistered,
                reset,
                "CMIO sink teardown could not isolate the output queue; retaining it"
            );
            std::mem::forget(self.queue.clone());
        }
    }
}

fn unregister_queue_callback(stream: u32) -> Result<(), i32> {
    let mut unused = ptr::null_mut();
    let status = unsafe { CMIOStreamCopyBufferQueue(stream, None, ptr::null_mut(), &mut unused) };
    if status != 0 {
        return Err(status);
    }
    if let Some(queue) = NonNull::new(unused) {
        // SAFETY: CopyBufferQueue returned a +1 object used only to
        // unregister the callback; it is released at this scope end.
        drop(unsafe { CFRetained::<CMSimpleQueue>::from_raw(queue) });
    }
    Ok(())
}

impl Drop for MacCmioSink {
    fn drop(&mut self) {
        self.stop();
    }
}

fn make_sample(
    image: &CVPixelBuffer,
    fps: u32,
    sequence: u64,
    discontinuity: bool,
) -> Result<CFRetained<CMSampleBuffer>, CmioSinkError> {
    let mut raw_format = ptr::null();
    checked("create image format", unsafe {
        CMVideoFormatDescriptionCreateForImageBuffer(None, image, NonNull::from(&mut raw_format))
    })?;
    let raw_format = NonNull::new(raw_format.cast_mut()).ok_or(CmioSinkError::InvalidObject)?;
    // SAFETY: The create call returns a +1 format description.
    let format = unsafe { CFRetained::<CMFormatDescription>::from_raw(raw_format) };
    // The sink sample is only the host-to-extension transport envelope. Use
    // the host clock directly so placeholders and source-frame PTS resets can
    // never make this CMIO stream non-monotonic; the source stream owns its
    // independent client-facing SampleClock.
    let presentation_time = unsafe { CMClock::host_time_clock().time() };
    if !presentation_time.flags.contains(CMTimeFlags::Valid) {
        return Err(CmioSinkError::InvalidObject);
    }
    let timing = CMSampleTimingInfo {
        duration: CMTime {
            value: 1,
            timescale: fps as i32,
            flags: CMTimeFlags::Valid,
            epoch: 0,
        },
        presentationTimeStamp: presentation_time,
        decodeTimeStamp: CMTime {
            value: 0,
            timescale: 0,
            flags: CMTimeFlags::empty(),
            epoch: 0,
        },
    };
    let mut raw_sample = ptr::null_mut();
    checked("create CMIO image sample", unsafe {
        CMIOSampleBufferCreateForImageBuffer(
            ptr::null(),
            image,
            CFRetained::<CMFormatDescription>::as_ptr(&format).as_ptr(),
            &timing,
            sequence,
            u32::from(discontinuity),
            &mut raw_sample,
        )
    })?;
    let raw_sample = NonNull::new(raw_sample).ok_or(CmioSinkError::InvalidObject)?;
    // SAFETY: CMIOSampleBufferCreateForImageBuffer returns a +1 sample.
    Ok(unsafe { CFRetained::from_raw(raw_sample) })
}

fn find_device() -> Result<u32, CmioSinkError> {
    let devices = property_u32_vec(
        SYSTEM_OBJECT,
        PropertyAddress {
            selector: PROPERTY_DEVICES,
            scope: SCOPE_GLOBAL,
            element: ELEMENT_MAIN,
        },
    )?;
    for device in devices {
        let Ok(uid) = property_cf::<CFString>(
            device,
            PropertyAddress {
                selector: PROPERTY_DEVICE_UID,
                scope: SCOPE_GLOBAL,
                element: ELEMENT_MAIN,
            },
        ) else {
            continue;
        };
        if uid.to_string() == DEVICE_UID {
            return Ok(device);
        }
    }
    Err(CmioSinkError::DeviceUnavailable)
}

fn property_u32(object: u32, address: PropertyAddress) -> Result<u32, CmioSinkError> {
    let mut value = MaybeUninit::<u32>::uninit();
    read_property_exact(object, address, size_of::<u32>(), value.as_mut_ptr().cast())?;
    // SAFETY: UInt32 has no invalid bit patterns and the helper initialized all bytes.
    Ok(unsafe { value.assume_init() })
}

fn property_f64(object: u32, address: PropertyAddress) -> Result<f64, CmioSinkError> {
    let mut value = MaybeUninit::<f64>::uninit();
    read_property_exact(object, address, size_of::<f64>(), value.as_mut_ptr().cast())?;
    // SAFETY: Float64 has no invalid bit patterns and the helper initialized all bytes.
    Ok(unsafe { value.assume_init() })
}

fn read_property_exact(
    object: u32,
    address: PropertyAddress,
    size: usize,
    destination: *mut c_void,
) -> Result<(), CmioSinkError> {
    let size = u32::try_from(size).map_err(|_| CmioSinkError::InvalidObject)?;
    let mut used = 0_u32;
    checked("read property", unsafe {
        CMIOObjectGetPropertyData(
            object,
            &address,
            0,
            ptr::null(),
            size,
            &mut used,
            destination,
        )
    })?;
    if used != size {
        return Err(CmioSinkError::InvalidObject);
    }
    Ok(())
}

fn property_u32_vec(object: u32, address: PropertyAddress) -> Result<Vec<u32>, CmioSinkError> {
    let mut bytes = 0_u32;
    checked("size property", unsafe {
        CMIOObjectGetPropertyDataSize(object, &address, 0, ptr::null(), &mut bytes)
    })?;
    if bytes == 0 || !(bytes as usize).is_multiple_of(size_of::<u32>()) {
        return Err(CmioSinkError::InvalidObject);
    }
    let count = bytes as usize / size_of::<u32>();
    let mut values = Vec::<u32>::with_capacity(count);
    let mut used = 0_u32;
    checked("read property array", unsafe {
        CMIOObjectGetPropertyData(
            object,
            &address,
            0,
            ptr::null(),
            bytes,
            &mut used,
            values.as_mut_ptr().cast(),
        )
    })?;
    if used != bytes {
        return Err(CmioSinkError::InvalidObject);
    }
    // SAFETY: Both properties routed through this helper are documented as
    // arrays of CMIOObjectID, whose ABI is UInt32.
    unsafe { values.set_len(count) };
    Ok(values)
}

fn property_cf<T>(object: u32, address: PropertyAddress) -> Result<CFRetained<T>, CmioSinkError>
where
    T: objc2_core_foundation::Type,
{
    let mut raw = MaybeUninit::<*mut T>::uninit();
    read_property_exact(
        object,
        address,
        size_of::<*mut T>(),
        raw.as_mut_ptr().cast(),
    )?;
    // SAFETY: Raw pointers have no invalid bit patterns and the property helper
    // initialized exactly one pointer-sized value.
    let raw = unsafe { raw.assume_init() };
    let raw = NonNull::new(raw).ok_or(CmioSinkError::InvalidObject)?;
    // SAFETY: The CMIO property contract gives the caller a +1 CF object.
    Ok(unsafe { CFRetained::from_raw(raw) })
}

fn set_property<T>(object: u32, selector: u32, value: T) -> Result<(), CmioSinkError> {
    let address = PropertyAddress {
        selector,
        scope: SCOPE_GLOBAL,
        element: ELEMENT_MAIN,
    };
    checked("set property", unsafe {
        CMIOObjectSetPropertyData(
            object,
            &address,
            0,
            ptr::null(),
            size_of::<T>() as u32,
            (&value as *const T).cast(),
        )
    })
}

fn checked(operation: &'static str, status: i32) -> Result<(), CmioSinkError> {
    if status == 0 {
        Ok(())
    } else {
        Err(CmioSinkError::Platform(operation, status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmio_constants_match_header_fourcc_values() {
        assert_eq!(PROPERTY_DEVICES, 0x6465_7623);
        assert_eq!(PROPERTY_STREAMS, 0x7374_6d23);
        assert_eq!(PROPERTY_FORMAT, 0x7066_7420);
        assert_eq!(SCOPE_OUTPUT, 0x6f75_7470);
    }
}
