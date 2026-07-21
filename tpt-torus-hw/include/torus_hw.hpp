/// @file torus_hw.hpp
/// @brief C++20 header for TPT Torus Hardware Bypass (SPDK, DPDK, GPU-Direct).
///
/// This header provides a C++ interface to the hardware bypass layer,
/// enabling user-space storage and networking I/O with direct hardware access.
///
/// # Requirements
/// - C++20 or later
/// - SPDK installed (for NVMe operations)
/// - DPDK installed (for networking operations)
/// - CUDA installed (for GPU-Direct operations)

#pragma once

#include <cstdint>
#include <cstring>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <system_error>
#include <vector>

#ifdef _WIN32
#define TORUS_HW_API __declspec(dllexport)
#else
#define TORUS_HW_API __attribute__((visibility("default")))
#endif

namespace torus {
namespace hw {

// ─── SPDK Types ────────────────────────────────────────────────────────────

/// NVMe namespace handle (user-space I/O).
class NvmeNamespace {
public:
    /// Block size in bytes.
    [[nodiscard]] uint32_t block_size() const noexcept { return m_block_size; }

    /// Total number of blocks.
    [[nodiscard]] uint64_t total_blocks() const noexcept { return m_total_blocks; }

    /// Namespace size in bytes.
    [[nodiscard]] uint64_t size_bytes() const noexcept {
        return m_total_blocks * m_block_size;
    }

    /// Submit an async read.
    /// @param lba Starting logical block address.
    /// @param num_blocks Number of blocks to read.
    /// @param buf DMA-capable buffer.
    /// @param callback Completion callback (result, user_data).
    void read(uint64_t lba, uint32_t num_blocks, void* buf,
              std::function<void(int64_t, uint64_t)> callback,
              uint64_t user_data = 0);

    /// Submit an async write.
    void write(uint64_t lba, uint32_t num_blocks, const void* buf,
               std::function<void(int64_t, uint64_t)> callback,
               uint64_t user_data = 0);

    /// Submit an async flush.
    void flush(std::function<void(int64_t, uint64_t)> callback,
               uint64_t user_data = 0);

private:
    uint32_t m_block_size = 0;
    uint64_t m_total_blocks = 0;
};

/// NVMe command builder.
struct NvmeCmd {
    uint8_t opcode = 0;
    uint8_t flags = 0;
    uint32_t ns_id = 0;
    uint32_t cdw10 = 0;
    uint32_t cdw11 = 0;
    uint32_t cdw12 = 0;
    uint32_t cdw13 = 0;
    uint64_t cdw14 = 0;
    uint64_t cdw15 = 0;

    /// Create a read command.
    static NvmeCmd read(uint64_t lba, uint32_t num_blocks,
                        uint64_t prp1, uint64_t prp2, uint32_t ns_id) {
        NvmeCmd cmd;
        cmd.opcode = 0x02;
        cmd.ns_id = ns_id;
        cmd.cdw10 = static_cast<uint32_t>(lba & 0xFFFFFFFF);
        cmd.cdw11 = static_cast<uint32_t>((lba >> 32) & 0xFFFFFFFF);
        cmd.cdw12 = num_blocks - 1;
        cmd.cdw14 = prp1;
        cmd.cdw15 = prp2;
        return cmd;
    }

    /// Create a write command.
    static NvmeCmd write(uint64_t lba, uint32_t num_blocks,
                         uint64_t prp1, uint64_t prp2, uint32_t ns_id) {
        NvmeCmd cmd;
        cmd.opcode = 0x01;
        cmd.ns_id = ns_id;
        cmd.cdw10 = static_cast<uint32_t>(lba & 0xFFFFFFFF);
        cmd.cdw11 = static_cast<uint32_t>((lba >> 32) & 0xFFFFFFFF);
        cmd.cdw12 = num_blocks - 1;
        cmd.cdw14 = prp1;
        cmd.cdw15 = prp2;
        return cmd;
    }
};

// ─── DPDK Types ────────────────────────────────────────────────────────────

/// DPDK packet buffer (mbuf).
class Mbuf {
public:
    /// Packet data pointer.
    [[nodiscard]] const uint8_t* data() const noexcept { return m_data; }
    [[nodiscard]] uint8_t* data() noexcept { return m_data; }

    /// Packet data length.
    [[nodiscard]] uint16_t data_len() const noexcept { return m_data_len; }

    /// Total packet length (including chained segments).
    [[nodiscard]] uint32_t pkt_len() const noexcept { return m_pkt_len; }

    /// Append data to the packet.
    /// @return Pointer to the appended data, or nullptr on failure.
    uint8_t* append(uint16_t len);

    /// Prepend data to the packet.
    uint8_t* prepend(uint16_t len);

    /// Trim data from the end.
    bool trim(uint16_t len);

private:
    uint8_t* m_data = nullptr;
    uint16_t m_data_len = 0;
    uint32_t m_pkt_len = 0;
};

/// DPDK memory pool for packet buffers.
class Mempool {
public:
    /// Create a new mempool.
    /// @param name Pool name.
    /// @param count Number of mbufs.
    /// @param cache_size Per-core cache size (0 to disable).
    /// @param priv_size Private data size per mbuf.
    /// @param data_room_size Data buffer size per mbuf.
    /// @throws std::runtime_error if creation fails.
    Mempool(const std::string& name, size_t count,
            uint32_t cache_size = 256,
            uint16_t priv_size = 0,
            uint16_t data_room_size = 2048);

    /// Get the pool name.
    [[nodiscard]] const std::string& name() const noexcept { return m_name; }

    /// Number of mbufs in the pool.
    [[nodiscard]] size_t count() const noexcept { return m_count; }

    /// Allocate an mbuf.
    [[nodiscard]] std::unique_ptr<Mbuf> alloc();

    /// Return mbufs to the pool.
    void free_bulk(std::vector<Mbuf*>& mbufs);

private:
    std::string m_name;
    size_t m_count;
};

/// DPDK ethernet port with poll-mode I/O.
class Port {
public:
    /// Port configuration.
    struct Config {
        uint32_t link_speed = 0;        // 0 = auto-negotiate
        uint32_t max_rx_pkt_len = 1518;
        uint16_t nb_rx_queues = 1;
        uint16_t nb_tx_queues = 1;
        uint64_t offloads = 0;
    };

    /// Open a port.
    /// @param port_id Port index.
    /// @param conf Port configuration.
    /// @throws std::runtime_error if port cannot be opened.
    Port(uint16_t port_id, const Config& conf = Config{});

    /// Start the port.
    void start();

    /// Stop the port.
    void stop();

    /// Receive packets.
    /// @param queue_id RX queue index.
    /// @param max_packets Maximum packets to receive.
    /// @return Vector of received mbufs.
    [[nodiscard]] std::vector<std::unique_ptr<Mbuf>>
    rx_burst(uint16_t queue_id, uint16_t max_packets = 32);

    /// Transmit packets.
    /// @param queue_id TX queue index.
    /// @param mbufs Packets to transmit.
    /// @return Number of packets successfully transmitted.
    [[nodiscard]] size_t tx_burst(uint16_t queue_id,
                                  std::vector<std::unique_ptr<Mbuf>>& mbufs);

    /// Get port statistics.
    struct Stats {
        uint64_t rx_packets = 0;
        uint64_t tx_packets = 0;
        uint64_t rx_bytes = 0;
        uint64_t tx_bytes = 0;
        uint64_t rx_errors = 0;
        uint64_t tx_errors = 0;
    };

    [[nodiscard]] Stats stats() const;

    /// Get the port ID.
    [[nodiscard]] uint16_t port_id() const noexcept { return m_port_id; }

private:
    uint16_t m_port_id;
};

// ─── GPU-Direct Types ──────────────────────────────────────────────────────

/// Direction of a DMA transfer.
enum class TransferDirection {
    NvmeToGpu,  ///< NVMe → GPU VRAM
    GpuToNvme,  ///< GPU VRAM → NVMe
    GpuToGpu,   ///< GPU VRAM → GPU VRAM
};

/// DMA scatter-gather entry.
struct DmaEntry {
    uint64_t src;   ///< Source physical address.
    uint64_t dst;   ///< Destination physical address.
    uint32_t len;   ///< Transfer length in bytes.
    uint32_t flags; ///< Flags.
};

/// Handle to a GPU memory region.
class GpuBuffer {
public:
    /// @param device_id GPU device ID.
    /// @param dev_ptr Device-side virtual address.
    /// @param size Buffer size in bytes.
    GpuBuffer(int device_id, uint64_t dev_ptr, size_t size);

    [[nodiscard]] int device_id() const noexcept { return m_device_id; }
    [[nodiscard]] uint64_t dev_ptr() const noexcept { return m_dev_ptr; }
    [[nodiscard]] size_t size() const noexcept { return m_size; }
    [[nodiscard]] bool can_fit(size_t len) const noexcept { return len <= m_size; }

private:
    int m_device_id;
    uint64_t m_dev_ptr;
    size_t m_size;
};

/// DMA transfer status.
enum class DmaStatus {
    Completed,
    InProgress,
    Failed,
    Cancelled,
};

/// High-level GPU-Direct orchestrator.
class GpuDirect {
public:
    /// Create a new GPU-Direct orchestrator.
    /// @param device_id GPU device ID.
    /// @param num_engines Number of DMA engines.
    explicit GpuDirect(int device_id, size_t num_engines = 4);

    [[nodiscard]] int device_id() const noexcept { return m_device_id; }
    [[nodiscard]] size_t engine_count() const noexcept { return m_engines.size(); }

    /// Transfer data from NVMe to GPU VRAM.
    /// @param nvme_lba NVMe logical block address.
    /// @param gpu_buf Target GPU buffer.
    /// @param offset Offset within the GPU buffer.
    /// @param len Transfer length in bytes.
    /// @return Transfer ID.
    uint64_t nvme_to_gpu(uint64_t nvme_lba, const GpuBuffer& gpu_buf,
                         size_t offset, size_t len);

    /// Transfer data from GPU VRAM to NVMe.
    uint64_t gpu_to_nvme(const GpuBuffer& gpu_buf, size_t offset,
                         size_t len, uint64_t nvme_lba);

    /// Transfer data between GPU buffers.
    uint64_t gpu_to_gpu(const GpuBuffer& src, size_t src_offset,
                        const GpuBuffer& dst, size_t dst_offset,
                        size_t len);

    /// Wait for all transfers to complete.
    void sync_all();

    /// Get transfer statistics.
    struct Stats {
        size_t total_engines;
        size_t active_transfers;
    };

    [[nodiscard]] Stats stats() const;

private:
    int m_device_id;
    std::vector<size_t> m_engines; // active transfer counts per engine
};

} // namespace hw
} // namespace torus
