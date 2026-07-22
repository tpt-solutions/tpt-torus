//! Fuzz target for Result construction from arbitrary bytes.
//!
//! Tests that creating a Result from raw i64/user_data doesn't panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt_torus_core::result::Result;

fuzz_target!(|data: &[u8]| {
    if data.len() < 16 {
        return;
    }

    let result_val = i64::from_ne_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);
    let user_data = u64::from_ne_bytes([
        data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
    ]);

    let result = Result::new(result_val, user_data);

    // Exercise all accessor methods
    let _ = result.is_ok();
    let _ = result.bytes();
    let _ = result.raw();
    let _ = result.error();
});
