//! DPDK (Data Plane Development Kit) integration for user-space networking.
//!
//! DPDK provides a user-space networking stack that bypasses the kernel,
//! enabling high-performance packet processing directly from user space.
//! This module wraps DPDK's core functionality behind safe Rust abstractions.
//!
//! # Architecture
//!
//! ```text
//! Application
//!     │
//!     ▼
//! DpdkPort ──────► Mempool (mbufs)
//!     │                  │
//!     │                  ▼
//!     │            Packet Rings (TX/RX)
//!     │                  │
//!     ▼                  ▼
//! Ethernet NIC ◄──► Poll Mode Driver
//! ```
//!
//! # Feature Flag
//!
//! This module requires the `dpdk` feature flag to be enabled.
//! When disabled, all functions return `HwError::NotAvailable`.

use crate::{HwError, HwResult};

// ─── DPDK FFI bindings ─────────────────────────────────────────────────────

/// Opaque DPDK mempool handle.
pub struct RteMempool {
    _private: [u8; 0],
}

/// Opaque DPDK mbuf handle.
pub struct RteMbuf {
    _private: [u8; 0],
}

/// Opaque DPDK ethernet device handle.
pub struct RteEthDev {
    _private: [u8; 0],
}

/// DPDK mbuf headroom and data layout.
#[repr(C)]
pub struct DpdkMbuf {
    /// Pointer to the segment buffer.
    pub buf_addr: *mut u8,
    /// Data start address (re-adds headroom).
    pub data_off: u16,
    /// Ref count.
    pub refcnt: u16,
    /// NB segs (multi-segment mbuf).
    pub nb_segs: u16,
    /// Port interface.
    pub port: u16,
    /// Packet type flags.
    pub packet_type: u32,
    /// Packet length (data + attached segments).
    pub pkt_len: u32,
    /// Data length.
    pub data_len: u16,
    /// VLAN TCI.
    pub vlan_tci: u16,
    // ... additional fields omitted for brevity
}

// ─── DPDK Mempool ──────────────────────────────────────────────────────────

/// A DPDK memory pool for packet buffers (mbufs).
pub struct Mempool {
    name: String,
    count: usize,
    _pool: *mut RteMempool,
}

unsafe impl Send for Mempool {}
unsafe impl Sync for Mempool {}

impl Mempool {
    /// Create a new mempool.
    ///
    /// # Arguments
    /// - `name`: Name of the mempool.
    /// - `count`: Number of mbufs in the pool.
    /// - `cache_size`: Per-core cache size (0 to disable).
    /// - `priv_size`: Private data size per mbuf.
    /// - `data_room_size`: Data buffer size per mbuf.
    pub fn new(
        _name: &str,
        count: usize,
        cache_size: u32,
        priv_size: u16,
        data_room_size: u16,
    ) -> HwResult<Self> {
        let _ = (count, cache_size, priv_size, data_room_size);

        // In a real implementation, this would call rte_mempool_create_empty()
        // followed by rte_mempool_populate_obj_mem()
        Err(HwError::NotAvailable(
            "DPDK integration requires DPDK to be installed and linked".into(),
        ))
    }

    /// Get the name of the mempool.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Number of mbufs in the pool.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Allocate an mbuf from the pool.
    pub fn alloc(&self) -> Option<*mut DpdkMbuf> {
        // In a real implementation: rte_pktmbuf_alloc(pool)
        None
    }

    /// Return mbufs to the pool.
    pub fn free_bulk(&self, mbufs: &[*mut DpdkMbuf]) {
        // In a real implementation: rte_pktmbuf_free_bulk(pool, mbufs, count)
        let _ = mbufs;
    }

    /// Number of available mbufs.
    pub fn available_count(&self) -> u32 {
        // In a real implementation: rte_mempool_avail_count(pool)
        0
    }
}

impl Drop for Mempool {
    fn drop(&mut self) {
        // In a real implementation: rte_mempool_free(pool)
    }
}

// ─── DPDK Mbuf ─────────────────────────────────────────────────────────────

/// A DPDK packet buffer (mbuf).
pub struct Mbuf {
    ptr: *mut DpdkMbuf,
}

unsafe impl Send for Mbuf {}
unsafe impl Sync for Mbuf {}

impl Mbuf {
    /// Create a new Mbuf wrapper.
    ///
    /// # Safety
    ///
    /// `ptr` must be a valid DPDK mbuf pointer.
    pub unsafe fn new(ptr: *mut DpdkMbuf) -> Self {
        Self { ptr }
    }

    /// Get the raw mbuf pointer.
    pub fn as_ptr(&self) -> *mut DpdkMbuf {
        self.ptr
    }

    /// Get the packet data pointer.
    pub fn data_ptr(&self) -> *mut u8 {
        unsafe { (*self.ptr).buf_addr.add((*self.ptr).data_off as usize) }
    }

    /// Get the packet data length.
    pub fn data_len(&self) -> u16 {
        unsafe { (*self.ptr).data_len }
    }

    /// Get the total packet length (including chained segments).
    pub fn pkt_len(&self) -> u32 {
        unsafe { (*self.ptr).pkt_len }
    }

    /// Get the number of segments.
    pub fn nb_segs(&self) -> u16 {
        unsafe { (*self.ptr).nb_segs }
    }

    /// Append data to the packet.
    pub fn append(&mut self, len: u16) -> HwResult<*mut u8> {
        // In a real implementation: rte_pktmbuf_append(mbuf, len)
        let _ = len;
        Err(HwError::NotAvailable(
            "DPDK integration requires DPDK to be installed and linked".into(),
        ))
    }

    /// Prepend data to the packet.
    pub fn prepend(&mut self, len: u16) -> HwResult<*mut u8> {
        let _ = len;
        Err(HwError::NotAvailable(
            "DPDK integration requires DPDK to be installed and linked".into(),
        ))
    }

    /// Trim data from the end of the packet.
    pub fn trim(&mut self, len: u16) -> HwResult<()> {
        let _ = len;
        Err(HwError::NotAvailable(
            "DPDK integration requires DPDK to be installed and linked".into(),
        ))
    }
}

impl Drop for Mbuf {
    fn drop(&mut self) {
        // Note: We don't free the mbuf here — the caller is responsible
        // for returning it to the mempool. This is consistent with DPDK's
        // manual memory management model.
    }
}

// ─── DPDK Port ─────────────────────────────────────────────────────────────

/// Configuration for a DPDK ethernet port.
pub struct EthConf {
    /// Link speed (0 = auto-negotiate).
    pub link_speed: u32,
    /// Maximum packet length.
    pub max_rx_pkt_len: u32,
    /// Number of RX queues.
    pub nb_rx_queues: u16,
    /// Number of TX queues.
    pub nb_tx_queues: u16,
    /// Offload flags.
    pub offloads: u64,
}

impl Default for EthConf {
    fn default() -> Self {
        Self {
            link_speed: 0,
            max_rx_pkt_len: 1518,
            nb_rx_queues: 1,
            nb_tx_queues: 1,
            offloads: 0,
        }
    }
}

/// A DPDK ethernet port with poll-mode I/O.
pub struct Port {
    port_id: u16,
    #[allow(dead_code)] // Used when DPDK is enabled
    conf: EthConf,
}

unsafe impl Send for Port {}
unsafe impl Sync for Port {}

impl Port {
    /// Open a DPDK ethernet port.
    pub fn open(port_id: u16, conf: EthConf) -> HwResult<Self> {
        let _ = (port_id, &conf);
        Err(HwError::NotAvailable(
            "DPDK integration requires DPDK to be installed and linked".into(),
        ))
    }

    /// Start the port.
    pub fn start(&mut self) -> HwResult<()> {
        Err(HwError::NotAvailable(
            "DPDK integration requires DPDK to be installed and linked".into(),
        ))
    }

    /// Stop the port.
    pub fn stop(&mut self) {
        // In a real implementation: rte_eth_dev_stop(port_id)
    }

    /// Receive packets from the port.
    ///
    /// # Arguments
    /// - `queue_id`: RX queue index.
    /// - `mbufs`: Slice to store received mbuf pointers.
    ///
    /// # Returns
    /// Number of packets received.
    pub fn rx_burst(&self, queue_id: u16, mbufs: &mut [*mut DpdkMbuf]) -> HwResult<u16> {
        let _ = (queue_id, mbufs);
        // In a real implementation: rte_eth_rx_burst(port_id, queue_id, mbufs, count)
        Ok(0)
    }

    /// Transmit packets on the port.
    ///
    /// # Arguments
    /// - `queue_id`: TX queue index.
    /// - `mbufs`: Slice of mbuf pointers to transmit.
    ///
    /// # Returns
    /// Number of packets successfully transmitted.
    pub fn tx_burst(&self, queue_id: u16, mbufs: &[*mut DpdkMbuf]) -> HwResult<u16> {
        let _ = (queue_id, mbufs);
        // In a real implementation: rte_eth_tx_burst(port_id, queue_id, mbufs, count)
        Ok(0)
    }

    /// Get port statistics.
    pub fn stats(&self) -> HwResult<PortStats> {
        // In a real implementation: rte_eth_stats_get(port_id, &stats)
        Ok(PortStats::default())
    }

    /// Get the port ID.
    pub fn port_id(&self) -> u16 {
        self.port_id
    }
}

/// Port statistics.
#[derive(Debug, Default, Clone)]
pub struct PortStats {
    /// Packets received.
    pub rx_packets: u64,
    /// Packets transmitted.
    pub tx_packets: u64,
    /// Bytes received.
    pub rx_bytes: u64,
    /// Bytes transmitted.
    pub tx_bytes: u64,
    /// RX errors.
    pub rx_errors: u64,
    /// TX errors.
    pub tx_errors: u64,
    /// RX missed (dropped) packets.
    pub rx_missed: u64,
    /// RX no-mbuffer errors.
    pub rx_nombuf: u64,
}
