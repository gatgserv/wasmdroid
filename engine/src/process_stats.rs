//! Bounded current-process measurements. No PID lookup or authority over another
//! process is exposed. Missing OS measurements remain null, never invented zero.
use crate::driver::{RuntimeStats, StatsScope};

pub fn sample_worker() -> RuntimeStats {
    RuntimeStats {
        scope: StatsScope::WorkerProcess,
        memory_bytes: resident_bytes(),
        cpu_time_ns: clock_ns(true),
        sampled_at_monotonic_ns: clock_ns(false),
    }
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn resident_bytes() -> Option<u64> {
    use std::io::Read;
    let mut bytes = Vec::with_capacity(257);
    std::fs::File::open("/proc/self/statm")
        .ok()?
        .take(257)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 256 {
        return None;
    }
    // SAFETY: sysconf has no pointer arguments or memory ownership effects.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    parse_resident(&bytes, u64::try_from(page_size).ok()?)
}

#[cfg(any(target_os = "android", target_os = "linux", test))]
fn parse_resident(bytes: &[u8], page_size: u64) -> Option<u64> {
    if page_size == 0 {
        return None;
    }
    let mut fields = std::str::from_utf8(bytes).ok()?.split_ascii_whitespace();
    fields.next()?.parse::<u64>().ok()?;
    fields.next()?.parse::<u64>().ok()?.checked_mul(page_size)
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn clock_ns(cpu: bool) -> Option<u64> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let clock = if cpu {
        libc::CLOCK_PROCESS_CPUTIME_ID
    } else {
        libc::CLOCK_MONOTONIC
    };
    // SAFETY: time is a valid writable timespec, and the clock IDs are standard
    // Android/Linux clocks. Failed calls never consume uninitialized output.
    if unsafe { libc::clock_gettime(clock, &mut time) } != 0
        || !(0..1_000_000_000).contains(&time.tv_nsec)
    {
        return None;
    }
    u64::try_from(time.tv_sec)
        .ok()?
        .checked_mul(1_000_000_000)?
        .checked_add(u64::try_from(time.tv_nsec).ok()?)
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
fn resident_bytes() -> Option<u64> {
    None
}
#[cfg(not(any(target_os = "android", target_os = "linux")))]
fn clock_ns(_: bool) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::parse_resident;
    #[test]
    fn uses_current_resident_pages_and_the_actual_page_size() {
        assert_eq!(parse_resident(b"1000 3 1 2 0 8 0\n", 16384), Some(49152));
        assert_eq!(parse_resident(b"1000 0 0", 4096), Some(0));
        assert_eq!(parse_resident(b"1000 3", 0), None);
        assert_eq!(parse_resident(b"1000 -1", 4096), None);
        assert_eq!(parse_resident(b"1000", 4096), None);
        assert_eq!(parse_resident(b"1 18446744073709551615", 4096), None);
    }
}
