//! Benchmarks for the Hardware Bypass layer.
//!
//! Run with: `cargo bench -p tpt-torus-hw --features gpu_direct`

use criterion::{criterion_group, Criterion};

/// Benchmark: NVMe command builder overhead.
fn bench_nvme_cmd(c: &mut Criterion) {
    use tpt_torus_hw::spdk::NvmeCmd;

    let mut group = c.benchmark_group("nvme_cmd");

    group.bench_function("cmd_new", |b| {
        b.iter(NvmeCmd::new);
    });

    group.bench_function("cmd_read", |b| {
        b.iter(|| NvmeCmd::read(100, 8, 0x1000, 0x2000, 1));
    });

    group.bench_function("cmd_write", |b| {
        b.iter(|| NvmeCmd::write(200, 4, 0x3000, 0x4000, 1));
    });

    group.bench_function("cmd_flush", |b| {
        b.iter(|| NvmeCmd::flush(1));
    });

    group.bench_function("cmd_deallocate", |b| {
        b.iter(|| NvmeCmd::deallocate(0, 1000, 1));
    });

    group.finish();
}

/// Benchmark: DMA pool operations.
fn bench_dma_pool(c: &mut Criterion) {
    use tpt_torus_hw::spdk::DmaPool;

    let mut group = c.benchmark_group("dma_pool");

    group.bench_function("alloc_free", |b| {
        let mut buf = vec![0u8; 4096 * 64];
        let mut pool;
        unsafe {
            pool = DmaPool::new(buf.as_mut_ptr(), 4096 * 64, 4096, 64);
        }
        b.iter(|| {
            let ptr = pool.alloc().unwrap();
            pool.free(ptr);
        });
    });

    group.bench_function("alloc_batch", |b| {
        let mut buf = vec![0u8; 4096 * 64];
        let mut pool;
        unsafe {
            pool = DmaPool::new(buf.as_mut_ptr(), 4096 * 64, 4096, 64);
        }
        b.iter(|| {
            let mut ptrs = Vec::with_capacity(32);
            for _ in 0..32 {
                if let Some(p) = pool.alloc() {
                    ptrs.push(p);
                }
            }
            for p in ptrs {
                pool.free(p);
            }
        });
    });

    group.finish();
}

/// Benchmark: GPU-Direct buffer and entry creation.
#[cfg(feature = "gpu_direct")]
fn bench_gpu_direct(c: &mut Criterion) {
    use tpt_torus_hw::gpu_direct::{DmaEntry, GpuBuffer, TransferDirection};

    let mut group = c.benchmark_group("gpu_direct");

    group.bench_function("buffer_from_raw", |b| {
        b.iter(|| GpuBuffer::from_raw(0, 0x10000000, 1024 * 1024));
    });

    group.bench_function("buffer_can_fit", |b| {
        let buf = GpuBuffer::from_raw(0, 0x10000000, 1024 * 1024);
        b.iter(|| buf.can_fit(512));
    });

    group.bench_function("dma_entry_new", |b| {
        b.iter(|| DmaEntry {
            src: 0x1000,
            dst: 0x2000,
            len: 4096,
            flags: 0,
        });
    });

    group.bench_function("engine_new", |b| {
        b.iter(|| {
            let _ = tpt_torus_hw::gpu_direct::DmaEngine::new(0, 1024);
        });
    });

    group.finish();
}

/// Benchmark: TransferDirection display formatting.
#[cfg(feature = "gpu_direct")]
fn bench_transfer_direction(c: &mut Criterion) {
    use tpt_torus_hw::gpu_direct::TransferDirection;

    let mut group = c.benchmark_group("transfer_direction");

    group.bench_function("display", |b| {
        b.iter(|| format!("{}", TransferDirection::NvmeToGpu));
    });

    group.finish();
}

/// Benchmark: Pipelined zero-copy path vs naive copy-based path.
///
/// This compares two approaches for NVMe → GPU data transfer:
///
/// **Pipelined (zero-copy)**:
/// ```text
/// NVMe ──[io_uring READ]──► DMA Pool Buffer ──[CUDA H2D]──► GPU VRAM
/// ```
/// CPU never touches the data. Two DMA stages that can overlap.
///
/// **Naive (copy-based)**:
/// ```text
/// NVMe ──[io_uring READ]──► Stack Buffer ──[memcpy]──► GPU VRAM
/// ```
/// CPU copies data twice: once from kernel to stack, once from stack to GPU.
#[cfg(unix)]
fn bench_pipelined_vs_naive(c: &mut Criterion) {
    use tpt_torus_hw::spdk::DmaPool;

    let mut group = c.benchmark_group("pipelined_vs_naive");
    group.sample_size(100);

    // Test with different transfer sizes
    for &size in &[4096usize, 65536, 1048576] {
        let size_label = if size >= 1048576 {
            format!("{}MB", size / 1048576)
        } else if size >= 1024 {
            format!("{}KB", size / 1024)
        } else {
            format!("{}B", size)
        };

        // ── Pipelined path: DMA pool allocation (no actual I/O) ──
        group.bench_function(format!("{}_pipelined_alloc", size_label), |b| {
            let mut buf = vec![0u8; size * 64];
            let mut pool;
            unsafe {
                pool = DmaPool::new(buf.as_mut_ptr(), size * 64, size, 64);
            }
            b.iter(|| {
                let ptr = pool.alloc().unwrap();
                // Simulate: kernel DMA writes directly to pool buffer
                // (no CPU copy — this is the zero-copy stage)
                pool.free(ptr);
            });
        });

        // ── Naive path: stack buffer + memcpy ──
        group.bench_function(format!("{}_naive_stack_copy", size_label), |b| {
            let mut stack_buf = vec![0u8; size];
            let mut gpu_buf = vec![0u8; size];
            b.iter(|| {
                // Simulate: io_uring reads into stack buffer (kernel → user copy)
                unsafe {
                    std::ptr::write_bytes(stack_buf.as_mut_ptr(), 0xAA, size);
                }
                // Simulate: memcpy from stack to GPU (CPU copy)
                gpu_buf.copy_from_slice(&stack_buf);
                std::hint::black_box(&gpu_buf);
            });
        });

        // ── Pipelined path: full simulation ──
        group.bench_function(format!("{}_pipelined_full", size_label), |b| {
            let mut buf = vec![0u8; size * 64];
            let mut pool;
            unsafe {
                pool = DmaPool::new(buf.as_mut_ptr(), size * 64, size, 64);
            }

            b.iter(|| {
                // Stage 1: Allocate DMA pool buffer (zero-copy, no CPU involvement)
                let pool_buf = pool.alloc().unwrap();

                // Stage 2: Simulate kernel DMA to pool buffer
                // In real code, this is NVMe → pool buffer via PCIe DMA
                unsafe {
                    std::ptr::write_bytes(pool_buf, 0xBB, size);
                }

                // Stage 3: Simulate CUDA async memcpy (pool → GPU)
                // In real code, this is cudaMemcpyAsync H2D

                pool.free(pool_buf);
            });
        });

        // ── Naive path: explicit memcpy overhead ──
        group.bench_function(format!("{}_naive_memcpy", size_label), |b| {
            let mut src = vec![0u8; size];
            let mut dst = vec![0u8; size];
            b.iter(|| {
                // Simulate: kernel → userspace copy (read syscall)
                unsafe {
                    std::ptr::write_bytes(src.as_mut_ptr(), 0xCC, size);
                }
                // Simulate: userspace → GPU copy (memcpy + CUDA)
                dst.copy_from_slice(&src);
                std::hint::black_box(&dst);
            });
        });
    }

    group.finish();
}

/// Benchmark: DMA pool vs stack allocation overhead.
fn bench_allocation_overhead(c: &mut Criterion) {
    use tpt_torus_hw::spdk::DmaPool;

    let mut group = c.benchmark_group("allocation_overhead");

    group.bench_function("dma_pool_alloc_4k", |b| {
        let mut buf = vec![0u8; 4096 * 256];
        let mut pool;
        unsafe {
            pool = DmaPool::new(buf.as_mut_ptr(), 4096 * 256, 4096, 256);
        }
        b.iter(|| {
            let ptr = pool.alloc().unwrap();
            pool.free(ptr);
        });
    });

    group.bench_function("stack_alloc_4k", |b| {
        b.iter(|| {
            let buf = vec![0u8; 4096];
            std::hint::black_box(&buf);
        });
    });

    group.bench_function("dma_pool_alloc_64k", |b| {
        let mut buf = vec![0u8; 65536 * 64];
        let mut pool;
        unsafe {
            pool = DmaPool::new(buf.as_mut_ptr(), 65536 * 64, 65536, 64);
        }
        b.iter(|| {
            let ptr = pool.alloc().unwrap();
            pool.free(ptr);
        });
    });

    group.bench_function("stack_alloc_64k", |b| {
        b.iter(|| {
            let buf = vec![0u8; 65536];
            std::hint::black_box(&buf);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_nvme_cmd,
    bench_dma_pool,
    bench_allocation_overhead,
);

#[cfg(feature = "gpu_direct")]
criterion_group!(gpu_benches, bench_gpu_direct, bench_transfer_direction,);

#[cfg(unix)]
criterion_group!(unix_benches, bench_pipelined_vs_naive,);

fn main() {
    benches();
    #[cfg(feature = "gpu_direct")]
    gpu_benches();
    #[cfg(unix)]
    unix_benches();
}
