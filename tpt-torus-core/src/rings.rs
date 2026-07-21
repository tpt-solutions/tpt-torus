use crate::result::Result;
use std::sync::atomic::{AtomicU32, Ordering};

// ─── Raw structs matching the kernel io_uring layout ───────────────────────

/// Raw submission queue entry layout matching the kernel struct.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct io_uring_sqe {
    pub opcode: u8,
    pub flags: u8,
    pub ioprio: u16,
    pub fd: i32,
    pub off_addr2: u64,
    pub addr_splice_off_in: u64,
    pub len: u32,
    pub op_flags: u32,
    pub user_data: u64,
    pub buf_group: u16,
    pub personality: u16,
    pub splice_fd_in: i32,
    pub __pad2: [u32; 2],
}

/// Raw completion queue entry layout matching the kernel struct.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct io_uring_cqe {
    pub user_data: u64,
    pub res: i32,
    pub flags: u32,
}

// ─── Submission Ring ───────────────────────────────────────────────────────

/// Virtual Submission Queue — the user-space abstraction over an io_uring SQ.
///
/// On Linux the head/tail pointers share memory with the kernel.
/// On Windows/macOS a background reactor drains this queue.
pub struct SubmissionRing {
    entries: u32,
    #[allow(dead_code)] // Used by non-Linux backends
    sq: Vec<AtomicU32>,
    #[allow(dead_code)] // Used by non-Linux backends
    sqe: Vec<io_uring_sqe>,
    /// Head pointer — advanced by the kernel (or reactor) when consuming entries.
    pub head: Box<AtomicU32>,
    /// Tail pointer — advanced by the application when publishing entries.
    pub tail: Box<AtomicU32>,
}

impl SubmissionRing {
    /// Create a new virtual submission ring with the given capacity (must be power of 2).
    pub fn new(entries: u32) -> Self {
        assert!(
            entries.is_power_of_two(),
            "ring size must be a power of two"
        );
        let mut sq = Vec::with_capacity(entries as usize);
        for _ in 0..entries {
            sq.push(AtomicU32::new(0));
        }
        let mut sqe = Vec::with_capacity(entries as usize);
        for _ in 0..entries {
            sqe.push(io_uring_sqe::default());
        }
        Self {
            entries,
            sq,
            sqe,
            head: Box::new(AtomicU32::new(0)),
            tail: Box::new(AtomicU32::new(0)),
        }
    }

    /// Number of entries in the ring.
    pub fn entries(&self) -> u32 {
        self.entries
    }

    /// How many entries are currently available for submission.
    pub fn free_slots(&self) -> u32 {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        self.entries - (tail.wrapping_sub(head) & (self.entries - 1))
    }

    /// Returns `true` if the ring has room for at least one more entry.
    pub fn has_room(&self) -> bool {
        self.free_slots() > 0
    }

    /// Get a mutable pointer to the SQE at the given index.
    #[allow(dead_code)] // Used by non-Linux backends
    pub(crate) fn sqe_at(&mut self, index: u32) -> &mut io_uring_sqe {
        &mut self.sqe[index as usize]
    }

    /// Publish `count` new entries starting from the current tail.
    pub fn publish(&self, count: u32) -> u32 {
        self.tail.fetch_add(count, Ordering::Release)
    }
}

// ─── Completion Ring ───────────────────────────────────────────────────────

/// Virtual Completion Queue — the user-space abstraction over an io_uring CQE.
///
/// On Linux the head/tail pointers share memory with the kernel.
/// On Windows/macOS a background reactor populates this queue.
pub struct CompletionRing {
    entries: u32,
    cqes: Vec<io_uring_cqe>,
    /// Head pointer — advanced by the application when consuming completions.
    pub head: Box<AtomicU32>,
    /// Tail pointer — advanced by the kernel (or reactor) when publishing completions.
    pub tail: Box<AtomicU32>,
}

impl CompletionRing {
    /// Create a new virtual completion ring with the given capacity (must be power of 2).
    pub fn new(entries: u32) -> Self {
        assert!(
            entries.is_power_of_two(),
            "ring size must be a power of two"
        );
        let mut cqes = Vec::with_capacity(entries as usize);
        for _ in 0..entries {
            cqes.push(io_uring_cqe::default());
        }
        Self {
            entries,
            cqes,
            head: Box::new(AtomicU32::new(0)),
            tail: Box::new(AtomicU32::new(0)),
        }
    }

    /// Number of entries in the ring.
    pub fn entries(&self) -> u32 {
        self.entries
    }

    /// How many completed entries are available to consume.
    pub fn available(&self) -> u32 {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        tail.wrapping_sub(head) & (self.entries - 1)
    }

    /// Returns `true` if there are completions ready to be consumed.
    pub fn has_completions(&self) -> bool {
        self.available() > 0
    }

    /// Peek at the next completion without consuming it.
    pub fn peek(&self) -> Option<Result> {
        if !self.has_completions() {
            return None;
        }
        let head = self.head.load(Ordering::Acquire);
        let index = (head & (self.entries - 1)) as usize;
        let cqe = &self.cqes[index];
        Some(Result::new(cqe.res as i64, cqe.user_data))
    }

    /// Consume the next completion from the ring.
    pub fn consume(&self) -> Option<Result> {
        if !self.has_completions() {
            return None;
        }
        let head = self.head.fetch_add(1, Ordering::AcqRel);
        let index = (head & (self.entries - 1)) as usize;
        let cqe = &self.cqes[index];
        Some(Result::new(cqe.res as i64, cqe.user_data))
    }

    /// Drain all available completions into a vec.
    pub fn drain(&self) -> Vec<Result> {
        let mut results = Vec::new();
        while let Some(r) = self.consume() {
            results.push(r);
        }
        results
    }
}
