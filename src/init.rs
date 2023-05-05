use std::ffi::CStr;

use pgrx::pg_sys::{GetBackendTypeDesc, MyBackendType};
use pgrx::*;

use crate::cache_worker;
use crate::crate_info::*;
use crate::settings;

#[pg_guard]
pub extern "C" fn _PG_init() {
    let backend_type = backend_type();

    debug1!(
        "{}: Running _PG_init() in backend of type: {}",
        CRATE,
        backend_type
    );

    settings::define();
    cache_worker::start();
}

pub fn backend_type() -> String {
    let s = unsafe {
        let chars = GetBackendTypeDesc(MyBackendType);
        CStr::from_ptr(chars)
    };
    String::from_utf8_lossy(s.to_bytes()).to_string()
}
