//! Host side of `fish:networking`.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};

use fish_abi::networking::{NameSink, NetRecvFn, NetworkingServiceV1};
use fish_abi::{DropUserFn, FishServiceHeader, FishStatus, FishStr};

use crate::inner::Inner;

fn guard(f: impl FnOnce() -> FishStatus) -> FishStatus {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(FishStatus::PANICKED)
}

/// # Safety
/// `host` must be the loader's `Inner` pointer.
unsafe fn inner<'a>(host: *mut c_void) -> &'a Inner {
    // SAFETY: guaranteed by the caller.
    unsafe { &*host.cast::<Inner>() }
}

/// # Safety
/// `data` must point at `len` readable bytes (or `len` is zero).
unsafe fn bytes<'a>(data: *const u8, len: usize) -> &'a [u8] {
    if len == 0 || data.is_null() {
        &[]
    } else {
        // SAFETY: guaranteed by the caller.
        unsafe { std::slice::from_raw_parts(data, len) }
    }
}

unsafe extern "C" fn register_type(
    host: *mut c_void,
    token: u64,
    dir: u8,
    id: FishStr,
    max_size: u32,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract.
        let (inner, id) = unsafe { (inner(host), id.as_str()) };
        match id {
            Ok(id) => inner.net.register_type(token, dir, id, max_size),
            Err(_) => FishStatus::INVALID_ARGUMENT,
        }
    })
}

unsafe extern "C" fn register_receiver(
    host: *mut c_void,
    token: u64,
    dir: u8,
    id: FishStr,
    func: NetRecvFn,
    user: *mut c_void,
    drop_user: Option<DropUserFn>,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract.
        let (inner, id) = unsafe { (inner(host), id.as_str()) };
        match id {
            Ok(id) => inner
                .net
                .register_receiver(token, dir, id, func, user, drop_user),
            Err(_) => FishStatus::INVALID_ARGUMENT,
        }
    })
}

unsafe extern "C" fn send_to_server(
    host: *mut c_void,
    dir: u8,
    id: FishStr,
    data: *const u8,
    len: usize,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract.
        let (inner, id, data) = unsafe { (inner(host), id.as_str(), bytes(data, len)) };
        match id {
            Ok(id) => inner.net.send_to_server(dir, id, data),
            Err(_) => FishStatus::INVALID_ARGUMENT,
        }
    })
}

unsafe extern "C" fn send_to_player(
    host: *mut c_void,
    dir: u8,
    player: FishStr,
    id: FishStr,
    data: *const u8,
    len: usize,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract.
        let (inner, player, id, data) =
            unsafe { (inner(host), player.as_str(), id.as_str(), bytes(data, len)) };
        match (player, id) {
            (Ok(p), Ok(id)) => inner.net.send_to_player(dir, p, id, data),
            _ => FishStatus::INVALID_ARGUMENT,
        }
    })
}

unsafe extern "C" fn can_send(host: *mut c_void, dir: u8, player: FishStr, id: FishStr) -> u8 {
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: ABI contract.
        let (inner, player, id) = unsafe { (inner(host), player.as_str(), id.as_str()) };
        match (player, id) {
            (Ok(p), Ok(id)) => u8::from(inner.net.can_send(dir, p, id)),
            _ => 0,
        }
    }))
    .unwrap_or(0)
}

unsafe extern "C" fn players(host: *mut c_void, dir: u8, sink: NameSink, user: *mut c_void) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: ABI contract.
        let inner = unsafe { inner(host) };
        for name in inner.net.players(dir) {
            // SAFETY: the sink is the caller's.
            unsafe { sink(user, FishStr::new(&name)) };
        }
    }));
}

/// The `fish:networking` table.
pub(crate) static NETWORKING: NetworkingServiceV1 = NetworkingServiceV1 {
    header: FishServiceHeader {
        size: size_of::<NetworkingServiceV1>(),
        version: 1,
    },
    register_type,
    register_receiver,
    send_to_server,
    send_to_player,
    can_send,
    players,
};
