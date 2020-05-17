use core::pin::Pin;
use core::task::{Context, Poll};

#[cfg(loom)]
use loom::{sync::Arc, thread};
#[cfg(not(loom))]
use std::{sync::Arc, thread};

#[cfg(not(loom))]
use core::sync::atomic::{AtomicBool, Ordering};
#[cfg(loom)]
use loom::sync::atomic::{AtomicBool, Ordering};

fn maybe_loom_model(test: impl Fn() + Sync + Send + 'static) {
    #[cfg(loom)]
    loom::model(test);
    #[cfg(not(loom))]
    test();
}

#[test]
fn block_on_unit() {
    maybe_loom_model(|| {
        let _nothing: () = loom_executor::block_on(async {});
    })
}

#[test]
fn block_on_simple_value() {
    maybe_loom_model(|| {
        let i: u128 = loom_executor::block_on(async { 95u128 });
        assert_eq!(i, 95);
    })
}

#[test]
fn block_on_sleep() {
    maybe_loom_model(|| {
        let i: u128 = loom_executor::block_on(async {
            Delay::default().await;
            96u128
        });
        assert_eq!(i, 96);
    })
}

#[derive(Default)]
struct Delay {
    thread_spawned: bool,
    done: Arc<AtomicBool>,
}

impl std::future::Future for Delay {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        if !self.thread_spawned {
            let done = self.done.clone();
            let waker = cx.waker().clone();
            thread::spawn(move || {
                #[cfg(not(loom))]
                thread::sleep(core::time::Duration::from_millis(100));
                #[cfg(loom)]
                thread::yield_now();

                done.store(true, Ordering::SeqCst);
                waker.wake();
            });
            self.thread_spawned = true;
        }
        if self.done.load(Ordering::SeqCst) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
