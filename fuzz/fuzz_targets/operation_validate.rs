//! Fuzz target for Operation variant construction.
//!
//! Tests that creating Operation variants from arbitrary data doesn't panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt_torus_core::operation::Operation;

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }

    let op_type = data[0] % 7;
    let fd = data[1] as i32;
    let len = data[2] as usize;
    let offset = u64::from_ne_bytes([
        data[3], data[4], data[5], data[6], data[7], data[8 % data.len()], data[9 % data.len()], data[10 % data.len()],
    ]);

    let buf_ptr = data.as_ptr() as *mut u8;

    let _op = match op_type {
        0 => Operation::Read { fd, buf: buf_ptr, len, offset },
        1 => Operation::Write { fd, buf: buf_ptr as *const u8, len, offset },
        2 => Operation::Recv { fd, buf: buf_ptr, len },
        3 => Operation::Send { fd, buf: buf_ptr as *const u8, len },
        4 => Operation::Close { fd },
        5 => Operation::Accept {
            fd,
            addr: std::ptr::null_mut(),
            addrlen: std::ptr::null_mut(),
        },
        6 => Operation::Connect {
            fd,
            addr: std::ptr::null(),
            addrlen: len as u32,
        },
        _ => unreachable!(),
    };
});
