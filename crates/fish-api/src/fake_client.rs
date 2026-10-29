//! Fake client services (`fish:client`, `fish:hud`, `fish:input`, `fish:command_raw`) and tests of
//! the context wrappers built on them.

use std::ffi::c_void;
use std::sync::Mutex;

use fish_abi::client::{
    ClientServiceV1, CommandPayload, CommandRawServiceV1, FramePayload, HudPayload, HudServiceV1,
    InputServiceV1, KeyPayload, PlayerV1, ROLE_DEDICATED_SERVER, ROLE_INTEGRATED_SERVER,
    ROLE_WINDOW, StrSink, TickPayload, services as ids,
};
use fish_abi::{FishHandle, FishServiceHeader, FishStatus, FishStr};

use crate::client::{
    ClientHandle, DrawContext, FrameContext, HudRenderContext, KeyContext, RawCommandContext, Role,
    TickContext,
};

/// Everything the fake services were asked to do.
static CHAT: Mutex<Vec<String>> = Mutex::new(Vec::new());
static HUD: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Handle indexes with special behaviour.
const STALE: u32 = 99;
const MENU: u32 = 5;

unsafe extern "C" fn role(_h: *mut c_void, v: FishHandle) -> u8 {
    match v.index {
        STALE => u8::MAX,
        1 => ROLE_INTEGRATED_SERVER,
        2 => ROLE_DEDICATED_SERVER,
        _ => ROLE_WINDOW,
    }
}

unsafe extern "C" fn player(
    _h: *mut c_void,
    v: FishHandle,
    out: *mut PlayerV1,
    dimension: StrSink,
    user: *mut c_void,
) -> FishStatus {
    match v.index {
        STALE => FishStatus::STALE_HANDLE,
        MENU => FishStatus::NOT_FOUND,
        _ => {
            // SAFETY: the wrapper passes valid pointers.
            unsafe {
                *out = PlayerV1 {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                    yaw: 4.0,
                    pitch: 5.0,
                };
                dimension(user, FishStr::from_static("minecraft:the_nether"));
            }
            FishStatus::OK
        }
    }
}

unsafe extern "C" fn show_chat(_h: *mut c_void, v: FishHandle, text: FishStr) -> FishStatus {
    if v.index == STALE {
        return FishStatus::STALE_HANDLE;
    }
    // SAFETY: the wrapper passes a valid string.
    CHAT.lock()
        .unwrap()
        .push(unsafe { text.as_str() }.unwrap().to_string());
    FishStatus::OK
}

unsafe extern "C" fn game_dir(
    _h: *mut c_void,
    v: FishHandle,
    sink: StrSink,
    user: *mut c_void,
) -> FishStatus {
    if v.index == STALE {
        return FishStatus::STALE_HANDLE;
    }
    // SAFETY: valid sink and user pointer from the wrapper.
    unsafe { sink(user, FishStr::from_static("/the/game")) };
    FishStatus::OK
}

unsafe extern "C" fn fill(
    _h: *mut c_void,
    _hud: FishHandle,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    argb: u32,
) -> FishStatus {
    HUD.lock()
        .unwrap()
        .push(format!("fill {x} {y} {w} {h} {argb:#x}"));
    FishStatus::OK
}

unsafe extern "C" fn text(
    _h: *mut c_void,
    hud: FishHandle,
    t: FishStr,
    x: f32,
    y: f32,
    rgb: u32,
    shadow: u8,
) -> FishStatus {
    if hud.index == STALE {
        return FishStatus::STALE_HANDLE;
    }
    // SAFETY: the wrapper passes a valid string.
    let t = unsafe { t.as_str() }.unwrap();
    HUD.lock()
        .unwrap()
        .push(format!("text {t} {x} {y} {rgb:#x} {shadow}"));
    FishStatus::OK
}

unsafe extern "C" fn text_width(
    _h: *mut c_void,
    _hud: FishHandle,
    t: FishStr,
    out: *mut f32,
) -> FishStatus {
    // SAFETY: valid string and out pointer from the wrapper.
    unsafe { *out = 6.0 * t.as_str().unwrap().chars().count() as f32 };
    FishStatus::OK
}

unsafe extern "C" fn gui_size(_h: *mut c_void, _hud: FishHandle, out: *mut [f32; 2]) -> FishStatus {
    // SAFETY: valid out pointer from the wrapper.
    unsafe { *out = [320.0, 240.0] };
    FishStatus::OK
}

unsafe extern "C" fn is_down(_h: *mut c_void, key: FishStr) -> u8 {
    // SAFETY: valid string from the wrapper.
    u8::from(unsafe { key.as_str() } == Ok("key.keyboard.h"))
}

static CLIENT: ClientServiceV1 = ClientServiceV1 {
    header: FishServiceHeader {
        size: size_of::<ClientServiceV1>(),
        version: 1,
    },
    role,
    player,
    show_chat,
    game_dir,
};
static HUD_SVC: HudServiceV1 = HudServiceV1 {
    header: FishServiceHeader {
        size: size_of::<HudServiceV1>(),
        version: 1,
    },
    fill,
    text,
    text_width,
    gui_size,
};
static INPUT: InputServiceV1 = InputServiceV1 {
    header: FishServiceHeader {
        size: size_of::<InputServiceV1>(),
        version: 1,
    },
    is_down,
};
static COMMAND_RAW: CommandRawServiceV1 = CommandRawServiceV1 {
    header: FishServiceHeader {
        size: size_of::<CommandRawServiceV1>(),
        version: 1,
    },
    feedback: show_chat,
};

/// The table for a service id, if the fake provides it.
pub(crate) fn lookup(id: &str) -> Option<*const c_void> {
    Some(match id {
        ids::CLIENT => (&raw const CLIENT).cast(),
        ids::HUD => (&raw const HUD_SVC).cast(),
        ids::INPUT => (&raw const INPUT).cast(),
        ids::COMMAND_RAW => (&raw const COMMAND_RAW).cast(),
        _ => return None,
    })
}

fn handle(index: u32) -> FishHandle {
    FishHandle {
        index,
        generation: 1,
    }
}

// SAFETY (all casts below): the contexts are `repr(transparent)` over their payloads.
fn tick(index: u32, role: u8, tick: u64) -> TickContext {
    let p = TickPayload {
        size: size_of::<TickPayload>() as u32,
        role,
        reserved: [0; 3],
        tick,
        view: handle(index),
    };
    unsafe { std::mem::transmute(p) }
}

#[test]
fn tick_contexts_expose_role_tick_and_the_game() {
    crate::fake::install();
    let c = tick(1, ROLE_INTEGRATED_SERVER, 7);
    assert_eq!((c.tick(), c.role()), (7, Role::IntegratedServer));
    assert_eq!(
        tick(2, ROLE_DEDICATED_SERVER, 1).role(),
        Role::DedicatedServer
    );
    assert_eq!(tick(0, ROLE_WINDOW, 1).role(), Role::Window);
    assert_eq!(c.client().role().unwrap(), Role::IntegratedServer);
    assert_eq!(
        tick(2, 0, 1).client().role().unwrap(),
        Role::DedicatedServer
    );
    assert_eq!(tick(0, 0, 1).client().role().unwrap(), Role::Window);
}

#[test]
fn the_client_handle_reads_player_dir_and_shows_chat() {
    crate::fake::install();
    let game: ClientHandle = tick(0, 0, 1).client();
    let p = game.player().unwrap().unwrap();
    assert_eq!((p.x, p.y, p.z, p.yaw, p.pitch), (1.0, 2.0, 3.0, 4.0, 5.0));
    assert_eq!(p.dimension, "minecraft:the_nether");
    assert!(
        tick(MENU, 0, 1).client().player().unwrap().is_none(),
        "no player outside a world"
    );
    assert_eq!(
        game.game_dir().unwrap(),
        std::path::PathBuf::from("/the/game")
    );
    game.show_chat("hello chat").unwrap();
    assert!(CHAT.lock().unwrap().contains(&"hello chat".to_string()));
}

#[test]
fn stale_handles_are_errors_everywhere() {
    crate::fake::install();
    let stale = tick(STALE, 0, 1).client();
    for err in [
        stale.role().unwrap_err(),
        stale.player().unwrap_err(),
        stale.show_chat("x").unwrap_err(),
        stale.game_dir().unwrap_err(),
    ] {
        assert_eq!(err.status(), Some(FishStatus::STALE_HANDLE), "{err}");
    }
}

fn hud(index: u32) -> HudRenderContext {
    let p = HudPayload {
        size: size_of::<HudPayload>() as u32,
        view: handle(0),
        hud: handle(index),
    };
    unsafe { std::mem::transmute(p) }
}

#[test]
fn drawing_goes_through_the_hud_service() {
    crate::fake::install();
    let ctx = hud(3);
    let d: DrawContext = ctx.draw();
    d.fill([1.0, 2.0, 3.0, 4.0], 0x7800_0000).unwrap();
    d.text("Hello", 4.0, 5.0, 0xFF_FFFF, true).unwrap();
    assert_eq!(d.text_width("abc").unwrap(), 18.0);
    assert_eq!(d.gui_size().unwrap(), [320.0, 240.0]);
    let log = HUD.lock().unwrap().clone();
    assert!(
        log.contains(&"fill 1 2 3 4 0x78000000".to_string()),
        "{log:?}"
    );
    assert!(
        log.contains(&"text Hello 4 5 0xffffff 1".to_string()),
        "{log:?}"
    );
    assert_eq!(ctx.client().role().unwrap(), Role::Window);
    assert_eq!(
        hud(STALE)
            .draw()
            .text("x", 0.0, 0.0, 0, false)
            .unwrap_err()
            .status(),
        Some(FishStatus::STALE_HANDLE)
    );
}

#[test]
fn commands_keys_and_frames() {
    crate::fake::install();
    let command = "fish hello";
    let p = CommandPayload {
        size: size_of::<CommandPayload>() as u32,
        view: handle(0),
        command: FishStr::new(command),
    };
    let c: RawCommandContext = unsafe { std::mem::transmute(p) };
    assert_eq!(c.command(), "fish hello");
    c.feedback("done").unwrap();
    assert!(CHAT.lock().unwrap().contains(&"done".to_string()));
    assert_eq!(c.client().role().unwrap(), Role::Window);

    let key = "key.keyboard.h";
    let kp = KeyPayload {
        size: size_of::<KeyPayload>() as u32,
        pressed: 1,
        reserved: [0; 3],
        view: handle(0),
        key: FishStr::new(key),
    };
    let k: KeyContext = unsafe { std::mem::transmute(kp) };
    assert_eq!((k.key(), k.pressed()), ("key.keyboard.h", true));
    assert_eq!(k.client().role().unwrap(), Role::Window);
    assert!(crate::client::is_key_down("key.keyboard.h").unwrap());
    assert!(!crate::client::is_key_down("key.keyboard.j").unwrap());

    let fp = FramePayload {
        size: size_of::<FramePayload>() as u32,
        elapsed_secs: 0.25,
        view: handle(0),
    };
    let f: FrameContext = unsafe { std::mem::transmute(fp) };
    assert_eq!(f.elapsed_secs(), 0.25);
    assert_eq!(f.client().role().unwrap(), Role::Window);
}

#[test]
fn the_events_have_the_documented_ids_and_modes() {
    use crate::event::lifecycle::{client, server};
    assert_eq!(client::START_TICK.id(), "fish:lifecycle/client_start_tick");
    assert_eq!(client::END_TICK.id(), "fish:lifecycle/client_end_tick");
    assert_eq!(server::START_TICK.id(), "fish:lifecycle/server_start_tick");
    assert_eq!(server::END_TICK.id(), "fish:lifecycle/server_end_tick");
    assert_eq!(crate::event::client::FRAME.id(), "fish:client/frame");
    assert_eq!(crate::client::hud::RENDER.id(), "fish:hud/render");
    assert_eq!(
        crate::client::command::RAW_COMMAND.id(),
        "fish:command_raw/command"
    );
    assert_eq!(crate::client::input::KEY.id(), "fish:input/key");
}

#[test]
fn game_contexts_read_every_field_and_rewrite_text() {
    use fish_abi::game::{FACE_NONE, GamePayload};

    use crate::game::{BlockPos, Face, GameContext, Hand};

    unsafe extern "C" fn store(user: *mut c_void, text: FishStr) {
        unsafe { *user.cast::<String>() = text.as_str().unwrap().to_string() };
    }
    let mut stored = String::new();
    let mut p = GamePayload::new(ROLE_INTEGRATED_SERVER, handle(1));
    p.is_client = 1;
    p.hand = 1;
    p.face = 2;
    p.pos = [1, -2, 3];
    p.entity_id = 77;
    p.amount = 2.5;
    p.actor_uuid = 42u128.to_be_bytes();
    p.actor = FishStr::new("Steve");
    p.dimension = FishStr::new("minecraft:overworld");
    p.subject = FishStr::new("minecraft:stone");
    p.item = FishStr::new("minecraft:stick");
    p.extra = FishStr::new("player");
    p.replace = Some(store);
    p.replace_user = (&raw mut stored).cast();
    let mut c: GameContext = unsafe { std::mem::transmute(p) };
    assert!(c.is_client() && c.hand() == Hand::Off && c.face() == Some(Face::North));
    assert_eq!(c.face().unwrap().name(), "north");
    assert_eq!(c.pos(), BlockPos { x: 1, y: -2, z: 3 });
    assert_eq!(c.pos().to_string(), "1 -2 3");
    assert_eq!(
        (c.entity_id(), c.amount(), c.actor_uuid()),
        (77, 2.5, Some(42))
    );
    assert_eq!(
        (c.actor(), c.dimension(), c.subject(), c.item(), c.extra()),
        (
            "Steve",
            "minecraft:overworld",
            "minecraft:stone",
            "minecraft:stick",
            "player"
        )
    );
    assert_eq!(c.role(), Role::IntegratedServer);
    assert_eq!(c.client().role().unwrap(), Role::IntegratedServer);
    assert!(c.replace_text("edited"));
    assert_eq!(stored, "edited");

    let mut q = GamePayload::new(ROLE_DEDICATED_SERVER, handle(2));
    assert_eq!((q.face, q.hand), (FACE_NONE, 0));
    q.replace = None;
    let mut c: GameContext = unsafe { std::mem::transmute(q) };
    assert!(!c.replace_text("x"), "not rewritable");
    assert_eq!(
        (c.face(), c.actor_uuid(), c.hand(), c.role()),
        (None, None, Hand::Main, Role::DedicatedServer)
    );
}
