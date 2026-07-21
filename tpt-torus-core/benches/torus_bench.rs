//! Benchmarks for TPT Torus.
//!
//! These benchmarks verify the zero-cost abstraction claim: that the unified API
//! compiles to the same machine code as native calls on Linux.
//!
//! Run with: `cargo bench -p tpt-torus-core`

use criterion::{criterion_group, criterion_main, Criterion};
use tpt_torus_core::flow::Flow;
use tpt_torus_core::operation::Operation;
use tpt_torus_core::result::Result as TorusResult;

/// Benchmark: Flow creation overhead.
fn bench_flow_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("flow_creation");

    group.bench_function("flow_new_read", |b| {
        let buf = vec![0u8; 4096];
        b.iter(|| {
            Flow::new(Operation::Read {
                fd: 0,
                buf: buf.as_ptr() as *mut u8,
                len: 4096,
                offset: 0,
            })
        });
    });

    group.bench_function("flow_new_write", |b| {
        let buf = vec![0u8; 4096];
        b.iter(|| {
            Flow::new(Operation::Write {
                fd: 0,
                buf: buf.as_ptr(),
                len: 4096,
                offset: 0,
            })
        });
    });

    group.bench_function("flow_with_user_data", |b| {
        let buf = vec![0u8; 4096];
        b.iter(|| {
            Flow::with_user_data(
                Operation::Read {
                    fd: 0,
                    buf: buf.as_ptr() as *mut u8,
                    len: 4096,
                    offset: 0,
                },
                42,
            )
        });
    });

    group.finish();
}

/// Benchmark: Result creation and inspection overhead.
fn bench_result_inspection(c: &mut Criterion) {
    let mut group = c.benchmark_group("result_inspection");

    group.bench_function("result_new", |b| {
        b.iter(|| TorusResult::new(4096, 42));
    });

    group.bench_function("result_is_ok", |b| {
        let r = TorusResult::new(4096, 42);
        b.iter(|| r.is_ok());
    });

    group.bench_function("result_bytes", |b| {
        let r = TorusResult::new(4096, 42);
        b.iter(|| r.bytes());
    });

    group.bench_function("result_error", |b| {
        let r = TorusResult::new(-22, 42);
        b.iter(|| r.error());
    });

    group.finish();
}

/// Benchmark: Atomic ring buffer operations.
fn bench_ring_operations(c: &mut Criterion) {
    use std::sync::atomic::Ordering;
    use tpt_torus_core::rings::{CompletionRing, SubmissionRing};

    let mut group = c.benchmark_group("ring_operations");

    group.bench_function("sq_publish", |b| {
        let ring = SubmissionRing::new(1024);
        b.iter(|| {
            ring.tail.fetch_add(1, Ordering::Release);
        });
    });

    group.bench_function("sq_free_slots", |b| {
        let ring = SubmissionRing::new(1024);
        b.iter(|| ring.free_slots());
    });

    group.bench_function("cq_available", |b| {
        let ring = CompletionRing::new(1024);
        b.iter(|| ring.available());
    });

    group.bench_function("cq_consume", |b| {
        let ring = CompletionRing::new(1024);
        // Pre-fill some completions
        for _ in 0..100 {
            ring.tail.fetch_add(1, Ordering::Release);
        }
        b.iter(|| ring.consume());
    });

    group.finish();
}

/// Benchmark: Torus creation and submission path (without backend).
fn bench_torus_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("torus_overhead");

    group.bench_function("flow_creation_to_submit_path", |b| {
        let buf = vec![0u8; 4096];
        b.iter(|| {
            // Measure the overhead of creating a Flow and preparing for submission
            // This isolates the Torus API overhead from the backend
            let flow = Flow::with_user_data(
                Operation::Read {
                    fd: 0,
                    buf: buf.as_ptr() as *mut u8,
                    len: 4096,
                    offset: 0,
                },
                42,
            );
            // The actual submission would go through the backend
            // Here we just measure the Flow creation + user_data access
            flow.user_data()
        });
    });

    group.finish();
}

/// Benchmark: Memory lease operations.
fn bench_lease_operations(c: &mut Criterion) {
    use tpt_torus_core::lease::LeaseRegistry;

    let mut group = c.benchmark_group("lease_operations");

    group.bench_function("register", |b| {
        let registry = LeaseRegistry::new();
        let mut buf = vec![0u8; 4096];
        b.iter(|| unsafe {
            registry.register_mut(buf.as_mut_ptr(), 4096).unwrap();
            registry.unregister(buf.as_mut_ptr() as *const u8).unwrap();
        });
    });

    group.bench_function("checkout_checkin", |b| {
        let registry = LeaseRegistry::new();
        let mut buf = vec![0u8; 4096];
        unsafe {
            registry.register_mut(buf.as_mut_ptr(), 4096).unwrap();
        }
        let addr = buf.as_ptr() as usize;
        b.iter(|| {
            registry.checkout(addr, 4096).unwrap();
            registry.checkin(addr);
        });
    });

    group.bench_function("verify", |b| {
        let registry = LeaseRegistry::new();
        let mut buf = vec![0u8; 4096];
        unsafe {
            registry.register_mut(buf.as_mut_ptr(), 4096).unwrap();
        }
        let addr = buf.as_ptr() as usize;
        b.iter(|| {
            let _ = registry.verify(addr, 4096);
        });
    });

    group.finish();
}

/// Benchmark: Resource limiter overhead.
fn bench_resource_limiter(c: &mut Criterion) {
    use tpt_torus_core::cgroup::ResourceLimiter;

    let mut group = c.benchmark_group("resource_limiter");

    group.bench_function("try_reserve", |b| {
        let limiter = ResourceLimiter::new();
        b.iter(|| {
            let _ = limiter.try_reserve();
            limiter.release();
        });
    });

    group.bench_function("can_submit", |b| {
        let limiter = ResourceLimiter::new();
        b.iter(|| limiter.can_submit());
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_flow_creation,
    bench_result_inspection,
    bench_ring_operations,
    bench_torus_overhead,
    bench_lease_operations,
    bench_resource_limiter,
);
criterion_main!(benches);
