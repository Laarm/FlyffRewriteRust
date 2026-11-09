//! Small shared helpers.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Locks `m`, ignoring poisoning: every critical section in this crate leaves its data consistent
/// (counters and maps updated in one step), and a builder panic is caught before it can unwind through
/// a held guard, so a poisoned lock never means a half-written state.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A readable message from a `catch_unwind` payload.
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

/// Maps `f` over `items` on up to `max_threads` scoped worker threads and returns the results in input
/// order (the output never depends on the scheduling). Used by the builders that read thousands of small
/// files in one job (model bounds, motion table); `f` must not panic on bad input (builders never do).
pub(crate) fn par_map<T: Sync, R: Send>(
    items: &[T],
    max_threads: usize,
    f: impl Fn(&T) -> R + Sync,
) -> Vec<R> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let threads = max_threads.min(items.len()).max(1);
    if threads == 1 {
        return items.iter().map(&f).collect();
    }
    let next = AtomicUsize::new(0);
    let mut parts: Vec<Vec<(usize, R)>> = Vec::with_capacity(threads);
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut local = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(i) else { break };
                        local.push((i, f(item)));
                    }
                    local
                })
            })
            .collect();
        for h in handles {
            match h.join() {
                Ok(v) => parts.push(v),
                // A worker panicked: re-raise it on this thread (the pool's catch_unwind handles it).
                Err(p) => std::panic::resume_unwind(p),
            }
        }
    });
    let mut flat: Vec<(usize, R)> = parts.into_iter().flatten().collect();
    flat.sort_unstable_by_key(|(i, _)| *i);
    flat.into_iter().map(|(_, r)| r).collect()
}

/// The worker count of [`par_map`] inside one job: the available cores, at most 8.
pub(crate) fn job_threads() -> usize {
    std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(8)
}
