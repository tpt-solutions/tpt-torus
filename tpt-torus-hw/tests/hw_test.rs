//! Tests for the hardware bypass crate.

#![cfg(feature = "gpu_direct")]

/// Test that SPDK types compile and have correct sizes.
#[test]
fn test_spdk_nvme_cmd_size() {
    let cmd = tpt_torus_hw::spdk::NvmeCmd::new();
    assert_eq!(cmd.opcode, 0);
    assert_eq!(cmd.flags, 0);
}

/// Test NVMe command builders.
#[test]
fn test_spdk_nvme_cmd_read() {
    let cmd = tpt_torus_hw::spdk::NvmeCmd::read(100, 8, 0x1000, 0x2000, 1);
    assert_eq!(cmd.opcode, 0x02);
    assert_eq!(cmd.ns_id, 1);
    assert_eq!(cmd.cdw12, 7);
    assert_eq!(cmd.cdw14, 0x1000);
    assert_eq!(cmd.cdw15, 0x2000);
}

/// Test NVMe command builders.
#[test]
fn test_spdk_nvme_cmd_write() {
    let cmd = tpt_torus_hw::spdk::NvmeCmd::write(200, 4, 0x3000, 0x4000, 1);
    assert_eq!(cmd.opcode, 0x01);
    assert_eq!(cmd.cdw12, 3);
}

/// Test DPDK Mempool creation (should return NotAvailable).
#[test]
fn test_dpdk_mempool_not_available() {
    let result = tpt_torus_hw::dpdk::Mempool::new("test", 1024, 256, 0, 2048);
    assert!(result.is_err());
    match result {
        Err(tpt_torus_hw::HwError::NotAvailable(_)) => {}
        _ => panic!("expected NotAvailable error"),
    }
}

/// Test GPU-Direct buffer creation from raw pointer.
#[test]
fn test_gpu_direct_buffer() {
    let buf = tpt_torus_hw::gpu_direct::GpuBuffer::from_raw(0, 0x10000000, 1024 * 1024);
    assert_eq!(buf.device_id, 0);
    assert_eq!(buf.dev_ptr, 0x10000000);
    assert_eq!(buf.size, 1024 * 1024);
    assert!(buf.can_fit(512));
    assert!(buf.can_fit(1024 * 1024));
    assert!(!buf.can_fit(1024 * 1024 + 1));
}

/// Test GPU-Direct DMA engine creation (may fail without CUDA).
#[test]
fn test_dma_engine_creation() {
    // DmaEngine::new now calls CUDA stream_create, which requires CUDA
    // This test verifies the API compiles and handles CUDA absence gracefully
    let result = tpt_torus_hw::gpu_direct::DmaEngine::new(0, 2);
    // On systems without CUDA, this should return an error
    // On systems with CUDA, it should succeed
    match result {
        Ok(engine) => {
            assert!(engine.has_capacity());
            assert_eq!(engine.active_count(), 0);
        }
        Err(tpt_torus_hw::HwError::NotAvailable(_)) => {
            // Expected on systems without CUDA
        }
        Err(tpt_torus_hw::HwError::InitFailed(_)) => {
            // Expected when CUDA is not initialized
        }
        Err(e) => panic!("unexpected error: {}", e),
    }
}

/// Test DMA entry creation.
#[test]
fn test_dma_entry() {
    let entry = tpt_torus_hw::gpu_direct::DmaEntry {
        src: 0x1000,
        dst: 0x2000,
        len: 4096,
        flags: 0,
    };
    assert_eq!(entry.src, 0x1000);
    assert_eq!(entry.dst, 0x2000);
    assert_eq!(entry.len, 4096);
}

/// Test GPU-Direct orchestrator creation (may fail without CUDA).
#[test]
fn test_gpu_direct_orchestrator() {
    let result = tpt_torus_hw::gpu_direct::GpuDirect::new(0, 4);
    match result {
        Ok(gd) => {
            assert_eq!(gd.device_id(), 0);
            assert_eq!(gd.engine_count(), 4);
            let stats = gd.stats();
            assert_eq!(stats.total_engines, 4);
            assert_eq!(stats.active_transfers, 0);
        }
        Err(tpt_torus_hw::HwError::NotAvailable(_)) => {
            // Expected on systems without CUDA
        }
        Err(tpt_torus_hw::HwError::InitFailed(_)) => {
            // Expected on systems without CUDA
        }
        Err(e) => panic!("unexpected error: {}", e),
    }
}

/// Test TransferDirection display.
#[test]
fn test_transfer_direction_display() {
    use tpt_torus_hw::gpu_direct::TransferDirection;
    assert_eq!(format!("{}", TransferDirection::NvmeToGpu), "NVMe → GPU");
    assert_eq!(format!("{}", TransferDirection::GpuToNvme), "GPU → NVMe");
    assert_eq!(format!("{}", TransferDirection::GpuToGpu), "GPU → GPU");
}

/// Test GPU-Direct nvme_to_gpu with invalid buffer.
#[test]
fn test_gpu_direct_buffer_overflow() {
    let mut gd = match tpt_torus_hw::gpu_direct::GpuDirect::new(0, 1) {
        Ok(gd) => gd,
        Err(_) => return, // Skip test if CUDA not available
    };
    let buf = tpt_torus_hw::gpu_direct::GpuBuffer::from_raw(0, 0x10000000, 1024);

    let result = gd.nvme_to_gpu(0, 1, &buf, 512, 1024);
    assert!(result.is_err());
    match result {
        Err(tpt_torus_hw::HwError::InvalidParam(_)) => {}
        _ => panic!("expected InvalidParam error"),
    }
}

/// Test DmaPool creation and operations.
#[test]
fn test_dma_pool() {
    let mut buf = vec![0u8; 4096 * 8];
    let mut pool;
    unsafe {
        pool = tpt_torus_hw::spdk::DmaPool::new(buf.as_mut_ptr(), 4096 * 8, 4096, 8);
    }

    assert_eq!(pool.available(), 8);
    assert_eq!(pool.total(), 8);

    let b1 = pool.alloc().unwrap();
    let b2 = pool.alloc().unwrap();
    assert_eq!(pool.available(), 6);

    pool.free(b1);
    pool.free(b2);
    assert_eq!(pool.available(), 8);
}

/// Test NVMe passthrough command creation.
#[cfg(target_os = "linux")]
#[test]
fn test_nvme_passthrough_cmd() {
    use tpt_torus_hw::nvme_gpu::NvmePassthroughCmd;

    let read_cmd = NvmePassthroughCmd::read(1000, 8, 1);
    assert_eq!(read_cmd.opcode, 0x02);
    assert_eq!(read_cmd.ns_id, 1);
    assert_eq!(read_cmd.cdw12, 7); // NLB is 0-based

    let write_cmd = NvmePassthroughCmd::write(2000, 16, 1);
    assert_eq!(write_cmd.opcode, 0x01);
    assert_eq!(write_cmd.cdw12, 15);
}

/// Test GpuDirectNvme creation (may fail without NVMe device).
#[cfg(target_os = "linux")]
#[test]
fn test_gpu_direct_nvme_creation() {
    use tpt_torus_hw::nvme_gpu::GpuDirectNvme;

    // This test verifies the API compiles and handles missing devices gracefully
    let result = GpuDirectNvme::new("/dev/nvme0n1", 1);
    match result {
        Ok(_nvme) => {
            // System has an NVMe device
        }
        Err(tpt_torus_hw::HwError::InitFailed(_)) => {
            // Expected on systems without NVMe or without permissions
        }
        Err(tpt_torus_hw::HwError::NotAvailable(_)) => {
            // Expected on systems without io_uring
        }
        Err(e) => panic!("unexpected error: {}", e),
    }
}

/// Test ZeroCopyDmaPool builder configuration.
#[cfg(all(unix, feature = "gpu_direct"))]
#[test]
fn test_zero_copy_dma_pool_builder() {
    use tpt_torus_hw::dma_pool::DmaPoolBuilder;

    // Test builder API (can't build without io_uring fd)
    let builder = DmaPoolBuilder::new(0)
        .buf_size(8192)
        .capacity(128)
        .use_hugepages();

    assert_eq!(builder.buf_size, 8192);
    assert_eq!(builder.capacity, 128);
    assert!(builder.use_hugepages);
}

/// Test BufferDesc layout.
#[cfg(all(unix, feature = "gpu_direct"))]
#[test]
fn test_buffer_desc_layout() {
    use tpt_torus_hw::dma_pool::BufferDesc;

    let desc = BufferDesc {
        index: 0,
        addr: 0x1000,
        dma_addr: 0x2000,
        size: 4096,
        allocated: false,
    };

    assert_eq!(desc.index, 0);
    assert_eq!(desc.addr, 0x1000);
    assert_eq!(desc.dma_addr, 0x2000);
    assert_eq!(desc.size, 4096);
    assert!(!desc.allocated);
}

/// Test PoolStats tracking.
#[cfg(all(unix, feature = "gpu_direct"))]
#[test]
fn test_pool_stats() {
    use tpt_torus_hw::dma_pool::PoolStats;

    let stats = PoolStats::default();
    assert_eq!(stats.in_flight(), 0);
    assert_eq!(stats.total_allocs(), 0);

    stats.record_alloc();
    assert_eq!(stats.in_flight(), 1);
    assert_eq!(stats.total_allocs(), 1);

    stats.record_alloc();
    assert_eq!(stats.in_flight(), 2);
    assert_eq!(stats.peak_in_flight(), 2);

    stats.record_dealloc();
    assert_eq!(stats.in_flight(), 1);
    assert_eq!(stats.peak_in_flight(), 2);

    stats.record_dealloc();
    assert_eq!(stats.in_flight(), 0);
    assert_eq!(stats.peak_in_flight(), 2);
}
