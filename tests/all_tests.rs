use core::pin::Pin;
use core::task::{Context, Poll};
use loom::sync::atomic::{AtomicBool, Ordering};
use loom::sync::Arc;
use loom::thread;

#[test]
fn block_on_unit() {
    loom::model(|| {
        let _nothing: () = loom_executor::block_on(async {});
    })
}

#[test]
fn block_on_simple_value() {
    loom::model(|| {
        let i: u128 = loom_executor::block_on(async { 95u128 });
        assert_eq!(i, 95);
    })
}

#[test]
fn block_on_sleep() {
    loom::model(|| {
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
            self.thread_spawned = true;
            let done = self.done.clone();
            let waker = cx.waker().clone();
            thread::spawn(move || {
                done.store(true, Ordering::SeqCst);
                waker.wake();
            });
        }
        if self.done.load(Ordering::SeqCst) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
