use loom::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

pub fn block_on<F: std::future::Future>(mut f: F) -> F::Output {
    let mut f = unsafe { std::pin::Pin::new_unchecked(&mut f) };
    let park = Arc::new(Park::default());
    let sender = Arc::into_raw(park.clone());
    let raw_waker = RawWaker::new(sender as *const _, &VTABLE);
    let waker = unsafe { Waker::from_raw(raw_waker) };
    let mut cx = Context::from_waker(&waker);

    loop {
        match f.as_mut().poll(&mut cx) {
            Poll::Pending => {
                let mut runnable = park.0.lock().unwrap();
                while !*runnable {
                    runnable = park.1.wait(runnable).unwrap();
                }
                *runnable = false;
            }
            Poll::Ready(val) => return val,
        }
    }
}

struct Park(Mutex<bool>, Condvar);

impl Default for Park {
    fn default() -> Self {
        Park(Mutex::new(false), Condvar::new())
    }
}

impl Park {
    pub fn unpark(&self) {
        *self.0.lock().unwrap() = true;
        self.1.notify_one();
    }
}

static VTABLE: RawWakerVTable = RawWakerVTable::new(
    raw_waker_clone,
    raw_waker_wake,
    raw_waker_wake_by_ref,
    raw_waker_drop,
);

unsafe fn raw_waker_clone(park_ptr: *const ()) -> RawWaker {
    let arc = Arc::from_raw(park_ptr as *const Park);
    std::mem::forget(arc.clone());
    RawWaker::new(Arc::into_raw(arc) as *const (), &VTABLE)
}

unsafe fn raw_waker_wake(park_ptr: *const ()) {
    Arc::from_raw(park_ptr as *const Park).unpark()
}

unsafe fn raw_waker_wake_by_ref(park_ptr: *const ()) {
    (&*(park_ptr as *const Park)).unpark()
}

unsafe fn raw_waker_drop(park_ptr: *const ()) {
    drop(Arc::from_raw(park_ptr as *const Park))
}
