//! The attached host table and the `log` bridge.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicPtr, Ordering};

use fish_abi::{FishHostV1, FishStatus, FishStr, level};

use crate::error::Error;

static HOST: AtomicPtr<FishHostV1> = AtomicPtr::new(std::ptr::null_mut());

/// Attaches the host table. Called by the generated entrypoint trampolines.
///
/// The host promises the table outlives the mod.
pub(crate) fn attach(host: *const FishHostV1) {
    HOST.store(host.cast_mut(), Ordering::Release);
}

/// The attached host table.
pub(crate) fn get() -> Result<&'static FishHostV1, Error> {
    let p = HOST.load(Ordering::Acquire);
    if p.is_null() {
        return Err(Error::NoHost);
    }
    // SAFETY: `attach` stored a pointer the host guarantees to keep valid and immutable for as
    // long as the mod runs.
    Ok(unsafe { &*p })
}

/// Writes a record to the host log.
pub(crate) fn host_log(level: u8, target: &str, message: &str) {
    if let Ok(h) = get() {
        // SAFETY: the host table is valid (see `get`); both strings outlive the call.
        unsafe {
            (h.log)(
                h.host,
                h.mod_token,
                level,
                FishStr::new(target),
                FishStr::new(message),
            );
        }
    }
}

struct HostLogger;

fn level_of(l: log::Level) -> u8 {
    match l {
        log::Level::Error => level::ERROR,
        log::Level::Warn => level::WARN,
        log::Level::Info => level::INFO,
        log::Level::Debug => level::DEBUG,
        log::Level::Trace => level::TRACE,
    }
}

impl log::Log for HostLogger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        host_log(
            level_of(record.level()),
            record.target(),
            &record.args().to_string(),
        );
    }

    fn flush(&self) {}
}

/// Routes the `log` crate of this mod to the host (idempotent).
pub(crate) fn install_logger() {
    static LOGGER: HostLogger = HostLogger;
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Trace);
    }
}

/// Best-effort text of a panic payload.
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic with a non-string payload".to_string()
    }
}

/// Runs `f`, converting a panic into [`FishStatus::PANICKED`] and a log record.
pub(crate) fn guard(what: &str, f: impl FnOnce() -> FishStatus) -> FishStatus {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(status) => status,
        Err(payload) => {
            host_log(
                level::ERROR,
                "fish_api",
                &format!("panic in {what}: {}", panic_message(&*payload)),
            );
            FishStatus::PANICKED
        }
    }
}
