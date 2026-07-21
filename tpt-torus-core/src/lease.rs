use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

/// A registered memory region that can be used with I/O operations.
#[derive(Debug, Clone)]
pub struct LeaseRegion {
    /// Start address of the region.
    pub start: usize,
    /// Length in bytes.
    pub len: usize,
    /// Reference count of in-flight operations using this region.
    pub in_flight: u32,
}

impl LeaseRegion {
    /// Returns true if the given address falls within this region.
    pub fn contains(&self, addr: usize) -> bool {
        addr >= self.start && addr < self.start + self.len
    }

    /// Returns true if this region is currently in use by I/O operations.
    pub fn is_in_flight(&self) -> bool {
        self.in_flight > 0
    }
}

/// Registry of memory regions that have been leased to the I/O engine.
///
/// All buffers passed to I/O operations must be registered here first.
/// This prevents invalid/freed pointers from reaching the kernel.
#[derive(Debug)]
pub struct LeaseRegistry {
    regions: RwLock<BTreeMap<usize, LeaseRegion>>,
}

impl LeaseRegistry {
    /// Create a new empty lease registry.
    pub fn new() -> Self {
        Self {
            regions: RwLock::new(BTreeMap::new()),
        }
    }

    /// Register a memory region for use with I/O operations.
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - `ptr` points to a valid, live memory region of at least `len` bytes
    /// - The region is not already registered
    /// - The region will not be freed while registered
    pub unsafe fn register(&self, ptr: *const u8, len: usize) -> Result<(), LeaseError> {
        let start = ptr as usize;
        let mut regions = self.regions.write().unwrap();

        // Check for overlapping regions under the same write lock to prevent TOCTOU race
        let end = start.saturating_add(len);
        if let Some((_addr, existing)) = regions.range(..end).next_back() {
            if existing.start + existing.len > start {
                return Err(LeaseError::Overlap {
                    existing_start: existing.start,
                    existing_len: existing.len,
                });
            }
        }

        regions.insert(
            start,
            LeaseRegion {
                start,
                len,
                in_flight: 0,
            },
        );
        Ok(())
    }

    /// Register a mutable memory region for use with I/O operations.
    ///
    /// # Safety
    ///
    /// Same safety requirements as [`register`](Self::register).
    pub unsafe fn register_mut(&self, ptr: *mut u8, len: usize) -> Result<(), LeaseError> {
        self.register(ptr as *const u8, len)
    }

    /// Unregister a previously registered memory region.
    ///
    /// # Safety
    ///
    /// The caller must ensure no I/O operations are currently using this region.
    pub unsafe fn unregister(&self, ptr: *const u8) -> Result<(), LeaseError> {
        let start = ptr as usize;
        let mut regions = self.regions.write().unwrap();
        match regions.remove(&start) {
            Some(region) => {
                if region.in_flight > 0 {
                    let count = region.in_flight;
                    // Re-insert since we can't unregister an in-flight region
                    regions.insert(start, region);
                    Err(LeaseError::InFlight { count })
                } else {
                    Ok(())
                }
            }
            None => Err(LeaseError::NotRegistered),
        }
    }

    /// Check if a buffer address is registered and mark it as in-flight.
    pub fn checkout(&self, addr: usize, len: usize) -> Result<(), LeaseError> {
        let mut regions = self.regions.write().unwrap();

        // Find the region containing this address
        let region = regions
            .range_mut(..addr + 1)
            .next_back()
            .map(|(_, r)| r)
            .ok_or(LeaseError::NotRegistered)?;

        if !region.contains(addr) {
            return Err(LeaseError::NotRegistered);
        }

        if addr + len > region.start + region.len {
            return Err(LeaseError::OutOfBounds {
                requested_end: addr + len,
                region_end: region.start + region.len,
            });
        }

        region.in_flight += 1;
        Ok(())
    }

    /// Mark a buffer as no longer in-flight after I/O completion.
    pub fn checkin(&self, addr: usize) {
        let mut regions = self.regions.write().unwrap();
        if let Some((_key, region)) = regions.range_mut(..addr + 1).next_back() {
            if region.contains(addr) && region.in_flight > 0 {
                region.in_flight -= 1;
            }
        }
    }

    /// Verify that a buffer is still valid and in-flight.
    ///
    /// Returns `Ok(true)` if the buffer is valid and in-flight.
    /// Returns `Ok(false)` if the buffer is not in-flight (completed or never submitted).
    /// Returns `Err` if the buffer was freed or is invalid.
    pub fn verify(&self, addr: usize, len: usize) -> Result<bool, LeaseError> {
        let regions = self.regions.read().unwrap();
        let region = regions
            .range(..addr + 1)
            .next_back()
            .map(|(_, r)| r)
            .ok_or(LeaseError::NotRegistered)?;

        if !region.contains(addr) {
            return Err(LeaseError::NotRegistered);
        }

        if addr + len > region.start + region.len {
            return Err(LeaseError::OutOfBounds {
                requested_end: addr + len,
                region_end: region.start + region.len,
            });
        }

        Ok(region.in_flight > 0)
    }

    /// Number of registered regions.
    pub fn region_count(&self) -> usize {
        self.regions.read().unwrap().len()
    }

    /// Check if any regions are currently in-flight.
    pub fn has_in_flight(&self) -> bool {
        self.regions
            .read()
            .unwrap()
            .values()
            .any(|r| r.in_flight > 0)
    }
}

impl Default for LeaseRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared handle to a lease registry.
pub type SharedLeaseRegistry = Arc<LeaseRegistry>;

/// Errors that can occur during buffer leasing operations.
#[derive(Debug)]
pub enum LeaseError {
    /// The buffer overlaps with an already-registered region.
    Overlap {
        existing_start: usize,
        existing_len: usize,
    },
    /// The buffer is not registered.
    NotRegistered,
    /// The buffer is currently in-flight and cannot be unregistered.
    InFlight { count: u32 },
    /// The buffer access goes out of the registered region bounds.
    OutOfBounds {
        requested_end: usize,
        region_end: usize,
    },
    /// Buffer lease violation detected — Torus Panic triggered.
    Violation { addr: usize, msg: String },
}

impl std::fmt::Display for LeaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LeaseError::Overlap {
                existing_start,
                existing_len,
            } => write!(
                f,
                "buffer overlaps with registered region at {:#x} (len={})",
                existing_start, existing_len
            ),
            LeaseError::NotRegistered => write!(f, "buffer is not registered with the lease registry"),
            LeaseError::InFlight { count } => {
                write!(f, "buffer is in-flight ({} operations) and cannot be unregistered", count)
            }
            LeaseError::OutOfBounds {
                requested_end,
                region_end,
            } => write!(
                f, "buffer access extends past registered region (requested end={:#x}, region end={:#x})", requested_end, region_end
            ),
            LeaseError::Violation { addr, msg } => {
                write!(f, "Torus Panic at {:#x}: {}", addr, msg)
            }
        }
    }
}

impl std::error::Error for LeaseError {}
