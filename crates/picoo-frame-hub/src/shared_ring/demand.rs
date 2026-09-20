//! CPU output demand is a bounded host-clock lease — REQ-PICOO-FRAME-014.
use super::layout::const_meta_at;
use std::sync::atomic::Ordering;

const LEASE_MS: u64 = 250;

fn live(deadline_ms: u64, now_ms: u64) -> bool {
    deadline_ms > now_ms && deadline_ms - now_ms <= LEASE_MS
}

pub(super) unsafe fn is_requested(base: *const u8) -> bool {
    let meta = &*const_meta_at(base);
    if meta.content_generation.load(Ordering::SeqCst) == 0 {
        return false;
    }
    host_milliseconds().is_some_and(|now| {
        live(
            (&*const_meta_at(base))
                .cpu_demand_until_ms
                .load(Ordering::SeqCst),
            now,
        )
    })
}

pub(super) unsafe fn sequence(base: *const u8) -> Option<u64> {
    if !is_requested(base) {
        return None;
    }
    let sequence = (&*const_meta_at(base))
        .cpu_request_sequence
        .load(Ordering::SeqCst);
    (sequence != 0).then_some(sequence)
}

pub(super) unsafe fn request(base: *const u8) {
    if (&*const_meta_at(base))
        .content_generation
        .load(Ordering::SeqCst)
        == 0
    {
        return;
    }
    if let Some(deadline) = host_milliseconds().and_then(|now| now.checked_add(LEASE_MS)) {
        (&*const_meta_at(base))
            .cpu_demand_until_ms
            .store(deadline, Ordering::SeqCst);
        // Exhaustion permanently stops new requests; never reuse an old ticket.
        let _ = (&*const_meta_at(base)).cpu_request_sequence.fetch_update(
            Ordering::SeqCst,
            Ordering::SeqCst,
            |sequence| sequence.checked_add(1),
        );
    }
}

#[cfg(unix)]
fn host_milliseconds() -> Option<u64> {
    let mut time = std::mem::MaybeUninit::<libc::timespec>::uninit();
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, time.as_mut_ptr()) } != 0 {
        return None;
    }
    let time = unsafe { time.assume_init() };
    u64::try_from(time.tv_sec)
        .ok()?
        .checked_mul(1_000)?
        .checked_add(u64::try_from(time.tv_nsec).ok()? / 1_000_000)
}

#[cfg(windows)]
fn host_milliseconds() -> Option<u64> {
    Some(unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() })
}

#[cfg(not(any(unix, windows)))]
fn host_milliseconds() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_or_unbounded_demand_is_not_live() {
        assert!(!live(0, 1));
        assert!(!live(1_000, 1_000));
        assert!(live(1_250, 1_000));
        assert!(!live(1_251, 1_000));
        assert!(!live(u64::MAX, 1_000));
    }
}
