use std::ffi::{c_char, CStr};
use std::ptr;

use anyhow::anyhow;
use pgrx::*;
use std::time::Duration;

use crate::crate_info::CRATE;

#[derive(Debug, Default)]
pub struct Settings {
    pub interval: Option<Duration>,
}

pub static INTERVAL: GucSetting<Option<&'static str>> = GucSetting::new(None);

pub fn define() {
    GucRegistry::define_string_guc(
        &format!("{CRATE}.interval"),
        "The interval at which to collect metrics.",
        "A background worker wakes up every interval and gathers statistics.",
        &INTERVAL,
        GucContext::Sighup,
        GucFlags::UNIT_S,
    );
}

pub fn read() -> anyhow::Result<Settings> {
    let seconds = unsafe {
        let name = format!("{CRATE}.interval");
        let input: *const c_char = INTERVAL.get_char_ptr();

        if input.is_null() {
            Ok(None)
        } else {
            debug1!("{}: {} = {:?}", CRATE, name, CStr::from_ptr(input));
            let mut hintmsg: *const c_char = ptr::null();
            let mut seconds: f64 = 0.0;
            let flags = pg_sys::GUC_UNIT_S as i32;
            if pg_sys::parse_real(input, &mut seconds, flags, &mut hintmsg) {
                Ok(Some(seconds))
            } else {
                let hint = CStr::from_ptr(hintmsg);
                let s = hint.to_string_lossy();
                Err(anyhow!("Error parsing {}: {}", &name, &s))
            }
        }
    }?;

    let interval = seconds.map(from_float_seconds);

    Ok(Settings { interval })
}

pub fn read_or_default() -> Settings {
    match read() {
        Ok(settings) => settings,
        Err(e) => {
            warning!("{}: Failed to parse settings: {:?}", CRATE, e);
            Settings::default()
        }
    }
}

fn from_float_seconds(seconds: f64) -> Duration {
    Duration::from_micros((seconds * 1000000.0) as u64)
}
