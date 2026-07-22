//! Fuzz target for Flow creation and serialization.
//!
//! Tests that constructing a Flow from arbitrary input data doesn't panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt_torus_core::flow::Flow;
use tpt_torus_core::operation::Operation;

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }

    let op_type = data[0] % 7; // 7 operation variants
    let fd = i32::from_ne_bytes([data[1], data[2], data[3], data[4 % data.len()]]);

    let buf_ptr = data.as_ptr() as *mut u8;
    let len = data.len();
    let offset = u64::from_ne_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7 % data.len()],
    ]);

    let operation = match op_type {
        0 => Operation::Read { fd, buf: buf_ptr, len, offset },
        1 => Operation::Write { fd, buf: buf_ptr as *const u8, len, offset },
        2 => Operation::Recv { fd, buf: buf_ptr, len },
        3 => Operation::Send { fd, buf: buf_ptr as *const u8, len },
        4 => Operation::Close { fd },
        5 => Operation::Accept {
            fd,
            addr: buf_ptr as *mut libc::sockaddr,
            addrlen: buf_ptr as *mut u32,
        },
        6 => Operation::Connect {
            fd,
            addr: buf_ptr as *const libc::sockaddr,
            addrlen: len as u32,
        },
        _ => unreachable!(),
    };

    let _flow = Flow::with_user_data(operation, u64::from_ne_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7 % data.len()],
    ]));
});
