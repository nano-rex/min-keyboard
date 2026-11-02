//! A small fixed-size thread pool for the model's matrix products. Its threads run at background
//! priority so the foreground app stays responsive while a check runs.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

type Job = dyn Fn(usize) + Sync;

/// A job borrowed for the duration of [`Pool::run`], which waits for every thread to finish it.
#[derive(Clone, Copy)]
struct JobPtr(*const Job);
unsafe impl Send for JobPtr {}

#[derive(Default)]
struct State {
    job: Option<JobPtr>,
    generation: u64,
    pending: usize,
    quit: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    start: Condvar,
    done: Condvar,
}

pub struct Pool {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

extern "C" {
    fn setpriority(which: i32, who: u32, priority: i32) -> i32;
}

/// Lowers the calling thread's priority to Android's `THREAD_PRIORITY_BACKGROUND` (nice 10).
/// On Linux and Android `setpriority` with `who = 0` applies to the calling thread only.
pub fn lower_thread_priority() {
    const PRIO_PROCESS: i32 = 0;
    // SAFETY: a plain system call on the current thread.
    unsafe { setpriority(PRIO_PROCESS, 0, 10) };
}

impl Pool {
    /// A pool where [`run`](Pool::run) uses `threads` threads, the caller's included.
    pub fn new(threads: usize) -> Pool {
        let shared = Arc::new(Shared::default());
        let workers = (1..threads.max(1))
            .filter_map(|index| {
                let shared = Arc::clone(&shared);
                std::thread::Builder::new().name("min-keyboard-ai".into()).spawn(move || worker(&shared, index)).ok()
            })
            .collect();
        Pool { shared, workers }
    }

    pub fn threads(&self) -> usize {
        self.workers.len() + 1
    }

    /// Calls `job(thread_index)` once on every thread and returns when all calls have finished.
    pub fn run(&self, job: &(dyn Fn(usize) + Sync)) {
        if self.workers.is_empty() {
            return job(0);
        }
        // SAFETY: only the lifetime is erased; the pointer is not used after this function returns,
        // because it waits until `pending` is zero and clears the job.
        let ptr = JobPtr(unsafe { std::mem::transmute::<&(dyn Fn(usize) + Sync), &'static Job>(job) });
        {
            let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
            state.job = Some(ptr);
            state.generation += 1;
            state.pending = self.workers.len();
        }
        self.shared.start.notify_all();
        let result = catch_unwind(AssertUnwindSafe(|| job(0)));
        let mut state = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        while state.pending > 0 {
            state = self.shared.done.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        state.job = None;
        drop(state);
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    /// Splits `0..count` into chunks of `chunk` and hands them out to the threads.
    pub fn for_chunks(&self, count: usize, chunk: usize, body: &(dyn Fn(usize, std::ops::Range<usize>) + Sync)) {
        let next = AtomicUsize::new(0);
        self.run(&|thread| loop {
            let start = next.fetch_add(chunk, Ordering::Relaxed);
            if start >= count {
                break;
            }
            body(thread, start..(start + chunk).min(count));
        });
    }
}

fn worker(shared: &Shared, index: usize) {
    lower_thread_priority();
    let mut seen = 0;
    loop {
        let job = {
            let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            while !state.quit && state.generation == seen {
                state = shared.start.wait(state).unwrap_or_else(|e| e.into_inner());
            }
            if state.quit {
                return;
            }
            seen = state.generation;
            state.job
        };
        if let Some(JobPtr(job)) = job {
            // SAFETY: `run` keeps the job alive until this thread reports back below.
            let _ = catch_unwind(AssertUnwindSafe(|| unsafe { (*job)(index) }));
        }
        let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        state.pending -= 1;
        if state.pending == 0 {
            shared.done.notify_all();
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap_or_else(|e| e.into_inner()).quit = true;
        self.shared.start.notify_all();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

/// A pointer to an output buffer that threads write to at disjoint indices.
#[derive(Clone, Copy)]
pub struct SyncPtr<T>(pub *mut T);
unsafe impl<T> Send for SyncPtr<T> {}
unsafe impl<T> Sync for SyncPtr<T> {}

impl<T> SyncPtr<T> {
    /// The pointer; a method, so closures capture the whole `Sync` wrapper.
    pub fn get(self) -> *mut T {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_chunk_runs_once() {
        let pool = Pool::new(4);
        for _ in 0..50 {
            let hits: Vec<AtomicUsize> = (0..1000).map(|_| AtomicUsize::new(0)).collect();
            pool.for_chunks(1000, 7, &|_, range| {
                for i in range {
                    hits[i].fetch_add(1, Ordering::Relaxed);
                }
            });
            assert!(hits.iter().all(|h| h.load(Ordering::Relaxed) == 1));
        }
    }
}
