use loom_executor::Runtime;

fn maybe_loom_model(test: impl Fn() + Sync + Send + 'static) {
    #[cfg(loom)]
    loom::model(test);
    #[cfg(not(loom))]
    test();
}

#[test]
fn block_on_unit() {
    maybe_loom_model(|| {
        let _nothing: () = Runtime::new().block_on(async {});
    })
}

#[test]
fn block_on_simple_value() {
    maybe_loom_model(|| {
        let i: u128 = Runtime::new().block_on(async { 95u128 });
        assert_eq!(i, 95);
    })
}

#[test]
fn block_on_spawn() {
    maybe_loom_model(|| {
        eprintln!("==== NEW TEST ====");
        let i: u128 = Runtime::new().block_on(async {
            let task1 = loom_executor::spawn(async { 12u128 });
            //let task2 = loom_executor::spawn(async { 8u128 });
            task1.await.unwrap() // + task2.await.unwrap()
        });
        assert_eq!(i, 12);
        eprintln!("block_on_spawn done!")
    })
}

#[test]
fn send_on_closed_channel() {
    loom::model(|| {
        let (s, r) = loom::sync::mpsc::channel();
        std::mem::drop(r);
        s.send(()).unwrap_err();
    })
}
