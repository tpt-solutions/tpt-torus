//! Regression test for the async_api.rs completion-matching race.
//!
//! Previously, each `Future`'s `Waiting` poll arm called `Torus::reap` itself
//! and unconditionally treated the first result as its own. With several
//! operations in flight at once, a future could steal a completion that
//! belonged to a different in-flight operation. This test drives many
//! concurrent reads on a single `TorusAsync` and asserts each one gets back
//! exactly the completion that matches its own operation.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake};
use std::time::Duration;

use tpt_torus_core::async_api::TorusAsync;
use tpt_torus_core::backend::Backend;
use tpt_torus_core::flow::Flow;
use tpt_torus_core::operation::Operation;
use tpt_torus_core::{Torus, TorusResult};

/// A backend whose completion value echoes the fd of the submitted read, so a
/// test thread can detect if it received someone else's completion.
struct EchoFdBackend {
    completions: Mutex<Vec<TorusResult>>,
}

impl EchoFdBackend {
    fn new() -> Self {
        Self {
            completions: Mutex::new(Vec::new()),
        }
    }
}

impl Backend for EchoFdBackend {
    fn submit(&self, flows: &[Flow]) -> tpt_torus_core::error::Result<usize> {
        for flow in flows {
            let fd = match flow.operation() {
                Operation::Read { fd, .. } => *fd,
                other => panic!("unexpected operation in test: {other:?}"),
            };
            self.completions
                .lock()
                .unwrap()
                .push(TorusResult::new(fd as i64, flow.user_data()));
        }
        Ok(flows.len())
    }

    fn reap(&self, results: &mut Vec<TorusResult>) -> tpt_torus_core::error::Result<usize> {
        let mut cq = self.completions.lock().unwrap();
        let before = results.len();
        results.append(&mut cq);
        Ok(results.len() - before)
    }

    fn wait(&self, _timeout_us: u64) -> tpt_torus_core::error::Result<()> {
        Ok(())
    }

    fn in_flight(&self) -> u32 {
        0
    }
}

struct ThreadWaker(std::thread::Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = Box::pin(fut);
    let waker = Arc::new(ThreadWaker(std::thread::current())).into();
    let mut cx = Context::from_waker(&waker);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(val) => return val,
            Poll::Pending => std::thread::park_timeout(Duration::from_millis(20)),
        }
    }
}

/// Force `Pin<Box<F>>` to reuse the same `block_on` regardless of future type.
fn pin_box<F: Future + 'static>(fut: F) -> Pin<Box<dyn Future<Output = F::Output>>> {
    Box::pin(fut)
}

#[test]
fn concurrent_reads_each_get_their_own_completion() {
    let torus = Arc::new(Torus::new(256, Box::new(EchoFdBackend::new())).unwrap());
    let async_torus = Arc::new(TorusAsync::from_torus(Arc::clone(&torus)));

    let mismatches = Arc::new(AtomicI64::new(0));
    const N: i32 = 16;

    std::thread::scope(|scope| {
        for fd in 0..N {
            let async_torus = Arc::clone(&async_torus);
            let mismatches = Arc::clone(&mismatches);
            scope.spawn(move || {
                let mut buf = vec![0u8; 8];
                let future = pin_box(async move { async_torus.read(fd, &mut buf, 0).await });
                let result = block_on(future);
                let bytes = result.expect("read future failed") as i32;
                if bytes != fd {
                    mismatches.fetch_add(1, Ordering::SeqCst);
                }
            });
        }
    });

    assert_eq!(
        mismatches.load(Ordering::SeqCst),
        0,
        "one or more futures received a completion that belonged to a different operation"
    );
}
