/*
 * Copyright 2018 Fluence Labs Limited
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! Debugger for Fluence VM

#![feature(allocator_api)]
#![allow(dead_code)]

use log::{error, info};

use std::error::Error;
use std::ptr::NonNull;

/// Result for all possible Error types.
pub type Result<T> = ::std::result::Result<T, Error>;

/// Takes the input, prints it out
#[no_mangle]
pub unsafe fn invoke(ptr: *mut u8, len: usize) -> NonNull<u8> {
    info!("invoke starts with ptr={:?}, len={}", ptr, len);
    // memory for the parameter will be deallocated when sql_str was dropped
    let raw_params = fluence::memory::read_input_from_mem(ptr, len);
    let params = String::from_utf8(raw_params).unwrap();

    info!("invoke input: {}", params);

    // return pointer to result in memory
    fluence::memory::write_result_to_mem("OK".as_bytes())
        .unwrap_or_else(|_| log_and_panic("Putting result string to the memory was failed.".into()))
}

fn log_and_panic(msg: String) -> ! {
    error!("{}", msg);
    panic!(msg);
}