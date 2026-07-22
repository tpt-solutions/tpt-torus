//! Linux io_uring backend: maps the virtual SQ/CQ directly to kernel shared memory.
#![cfg(target_os = "linux")]

use tpt_torus_core::backend::Backend;
use tpt_torus_core::flow::Flow;
use tpt_torus_core::operation::Operation;
use tpt_torus_core::result::Result as TorusResult;
use tpt_torus_sys::{io_uring_cqe, io_uring_params, io_uring_sqe, opcodes, queue_exit, queue_init};

use std::ptr;
use std::sync::atomic::{fence, AtomicU32, Ordering};

/// Memory-mapped kernel ring pointers.
struct MmapRing {
    head: *mut AtomicU32,
    tail: *mut AtomicU32,
    ring_mask: *const u32,
    ring_entries: *const u32,
    array: *mut u32,
    len: usize,
}

impl MmapRing {
    fn entries(&self) -> u32 {
        unsafe { *self.ring_entries }
    }

    fn mask(&self) -> u32 {
        unsafe { *self.ring_mask }
    }
}

impl Drop for MmapRing {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.head as *mut libc::c_void, self.len);
        }
    }
}

/// Backend that maps the Virtual Torus rings directly to io_uring kernel shared memory.
pub struct UringBackend {
    fd: i32,
    /// SQE array (kernel reads from here).
    sqe_array: *mut io_uring_sqe,
    sqe_array_len: usize,
    /// Mmap'd SQ ring (head/tail shared with kernel).
    sq_ring: MmapRing,
    /// Mmap'd CQ ring (head/tail shared with kernel).
    cq_ring: MmapRing,
    /// CQE array pointer (kernel writes CQEs here).
    cqes: *mut io_uring_cqe,
    cqes_len: usize,
    in_flight: AtomicU32,
}

unsafe impl Send for UringBackend {}
unsafe impl Sync for UringBackend {}

impl UringBackend {
    /// Create a new io_uring backend with the given ring capacity.
    pub fn new(entries: u32) -> Result<Self, &'static str> {
        let mut params: io_uring_params = unsafe { std::mem::zeroed() };
        let fd = queue_init(entries, &mut params)?;

        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;

        let sq_entries = params.sq_entries;
        let cq_entries = params.cq_entries;
        let sq_mask = sq_entries - 1;
        let cq_mask = cq_entries - 1;

        // mmap SQ ring: head + tail + ring_mask + ring_entries + flags + dropped + array[]
        let sq_ring_size =
            (params.sq_off.array as usize) + (sq_entries as usize) * std::mem::size_of::<u32>();
        let sq_ring_size = (sq_ring_size + page_size - 1) & !(page_size - 1);

        let sq_ring_ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                sq_ring_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_POPULATE,
                fd,
                0, // SQ ring offset is always 0
            )
        };
        if sq_ring_ptr == libc::MAP_FAILED {
            queue_exit(fd);
            return Err("failed to mmap SQ ring");
        }

        let sq_ring = unsafe {
            let base = sq_ring_ptr as *const u8;
            MmapRing {
                head: base.add(params.sq_off.head as usize) as *mut AtomicU32,
                tail: base.add(params.sq_off.tail as usize) as *mut AtomicU32,
                ring_mask: base.add(params.sq_off.ring_mask as usize) as *const u32,
                ring_entries: base.add(params.sq_off.ring_entries as usize) as *const u32,
                array: base.add(params.sq_off.array as usize) as *mut u32,
                len: sq_ring_size,
            }
        };

        // mmap SQE array
        let sqe_array_size = (sq_entries as usize) * std::mem::size_of::<io_uring_sqe>();
        let sqe_array_size = (sqe_array_size + page_size - 1) & !(page_size - 1);
        let sqe_array_offset =
            (params.sq_off.array as usize) + (sq_entries as usize) * std::mem::size_of::<u32>();

        let sqe_array_ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                sqe_array_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_POPULATE,
                fd,
                sqe_array_offset as libc::off_t,
            )
        };
        if sqe_array_ptr == libc::MAP_FAILED {
            queue_exit(fd);
            return Err("failed to mmap SQE array");
        }

        // mmap CQ ring
        let cq_ring_size = (params.cq_off.cqes as usize)
            + (cq_entries as usize) * std::mem::size_of::<io_uring_cqe>();
        let cq_ring_size = (cq_ring_size + page_size - 1) & !(page_size - 1);

        let cq_ring_ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                cq_ring_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_POPULATE,
                fd,
                0, // CQ ring offset (on single-mmap kernels this is same as SQ)
            )
        };
        if cq_ring_ptr == libc::MAP_FAILED {
            queue_exit(fd);
            return Err("failed to mmap CQ ring");
        }

        let (cq_ring, cqes_ptr, cqes_len) = unsafe {
            let base = cq_ring_ptr as *const u8;
            let ring = MmapRing {
                head: base.add(params.cq_off.head as usize) as *mut AtomicU32,
                tail: base.add(params.cq_off.tail as usize) as *mut AtomicU32,
                ring_mask: base.add(params.cq_off.ring_mask as usize) as *const u32,
                ring_entries: base.add(params.cq_off.ring_entries as usize) as *const u32,
                array: ptr::null_mut(),
                len: cq_ring_size,
            };
            let cqes = base.add(params.cq_off.cqes as usize) as *mut io_uring_cqe;
            (ring, cqes, cq_entries as usize)
        };

        Ok(Self {
            fd,
            sqe_array: sqe_array_ptr as *mut io_uring_sqe,
            sqe_array_len: sq_entries as usize,
            sq_ring,
            cq_ring,
            cqes: cqes_ptr,
            cqes_len,
            in_flight: AtomicU32::new(0),
        })
    }

    fn fd(&self) -> i32 {
        self.fd
    }
}

impl Backend for UringBackend {
    fn submit(&self, flows: &[Flow]) -> tpt_torus_core::error::Result<usize> {
        let mut submitted = 0u32;

        for flow in flows {
            let tail = unsafe { (*self.sq_ring.tail).load(Ordering::Acquire) };
            let head = unsafe { (*self.sq_ring.head).load(Ordering::Acquire) };
            let mask = self.sq_ring.mask();
            let entries = self.sq_ring.entries();

            if (tail.wrapping_sub(head) & mask) >= entries {
                break;
            }

            let index = (tail & mask) as usize;
            let sqe = unsafe { &mut *self.sqe_array.add(index) };

            match flow.operation() {
                Operation::Read {
                    fd,
                    buf,
                    len,
                    offset,
                } => {
                    sqe.opcode = opcodes::IORING_OP_READ;
                    sqe.fd = *fd;
                    sqe.addr_splice_off_in = *buf as u64;
                    sqe.len = *len as u32;
                    sqe.off_addr2 = *offset;
                }
                Operation::Write {
                    fd,
                    buf,
                    len,
                    offset,
                } => {
                    sqe.opcode = opcodes::IORING_OP_WRITE;
                    sqe.fd = *fd;
                    sqe.addr_splice_off_in = *buf as u64;
                    sqe.len = *len as u32;
                    sqe.off_addr2 = *offset;
                }
                Operation::Accept { fd, addr, addrlen } => {
                    sqe.opcode = opcodes::IORING_OP_ACCEPT;
                    sqe.fd = *fd;
                    sqe.addr_splice_off_in = *addr as u64;
                    sqe.len = 0;
                    sqe.off_addr2 = *addrlen as u64;
                }
                Operation::Connect { fd, addr, addrlen } => {
                    sqe.opcode = opcodes::IORING_OP_CONNECT;
                    sqe.fd = *fd;
                    sqe.addr_splice_off_in = *addr as u64;
                    sqe.len = 0;
                    sqe.off_addr2 = *addrlen as u64;
                }
                Operation::Recv { fd, buf, len } => {
                    sqe.opcode = opcodes::IORING_OP_RECV;
                    sqe.fd = *fd;
                    sqe.addr_splice_off_in = *buf as u64;
                    sqe.len = *len as u32;
                }
                Operation::Send { fd, buf, len } => {
                    sqe.opcode = opcodes::IORING_OP_SEND;
                    sqe.fd = *fd;
                    sqe.addr_splice_off_in = *buf as u64;
                    sqe.len = *len as u32;
                }
                Operation::Close { fd } => {
                    sqe.opcode = opcodes::IORING_OP_CLOSE;
                    sqe.fd = *fd;
                }
                Operation::Readv {
                    fd,
                    bufs,
                    buf_count,
                    offset,
                } => {
                    // Vectored read: submit one SQE per buffer with sequential offsets.
                    // This is the fallback; native IORING_OP_READV could be used with
                    // proper iovec support in torus-sys.
                    let bufs_slice = unsafe { std::slice::from_raw_parts(*bufs, *buf_count as usize) };
                    let mut current_offset = *offset;

                    // Write the first buffer to the current SQE
                    if let Some(first) = bufs_slice.first() {
                        sqe.opcode = opcodes::IORING_OP_READ;
                        sqe.fd = *fd;
                        sqe.addr_splice_off_in = first.buf as u64;
                        sqe.len = first.len as u32;
                        sqe.off_addr2 = current_offset;
                        sqe.user_data = flow.user_data();

                        current_offset += first.len as u64;

                        // Submit remaining buffers as additional SQEs
                        for buf_desc in &bufs_slice[1..] {
                            let next_tail = unsafe { (*self.sq_ring.tail).load(Ordering::Acquire) };
                            let next_head = unsafe { (*self.sq_ring.head).load(Ordering::Acquire) };
                            if (next_tail.wrapping_sub(next_head) & mask) >= entries {
                                break;
                            }
                            let next_index = (next_tail & mask) as usize;
                            let next_sqe = unsafe { &mut *self.sqe_array.add(next_index) };
                            next_sqe.opcode = opcodes::IORING_OP_READ;
                            next_sqe.fd = *fd;
                            next_sqe.addr_splice_off_in = buf_desc.buf as u64;
                            next_sqe.len = buf_desc.len as u32;
                            next_sqe.off_addr2 = current_offset;
                            next_sqe.user_data = flow.user_data();
                            unsafe {
                                (*self.sq_ring.tail).store(next_tail.wrapping_add(1), Ordering::Release);
                            }
                            submitted += 1;
                            current_offset += buf_desc.len as u64;
                        }
                    } else {
                        // Empty scatter list — still need to fill the SQE
                        sqe.opcode = opcodes::IORING_OP_NOP;
                    }
                }
                Operation::Writev {
                    fd,
                    bufs,
                    buf_count,
                    offset,
                } => {
                    // Vectored write: submit one SQE per buffer with sequential offsets.
                    let bufs_slice = unsafe { std::slice::from_raw_parts(*bufs, *buf_count as usize) };
                    let mut current_offset = *offset;

                    if let Some(first) = bufs_slice.first() {
                        sqe.opcode = opcodes::IORING_OP_WRITE;
                        sqe.fd = *fd;
                        sqe.addr_splice_off_in = first.buf as u64;
                        sqe.len = first.len as u32;
                        sqe.off_addr2 = current_offset;
                        sqe.user_data = flow.user_data();

                        current_offset += first.len as u64;

                        for buf_desc in &bufs_slice[1..] {
                            let next_tail = unsafe { (*self.sq_ring.tail).load(Ordering::Acquire) };
                            let next_head = unsafe { (*self.sq_ring.head).load(Ordering::Acquire) };
                            if (next_tail.wrapping_sub(next_head) & mask) >= entries {
                                break;
                            }
                            let next_index = (next_tail & mask) as usize;
                            let next_sqe = unsafe { &mut *self.sqe_array.add(next_index) };
                            next_sqe.opcode = opcodes::IORING_OP_WRITE;
                            next_sqe.fd = *fd;
                            next_sqe.addr_splice_off_in = buf_desc.buf as u64;
                            next_sqe.len = buf_desc.len as u32;
                            next_sqe.off_addr2 = current_offset;
                            next_sqe.user_data = flow.user_data();
                            unsafe {
                                (*self.sq_ring.tail).store(next_tail.wrapping_add(1), Ordering::Release);
                            }
                            submitted += 1;
                            current_offset += buf_desc.len as u64;
                        }
                    } else {
                        sqe.opcode = opcodes::IORING_OP_NOP;
                    }
                }
            }

            sqe.user_data = flow.user_data();

            unsafe {
                (*self.sq_ring.tail).store(tail.wrapping_add(1), Ordering::Release);
            }
            submitted += 1;
        }

        if submitted == 0 {
            return Err(tpt_torus_core::error::Error::SubmissionFull);
        }

        fence(Ordering::Release);

        let ret = unsafe { tpt_torus_sys::io_uring_enter(self.fd, submitted, 0, 0, ptr::null()) };

        if ret < 0 {
            let errno = -ret as i32;
            Err(tpt_torus_core::error::Error::Os(errno))
        } else {
            self.in_flight.fetch_add(submitted, Ordering::Relaxed);
            Ok(submitted as usize)
        }
    }

    fn reap(&self, results: &mut Vec<TorusResult>) -> tpt_torus_core::error::Result<usize> {
        let mask = self.cq_ring.mask();
        let entries = self.cq_ring.entries();
        let mut count = 0u32;

        loop {
            let head = unsafe { (*self.cq_ring.head).load(Ordering::Acquire) };
            let tail = unsafe { (*self.cq_ring.tail).load(Ordering::Acquire) };

            if (tail.wrapping_sub(head) & mask) == 0 {
                break;
            }

            let index = (head & mask) as usize;
            let cqe = unsafe { &*self.cqes.add(index) };

            results.push(TorusResult::new(cqe.res as i64, cqe.user_data));
            count += 1;

            unsafe {
                (*self.cq_ring.head).store(head.wrapping_add(1), Ordering::Release);
            }
        }

        self.in_flight.fetch_sub(count, Ordering::Relaxed);
        Ok(count as usize)
    }

    fn wait(&self, timeout_us: u64) -> tpt_torus_core::error::Result<()> {
        let min_complete = if self.in_flight.load(Ordering::Relaxed) == 0 {
            0
        } else {
            1
        };

        let ret = unsafe {
            tpt_torus_sys::io_uring_enter(
                self.fd,
                0,
                min_complete,
                tpt_torus_sys::enter_flags::IORING_ENTER_GETEVENTS,
                ptr::null(),
            )
        };

        if ret < 0 {
            let errno = -ret as i32;
            Err(tpt_torus_core::error::Error::Os(errno))
        } else {
            Ok(())
        }
    }

    fn in_flight(&self) -> u32 {
        self.in_flight.load(Ordering::Relaxed)
    }
}

impl Drop for UringBackend {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(
                self.sqe_array as *mut libc::c_void,
                self.sqe_array_len * std::mem::size_of::<io_uring_sqe>(),
            );
            libc::munmap(
                self.cqes as *mut libc::c_void,
                self.cqes_len * std::mem::size_of::<io_uring_cqe>(),
            );
        }
        queue_exit(self.fd);
    }
}
