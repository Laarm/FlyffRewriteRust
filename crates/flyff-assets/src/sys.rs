//! The one `unsafe` corner of the crate: the best-effort I/O priority of the asset workers
//! (Plan/24 §7.4: `ioprio_set(IOPRIO_WHO_PROCESS, tid, IOPRIO_CLASS_BE | 7)`).
//!
//! `rustix` has no wrapper for `ioprio_set(2)`, so this is a raw `syscall(2)` with integer arguments
//! only. A failure (old kernel, seccomp, non-Linux) is ignored: the CPU niceness already keeps the
//! workers out of the way of the game thread.

#![allow(unsafe_code)]

/// Lowers the I/O priority of the calling thread to best-effort class, level 7 (the lowest).
#[cfg(target_os = "linux")]
pub(crate) fn lower_io_priority() {
    const IOPRIO_WHO_PROCESS: libc::c_long = 1;
    const IOPRIO_CLASS_BE: libc::c_long = 2;
    const IOPRIO_CLASS_SHIFT: libc::c_long = 13;
    const LEVEL: libc::c_long = 7;
    // SAFETY: `ioprio_set(2)` takes three integers and no pointers; `who = 0` means the calling
    // thread. The kernel validates the arguments and reports failure through the return value only.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_ioprio_set,
            IOPRIO_WHO_PROCESS,
            0 as libc::c_long,
            (IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | LEVEL,
        )
    };
    if rc != 0 {
        tracing::debug!(rc, "could not lower asset worker I/O priority");
    }
}

/// No-op on platforms without `ioprio_set`.
#[cfg(not(target_os = "linux"))]
pub(crate) fn lower_io_priority() {}
