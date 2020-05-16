//! A loom aware async executor.
//!
//! This library is an async Rust executor that can be made [loom] aware. This executor is very
//! simple. It just spawns a thread for each future spawned with the `spawn` function.
//!
//! If built without the `loom` cfg enabled this library does not use loom in any way. But when
//! built with `--cfg loom` it uses primitives from loom to implement the executor. This makes
//! it possible to write tests for your `Future`s and have loom try all possible
//!
//! [loom] https://crates.io/crates/loom

#![feature(thread_id_value)]

use core::cell::RefCell;
use core::fmt;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use core::mem;

#[cfg(loom)]
use loom as std_or_loom;
#[cfg(not(loom))]
use std as std_or_loom;

use std_or_loom::sync::atomic::{AtomicBool, Ordering};
use std_or_loom::sync::{Condvar, Mutex};
use std_or_loom::thread;

std_or_loom::thread_local!(static RUNTIME: RefCell<Option<*const Runtime>> = Default::default());

pub struct Runtime {
    threads: Mutex<Vec<thread::JoinHandle<()>>>,
    cancelled: AtomicBool,
    condvar: Condvar,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            threads: Mutex::new(Vec::new()),
            cancelled: AtomicBool::new(false),
            condvar: Condvar::new(),
        }
    }
}

impl Runtime {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn block_on<F: Future>(&mut self, future: F) -> F::Output {
        RUNTIME.with(|rt| rt.replace(Some(self)));
        let result = run_future(future).expect("Runtime cancelled inside block_on");
        eprintln!("Cancelling runtime");
        // Cancel this runtime and wait for all threads to exit.
        let mut threads = self.threads.lock().unwrap();
        self.cancelled.store(true, Ordering::SeqCst);
        self.condvar.notify_all();
        eprintln!("Joining {} threads", threads.len());
        for thread in threads.drain(..) {
            eprintln!("block_on: joining thread");
            thread.join().unwrap();
            eprintln!("block_on: thread joined");
        }
        result
    }
}

/// Spawns a new asynchronous task, returning a JoinHandle for it.
///
/// This function must be called from the context of a loom executor runtime. Tasks running on the
/// loom executor runtime are always inside its context.
pub fn spawn<T>(task: T) -> JoinHandle<T::Output>
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
{
    let runtime: &Runtime =
        RUNTIME.with(|rt| unsafe { &*rt.borrow().expect("Spawn not called from runtime thread") });
    let (result_sender, result_receiver) = oneshot::channel();
    // If the runtime is not cancelled, spawn a thread running the task.
    let mut runtime_threads = runtime.threads.lock().unwrap();
    if !runtime.cancelled.load(Ordering::SeqCst) {
        eprintln!("{} spawn: Spawning worker thread", tid());
        let thread = thread::spawn(move || {
            eprintln!("{} worker thread: spawned", tid());
            RUNTIME.with(|rt| rt.replace(Some(runtime)));
            if let Some(result) = run_future(task) {
                let _ = result_sender.send(result);
            }
            eprintln!("{} worker thread: exiting", tid());
        });
        // Insert the new thread's handle into the list
        runtime_threads.push(thread);
    }
    JoinHandle { result_receiver }
}

pub struct JoinHandle<T> {
    result_receiver: oneshot::Receiver<T>,
}

impl<T> Future for JoinHandle<T> {
    type Output = Result<T, JoinError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Future::poll(Pin::new(&mut self.result_receiver), cx).map_err(|_| JoinError)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct JoinError;

impl fmt::Display for JoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        "runtime cancelled".fmt(f)
    }
}

impl std::error::Error for JoinError {}

fn run_future<F: Future>(mut future: F) -> Option<F::Output> {
    let runtime: &Runtime = RUNTIME.with(|rt| unsafe {
        &*rt.borrow()
            .expect("run_future not called from runtime thread")
    });
    let waker = unsafe { new_waker(&runtime.condvar) };
    let mut context = Context::from_waker(&waker);
    while !runtime.cancelled.load(Ordering::SeqCst) {
        let pinned_future = unsafe { Pin::new_unchecked(&mut future) };
        if let Poll::Ready(value) = Future::poll(pinned_future, &mut context) {
            eprintln!("{} run_future: Returning yielded value", tid());
            return Some(value);
        }
        eprintln!("{} run_future: Locking threads mutex", tid());
        let threads = runtime.threads.lock().unwrap();
        eprintln!("{} run_future: Locked threads mutex. Waiting on condvar", tid());
        mem::drop(runtime.condvar.wait(threads).unwrap());
        eprintln!("{} run_future: Woke up from condvar wait", tid());
    }
    None
}

unsafe fn new_waker(condvar: &Condvar) -> Waker {
    Waker::from_raw(RawWaker::new(condvar as *const _ as *const _, &VTABLE))
}

const VTABLE: RawWakerVTable = RawWakerVTable::new(
    raw_waker_clone,
    raw_waker_wake,
    raw_waker_wake_by_ref,
    raw_waker_drop,
);

unsafe fn raw_waker_clone(condvar_ptr: *const ()) -> RawWaker {
    eprintln!("{} raw_waker_clone", tid());
    RawWaker::new(condvar_ptr, &VTABLE)
}

unsafe fn raw_waker_wake(condvar_ptr: *const ()) {
    eprintln!("{} raw_waker_wake", tid());
    let condvar = &*(condvar_ptr as *const Condvar);
    condvar.notify_all();
}

unsafe fn raw_waker_wake_by_ref(condvar_ptr: *const ()) {
    eprintln!("{} raw_waker_wake_by_ref", tid());
    let condvar = &*(condvar_ptr as *const Condvar);
    condvar.notify_all();
}

unsafe fn raw_waker_drop(_condvar_ptr: *const ()) {
    eprintln!("{} raw_waker_drop", tid());
}

fn tid() -> u64 {
    std::thread::current().id().as_u64().get()
}
