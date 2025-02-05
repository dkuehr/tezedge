// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::io::Error;

pub fn fork<F: FnOnce()>(child_func: F) -> libc::pid_t {
    unsafe {
        match libc::fork() {
            -1 => panic!("fork failed: {}", Error::last_os_error()),
            0 => {
                child_func();
                libc::exit(0);
            }
            pid => pid,
        }
    }
}

pub fn wait(pid: libc::pid_t) -> bool {
    // libc::waitpid is unsafe function
    unsafe {
        let mut status: i32 = 0;
        let options: i32 = 0;
        return match libc::waitpid(pid, &mut status as *mut i32, options) {
            -1 => {
                panic!("error occured libc::waitpid problem")
            }
            _pid => true,
        };
    }
}
