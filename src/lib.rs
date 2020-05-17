use core::mem;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use loom::sync::atomic::{AtomicBool, Ordering};
use loom::sync::Arc;

/// Runs a future to completion
pub fn block_on<F: std::future::Future>(mut f: F) -> F::Output {
    let mut f = unsafe { core::pin::Pin::new_unchecked(&mut f) };
    let loom_waker = Arc::new(LoomWaker::default());
    let waker = {
        let loom_waker_ptr = Arc::into_raw(loom_waker.clone());
        let raw_waker = RawWaker::new(loom_waker_ptr as *const _, &VTABLE);
        unsafe { Waker::from_raw(raw_waker) }
    };
    let mut cx = Context::from_waker(&waker);

    loop {
        match f.as_mut().poll(&mut cx) {
            Poll::Pending => loom_waker.sleep(),
            Poll::Ready(val) => break val,
        }
    }
}

#[derive(Default)]
struct LoomWaker(AtomicBool);

impl LoomWaker {
    pub fn wake(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn sleep(&self) {
        while !self.0.load(Ordering::SeqCst) {
            loom::thread::yield_now();
        }
        self.0.store(false, Ordering::SeqCst);
    }
}

static VTABLE: RawWakerVTable = RawWakerVTable::new(
    raw_waker_clone,
    raw_waker_wake,
    raw_waker_wake_by_ref,
    raw_waker_drop,
);

unsafe fn raw_waker_clone(waker_ptr: *const ()) -> RawWaker {
    let waker = Arc::from_raw(waker_ptr as *const LoomWaker);
    mem::forget(waker.clone());
    RawWaker::new(Arc::into_raw(waker) as *const (), &VTABLE)
}

unsafe fn raw_waker_wake(waker_ptr: *const ()) {
    Arc::from_raw(waker_ptr as *const LoomWaker).wake()
}

unsafe fn raw_waker_wake_by_ref(waker_ptr: *const ()) {
    (&*(waker_ptr as *const LoomWaker)).wake()
}

unsafe fn raw_waker_drop(waker_ptr: *const ()) {
    mem::drop(Arc::from_raw(waker_ptr as *const LoomWaker))
}
