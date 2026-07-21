//! Cgroup-aware resource limiting for the Virtual Torus.
//!
//! Automatically caps SQ/CQ size and in-flight request count based on
//! container quotas. This mitigates the kernel resource exhaustion threat
//! (see spec.txt Section 5).
//!
//! # How it works
//!
//! On Linux, this reads cgroup v2 memory limits from:
//! - `/sys/fs/cgroup/memory.max` (memory limit)
//! - `/proc/self/cgroup` (cgroup path detection)
//!
//! On other platforms, sensible defaults are used.
//!
//! The limits are advisory — they guide the backend on how many resources
//! to allocate, but don't prevent the application from submitting more work.

use std::sync::atomic::{AtomicU32, Ordering};

/// Resource limits derived from cgroup quotas.
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// Maximum number of SQ/CQ entries.
    pub ring_entries: u32,
    /// Maximum number of in-flight operations.
    pub max_in_flight: u32,
    /// Memory limit in bytes (from cgroup), or None if unlimited.
    pub memory_limit: Option<u64>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self::detect()
    }
}

impl ResourceLimits {
    /// Detect resource limits from the current environment.
    pub fn detect() -> Self {
        let (memory_limit, memory_available) = Self::read_cgroup_memory_limit();

        // Scale ring entries based on available memory.
        // Each SQE is ~64 bytes, each CQE is ~16 bytes.
        // Reserve at least 1KB for overhead.
        let ring_entries = if let Some(available) = memory_available {
            // Use at most 1% of available memory for rings, minimum 256, maximum 4096
            let ring_memory = available / 100;
            let entries = (ring_memory / 80) as u32; // 64 + 16 bytes per entry pair
            entries.clamp(256, 4096).next_power_of_two()
        } else {
            1024 // Default
        };

        // Scale in-flight count based on memory.
        // Each in-flight operation may hold references to buffers.
        let max_in_flight = if let Some(available) = memory_available {
            // Use at most 5% of available memory for in-flight tracking
            let tracking_memory = available / 20;
            let count = (tracking_memory / 128) as u32; // ~128 bytes per in-flight entry
            count.clamp(64, 16384)
        } else {
            1024 // Default
        };

        Self {
            ring_entries,
            max_in_flight,
            memory_limit,
        }
    }

    /// Read the cgroup memory limit on Linux.
    ///
    /// Returns `(limit, usable_memory)` where `usable_memory` accounts for
    /// process overhead.
    fn read_cgroup_memory_limit() -> (Option<u64>, Option<u64>) {
        #[cfg(target_os = "linux")]
        {
            // Try cgroup v2 first
            if let Ok(contents) = std::fs::read_to_string("/sys/fs/cgroup/memory.max") {
                let contents = contents.trim();
                if contents != "max" {
                    if let Ok(limit) = contents.parse::<u64>() {
                        // Account for ~50% overhead (process + kernel overhead)
                        let usable = limit / 2;
                        return (Some(limit), Some(usable));
                    }
                }
            }

            // Try cgroup v1
            if let Ok(contents) =
                std::fs::read_to_string("/sys/fs/cgroup/memory/memory.limit_in_bytes")
            {
                if let Ok(limit) = contents.trim().parse::<u64>() {
                    if limit < u64::MAX / 2 {
                        let usable = limit / 2;
                        return (Some(limit), Some(usable));
                    }
                }
            }

            // No cgroup limit found — use system memory
            if let Ok(contents) = std::fs::read_to_string("/proc/meminfo") {
                for line in contents.lines() {
                    if line.starts_with("MemAvailable:") {
                        if let Some(kb) = line.split_whitespace().nth(1) {
                            if let Ok(kb) = kb.parse::<u64>() {
                                let bytes = kb * 1024;
                                // Use at most 10% of system memory
                                let usable = bytes / 10;
                                return (None, Some(usable));
                            }
                        }
                    }
                }
            }

            (None, None)
        }

        #[cfg(not(target_os = "linux"))]
        {
            (None, None)
        }
    }
}

/// Runtime resource limiter that enforces cgroup-aware limits.
pub struct ResourceLimiter {
    limits: ResourceLimits,
    in_flight: AtomicU32,
}

impl ResourceLimiter {
    /// Create a new resource limiter with auto-detected limits.
    pub fn new() -> Self {
        Self {
            limits: ResourceLimits::detect(),
            in_flight: AtomicU32::new(0),
        }
    }

    /// Create a new resource limiter with custom limits.
    pub fn with_limits(limits: ResourceLimits) -> Self {
        Self {
            limits,
            in_flight: AtomicU32::new(0),
        }
    }

    /// Check if a new operation can be submitted without exceeding limits.
    pub fn can_submit(&self) -> bool {
        self.in_flight.load(Ordering::Relaxed) < self.limits.max_in_flight
    }

    /// Try to reserve a slot for a new operation.
    ///
    /// Returns `true` if the slot was reserved, `false` if the limit would be exceeded.
    pub fn try_reserve(&self) -> bool {
        let current = self.in_flight.load(Ordering::Relaxed);
        if current >= self.limits.max_in_flight {
            return false;
        }
        self.in_flight.fetch_add(1, Ordering::AcqRel) < self.limits.max_in_flight
    }

    /// Release a slot after an operation completes.
    pub fn release(&self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
    }

    /// Number of in-flight operations.
    pub fn in_flight(&self) -> u32 {
        self.in_flight.load(Ordering::Relaxed)
    }

    /// Get the configured limits.
    pub fn limits(&self) -> &ResourceLimits {
        &self.limits
    }

    /// Get the recommended ring size for these limits.
    pub fn recommended_ring_entries(&self) -> u32 {
        self.limits.ring_entries
    }
}

impl Default for ResourceLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_limits_detect() {
        let limits = ResourceLimits::detect();
        assert!(limits.ring_entries.is_power_of_two());
        assert!(limits.ring_entries >= 256);
        assert!(limits.max_in_flight >= 64);
    }

    #[test]
    fn test_resource_limiter() {
        let limiter = ResourceLimiter::with_limits(ResourceLimits {
            ring_entries: 512,
            max_in_flight: 10,
            memory_limit: None,
        });

        assert!(limiter.can_submit());
        assert_eq!(limiter.in_flight(), 0);

        // Reserve slots
        for _ in 0..10 {
            assert!(limiter.try_reserve());
        }
        assert_eq!(limiter.in_flight(), 10);
        assert!(!limiter.can_submit());
        assert!(!limiter.try_reserve());

        // Release slots
        limiter.release();
        assert_eq!(limiter.in_flight(), 9);
        assert!(limiter.can_submit());
    }

    #[test]
    fn test_resource_limiter_default() {
        let limiter = ResourceLimiter::new();
        assert!(limiter.recommended_ring_entries().is_power_of_two());
        assert!(limiter.limits().max_in_flight > 0);
    }
}
