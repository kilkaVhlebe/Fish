//! Host side of the client services: `fish:client`, `fish:hud`, `fish:input`, `fish:command_raw`.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};

use fish_abi::client::{
    ClientServiceV1, CommandRawServiceV1, HudServiceV1, InputServiceV1, PlayerV1,
    ROLE_DEDICATED_SERVER, ROLE_INTEGRATED_SERVER, ROLE_WINDOW, StrSink,
};
use fish_abi::{FishHandle, FishServiceHeader, FishStatus, FishStr};
use fish_hooks::Role;

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

/// Wire value of a role.
pub(crate) fn role_byte(role: Role) -> u8 {
    match role {
        Role::Window => ROLE_WINDOW,
        Role::IntegratedServer => ROLE_INTEGRATED_SERVER,
        Role::DedicatedServer => ROLE_DEDICATED_SERVER,
    }
}

unsafe extern "C" fn client_role(host: *mut c_void, view: FishHandle) -> u8 {
    // SAFETY: ABI contract.
    let inner = unsafe { inner(host) };
    catch_unwind(AssertUnwindSafe(|| {
        inner
            .handles
            .client(view, |c| role_byte(c.role()))
            .unwrap_or(u8::MAX)
    }))
    .unwrap_or(u8::MAX)
}

unsafe extern "C" fn client_player(
    host: *mut c_void,
    view: FishHandle,
    out: *mut PlayerV1,
    dimension: StrSink,
    user: *mut c_void,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract.
        let inner = unsafe { inner(host) };
        if out.is_null() {
            return FishStatus::INVALID_ARGUMENT;
        }
        match inner.handles.client(view, |c| c.player()) {
            Err(st) => st,
            Ok(None) => FishStatus::NOT_FOUND,
            Ok(Some(p)) => {
                // SAFETY: `out` checked non-null; the sink is the caller's.
                unsafe {
                    *out = PlayerV1 {
                        x: p.x,
                        y: p.y,
                        z: p.z,
                        yaw: p.yaw,
                        pitch: p.pitch,
                    };
                    dimension(user, FishStr::new(&p.dimension));
                }
                FishStatus::OK
            }
        }
    })
}

unsafe extern "C" fn client_show_chat(
    host: *mut c_void,
    view: FishHandle,
    text: FishStr,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract.
        let (inner, text) = unsafe { (inner(host), text.as_str()) };
        let Ok(text) = text else {
            return FishStatus::INVALID_ARGUMENT;
        };
        match inner.handles.client(view, |c| c.show_chat(text)) {
            Ok(()) => FishStatus::OK,
            Err(st) => st,
        }
    })
}

unsafe extern "C" fn client_game_dir(
    host: *mut c_void,
    view: FishHandle,
    sink: StrSink,
    user: *mut c_void,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract.
        let inner = unsafe { inner(host) };
        match inner.handles.client(view, |c| c.game_dir()) {
            Ok(dir) => {
                let text = dir.display().to_string();
                // SAFETY: the sink is the caller's; the text outlives the call.
                unsafe { sink(user, FishStr::new(&text)) };
                FishStatus::OK
            }
            Err(st) => st,
        }
    })
}

/// `fish:client`.
pub(crate) static CLIENT: ClientServiceV1 = ClientServiceV1 {
    header: FishServiceHeader {
        size: size_of::<ClientServiceV1>(),
        version: 1,
    },
    role: client_role,
    player: client_player,
    show_chat: client_show_chat,
    game_dir: client_game_dir,
};

unsafe extern "C" fn hud_fill(
    host: *mut c_void,
    hud: FishHandle,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    argb: u32,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract.
        match unsafe { inner(host) }
            .handles
            .hud(hud, |v| v.fill([x, y, w, h], argb))
        {
            Ok(()) => FishStatus::OK,
            Err(st) => st,
        }
    })
}

unsafe extern "C" fn hud_text(
    host: *mut c_void,
    hud: FishHandle,
    text: FishStr,
    x: f32,
    y: f32,
    rgb: u32,
    shadow: u8,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract.
        let (inner, text) = unsafe { (inner(host), text.as_str()) };
        let Ok(text) = text else {
            return FishStatus::INVALID_ARGUMENT;
        };
        match inner
            .handles
            .hud(hud, |v| v.text(text, x, y, rgb, shadow != 0))
        {
            Ok(()) => FishStatus::OK,
            Err(st) => st,
        }
    })
}

unsafe extern "C" fn hud_text_width(
    host: *mut c_void,
    hud: FishHandle,
    text: FishStr,
    out: *mut f32,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract.
        let (inner, text) = unsafe { (inner(host), text.as_str()) };
        let (Ok(text), false) = (text, out.is_null()) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        match inner.handles.hud(hud, |v| v.text_width(text)) {
            Ok(w) => {
                // SAFETY: `out` checked non-null.
                unsafe { *out = w };
                FishStatus::OK
            }
            Err(st) => st,
        }
    })
}

unsafe extern "C" fn hud_gui_size(
    host: *mut c_void,
    hud: FishHandle,
    out: *mut [f32; 2],
) -> FishStatus {
    guard(|| {
        if out.is_null() {
            return FishStatus::INVALID_ARGUMENT;
        }
        // SAFETY: ABI contract.
        match unsafe { inner(host) }.handles.hud(hud, |v| v.gui_size()) {
            Ok(size) => {
                // SAFETY: `out` checked non-null.
                unsafe { *out = size };
                FishStatus::OK
            }
            Err(st) => st,
        }
    })
}

/// `fish:hud`.
pub(crate) static HUD: HudServiceV1 = HudServiceV1 {
    header: FishServiceHeader {
        size: size_of::<HudServiceV1>(),
        version: 1,
    },
    fill: hud_fill,
    text: hud_text,
    text_width: hud_text_width,
    gui_size: hud_gui_size,
};

unsafe extern "C" fn input_is_down(host: *mut c_void, key: FishStr) -> u8 {
    // SAFETY: ABI contract.
    let (inner, key) = unsafe { (inner(host), key.as_str()) };
    let Ok(key) = key else { return 0 };
    u8::from(
        inner
            .keys_down
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(key),
    )
}

/// `fish:input`.
pub(crate) static INPUT: InputServiceV1 = InputServiceV1 {
    header: FishServiceHeader {
        size: size_of::<InputServiceV1>(),
        version: 1,
    },
    is_down: input_is_down,
};

/// `fish:command_raw`.
pub(crate) static COMMAND_RAW: CommandRawServiceV1 = CommandRawServiceV1 {
    header: FishServiceHeader {
        size: size_of::<CommandRawServiceV1>(),
        version: 1,
    },
    feedback: client_show_chat,
};
