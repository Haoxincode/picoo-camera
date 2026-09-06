use super::*;
use crate::windows::tests::{diagnostic_context, Runtime};
use std::sync::Mutex;
use std::time::Duration;
use windows::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_READ, D3D11_SUBRESOURCE_DATA, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
};
use windows::Win32::System::Threading::{GetCurrentProcess, SetEvent};

static TEST_LOCK: Mutex<()> = Mutex::new(());

struct Owner(mpsc::SyncSender<()>);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.try_send(());
    }
}

unsafe fn duplicate_event(event: HANDLE) -> Event {
    let mut duplicate = HANDLE::default();
    DuplicateHandle(
        GetCurrentProcess(),
        event,
        GetCurrentProcess(),
        &mut duplicate,
        0,
        false,
        DUPLICATE_SAME_ACCESS,
    )
    .unwrap();
    Event(duplicate)
}

#[test]
fn cancelled_receiver_retains_owner_until_the_native_event() {
    let _serial = TEST_LOCK.lock().unwrap();
    let _runtime = Runtime::start();
    let gpu = diagnostic_context();
    let (released, observed) = mpsc::sync_channel(1);
    // Controlled event tests the real OS callback's ownership independently of
    // GPU speed. The separate copy test exercises actual Flush1 completion.
    let (pending, completion) = unsafe { Pending::prepare(gpu.clone(), Owner(released)) }.unwrap();
    let event = unsafe { duplicate_event(pending.event.0) };
    drop(completion);
    unsafe {
        Pending::arm(pending);
    }
    assert!(matches!(
        observed.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert_eq!(gpu.completion_slots.load(Ordering::Acquire), 1);
    unsafe {
        SetEvent(event.0).unwrap();
    }
    observed
        .recv_timeout(Duration::from_secs(3))
        .expect("callback did not release the cancelled owner");
    // The owner's destructor reports before the enclosing delivery releases its
    // permit. Wait for that final native callback cleanup before another test.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while gpu.completion_slots.load(Ordering::Acquire) != 0 && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(gpu.completion_slots.load(Ordering::Acquire), 0);
}

#[test]
fn completed_but_unconsumed_results_keep_the_capacity_reservation() {
    let _serial = TEST_LOCK.lock().unwrap();
    let _runtime = Runtime::start();
    let gpu = diagnostic_context();
    let mut completions = Vec::new();
    for _ in 0..CONTEXT_LIMIT {
        completions.push(unsafe { gpu.submit_owned((), |_, _| Ok(())) }.unwrap());
    }
    let rejected = unsafe {
        gpu.submit_owned((), |_, _| {
            panic!("capacity must be checked before submission")
        })
    };
    assert!(matches!(rejected, Err(WindowsCompletionError::Capacity)));
    for completion in completions {
        completion
            .received
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .result
            .unwrap();
    }
    assert_eq!(gpu.completion_slots.load(Ordering::Acquire), 0);
    unsafe { gpu.submit_owned((), |_, _| Ok(())) }
        .unwrap()
        .received
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
        .result
        .unwrap();
}

#[test]
fn panicking_submission_still_finishes_and_releases_its_owner() {
    let _serial = TEST_LOCK.lock().unwrap();
    let _runtime = Runtime::start();
    let gpu = diagnostic_context();
    let (released, observed) = mpsc::sync_channel(1);
    let completion =
        unsafe { gpu.submit_owned(Owner(released), |_, _| panic!("submission fixture")) }.unwrap();
    let delivery = completion
        .received
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    assert!(matches!(
        delivery.result,
        Err(WindowsCompletionError::SubmissionPanicked)
    ));
    observed.recv_timeout(Duration::from_secs(3)).unwrap();
    drop(delivery);
    assert_eq!(gpu.completion_slots.load(Ordering::Acquire), 0);
}

#[test]
fn rebuilding_contexts_cannot_bypass_the_process_limit() {
    let _serial = TEST_LOCK.lock().unwrap();
    let contexts: Vec<_> = (0..5).map(|_| Arc::new(AtomicUsize::new(0))).collect();
    let mut retained = Vec::new();
    for context in &contexts[..4] {
        for _ in 0..CONTEXT_LIMIT {
            retained.push(Permit::acquire(context).unwrap());
        }
    }
    assert!(matches!(
        Permit::acquire(&contexts[4]),
        Err(WindowsCompletionError::Capacity)
    ));
    retained.pop();
    let replacement = Permit::acquire(&contexts[4]).unwrap();
    drop(retained);
    drop(replacement);
}

struct CopyBuffers {
    source: ID3D11Buffer,
    target: ID3D11Buffer,
}
// SAFETY: D3D11 buffers are free-threaded; the completion keeps exclusive
// ownership and the test does not map the target before completion.
unsafe impl Send for CopyBuffers {}

#[test]
fn flush_event_completes_actual_gpu_copy_before_delivery() {
    let _serial = TEST_LOCK.lock().unwrap();
    let _runtime = Runtime::start();
    let gpu = diagnostic_context();
    let expected = [0x29_u8; 256];
    unsafe {
        let mut source = None;
        gpu.device
            .CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: expected.len() as u32,
                    Usage: D3D11_USAGE_DEFAULT,
                    ..Default::default()
                },
                Some(&D3D11_SUBRESOURCE_DATA {
                    pSysMem: expected.as_ptr().cast(),
                    ..Default::default()
                }),
                Some(&mut source),
            )
            .unwrap();
        let mut target = None;
        gpu.device
            .CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: expected.len() as u32,
                    Usage: D3D11_USAGE_STAGING,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut target),
            )
            .unwrap();
        let completion = gpu
            .submit_owned(
                CopyBuffers {
                    source: source.unwrap(),
                    target: target.unwrap(),
                },
                |gpu, buffers| {
                    gpu.with_immediate_context(|context| {
                        context.CopyResource(&buffers.target, &buffers.source)
                    });
                    Ok(())
                },
            )
            .unwrap();
        let buffers = completion
            .received
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .result
            .unwrap();
        // Diagnostic-only readback validates the submitted GPU operation.
        gpu.with_immediate_context(|context| {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context
                .Map(&buffers.target, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .unwrap();
            let actual = std::slice::from_raw_parts(mapped.pData.cast::<u8>(), expected.len());
            let matches = actual == expected;
            context.Unmap(&buffers.target, 0);
            assert!(matches);
        });
    }
}
