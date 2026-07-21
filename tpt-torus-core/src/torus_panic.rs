use crate::lease::LeaseError;
use std::fmt;

/// A descriptive, safe user-space abort that prevents kernel corruption.
///
/// `TorusPanic` is triggered when a buffer lease violation is detected — for example,
/// when an application attempts to free or modify a buffer while it is still in-flight.
/// Instead of allowing an invalid pointer to reach the kernel (which could cause a
/// kernel panic or privilege escalation), Torus Panic provides a clear, safe abort.
///
/// # Usage
///
/// ```ignore
/// // Torus Panic triggers automatically on lease violation:
/// torus_panic!("buffer freed while in-flight: {:#x}", ptr as usize);
///
/// // Or create one explicitly:
/// let panic = TorusPanic::new("use-after-free detected", Some(addr));
/// panic.trigger();
/// ```
#[derive(Debug)]
pub struct TorusPanic {
    message: String,
    address: Option<usize>,
    backtrace: Option<String>,
}

impl TorusPanic {
    /// Create a new Torus Panic with a descriptive message.
    pub fn new(message: impl Into<String>, address: Option<usize>) -> Self {
        Self {
            message: message.into(),
            address,
            backtrace: None,
        }
    }

    /// Create a Torus Panic from a lease error.
    pub fn from_lease_error(error: &LeaseError) -> Self {
        match error {
            LeaseError::Violation { addr, msg } => {
                Self::new(msg.clone(), Some(*addr))
            }
            LeaseError::NotRegistered => {
                Self::new("buffer is not registered with the lease registry", None)
            }
            LeaseError::InFlight { count } => {
                Self::new(
                    format!("buffer is in-flight ({} operations)", count),
                    None,
                )
            }
            LeaseError::OutOfBounds {
                requested_end,
                region_end,
            } => {
                Self::new(
                    format!(
                        "buffer access extends past registered region (requested end={:#x}, region end={:#x})",
                        requested_end, region_end
                    ),
                    None,
                )
            }
            LeaseError::Overlap {
                existing_start,
                existing_len,
            } => {
                Self::new(
                    format!(
                        "buffer overlaps with registered region at {:#x} (len={})",
                        existing_start, existing_len
                    ),
                    None,
                )
            }
        }
    }

    /// Trigger the Torus Panic.
    ///
    /// This prints a detailed error message and aborts the process.
    /// The panic is designed to be safe — it prevents any invalid pointer
    /// from reaching the kernel.
    pub fn trigger(&self) -> ! {
        eprintln!();
        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║                      TORUS PANIC                           ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");
        eprintln!("║ A buffer lease violation was detected.                     ║");
        eprintln!("║ The I/O engine has been safely aborted to prevent          ║");
        eprintln!("║ kernel corruption.                                         ║");
        eprintln!("╚══════════════════════════════════════════════════════════════╝");
        eprintln!();
        eprintln!("  Message: {}", self.message);
        if let Some(addr) = self.address {
            eprintln!("  Address: {:#x}", addr);
        }
        if let Some(ref bt) = self.backtrace {
            eprintln!("  Backtrace: {}", bt);
        }
        eprintln!();
        eprintln!("  This is a safety feature of TPT Torus. The buffer that");
        eprintln!("  caused this panic must have been freed or modified while");
        eprintln!("  still in-flight (i.e., submitted to the I/O engine).");
        eprintln!();
        eprintln!("  To fix this:");
        eprintln!("    1. Ensure buffers are not freed until all I/O completes");
        eprintln!("    2. Use the Buffer Leasing API to register memory regions");
        eprintln!("    3. Check the return value of submit() before reusing buffers");
        eprintln!();

        std::process::abort();
    }

    /// Get the panic message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Get the faulting address, if any.
    pub fn address(&self) -> Option<usize> {
        self.address
    }
}

impl fmt::Display for TorusPanic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Torus Panic: {}", self.message)?;
        if let Some(addr) = self.address {
            write!(f, " at {:#x}", addr)?;
        }
        Ok(())
    }
}

impl std::error::Error for TorusPanic {}

/// Macro to trigger a Torus Panic with a formatted message.
///
/// # Example
///
/// ```ignore
/// torus_panic!("buffer freed while in-flight: {:#x}", ptr as usize);
/// ```
#[macro_export]
macro_rules! torus_panic {
    ($msg:expr $(, $arg:expr)*) => {{
        let panic = $crate::torus_panic::TorusPanic::new(
            format!($msg $(, $arg)*),
            None,
        );
        panic.trigger();
    }};
    (addr: $addr:expr, $msg:expr $(, $arg:expr)*) => {{
        let panic = $crate::torus_panic::TorusPanic::new(
            format!($msg $(, $arg)*),
            Some($addr),
        );
        panic.trigger();
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_torus_panic_display() {
        let panic = TorusPanic::new("test violation", Some(0xdeadbeef));
        assert_eq!(panic.message(), "test violation");
        assert_eq!(panic.address(), Some(0xdeadbeef));
        assert_eq!(
            format!("{}", panic),
            "Torus Panic: test violation at 0xdeadbeef"
        );
    }

    #[test]
    fn test_torus_panic_from_lease_error() {
        let error = LeaseError::NotRegistered;
        let panic = TorusPanic::from_lease_error(&error);
        assert_eq!(
            panic.message(),
            "buffer is not registered with the lease registry"
        );
        assert_eq!(panic.address(), None);
    }
}
