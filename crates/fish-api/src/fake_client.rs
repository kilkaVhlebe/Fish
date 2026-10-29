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
        fish_abi::commands::SERVICE_COMMANDS => (&raw const COMMANDS).cast(),
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

// ---- commands ----

use fish_abi::DropUserFn;
use fish_abi::commands::{
    CommandArgV1, CommandCtxV1, CommandExecFn, CommandRegisterPayload, CommandsServiceV1,
    KIND_CLIENT, KIND_SERVER, arg,
};

/// What the fake host kept of a registration.
struct Registered {
    kind: u8,
    tree: String,
    exec: CommandExecFn,
    user: usize,
    drop_user: Option<DropUserFn>,
}

static REGISTERED: Mutex<Vec<Registered>> = Mutex::new(Vec::new());
/// Tests that use the fake registration list run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

static REGISTER_STATUS: Mutex<FishStatus> = Mutex::new(FishStatus::OK);

unsafe extern "C" fn register(
    _host: *mut c_void,
    _token: u64,
    kind: u8,
    tree: FishStr,
    exec: CommandExecFn,
    user: *mut c_void,
    drop_user: Option<DropUserFn>,
) -> FishStatus {
    let status = *REGISTER_STATUS.lock().unwrap();
    if status.is_ok() {
        REGISTERED.lock().unwrap().push(Registered {
            kind,
            // SAFETY: the API passes a valid string.
            tree: unsafe { tree.as_str() }.unwrap().to_string(),
            exec,
            user: user as usize,
            drop_user,
        });
    }
    status
}

static COMMANDS: CommandsServiceV1 = CommandsServiceV1 {
    header: FishServiceHeader {
        size: size_of::<CommandsServiceV1>(),
        version: 1,
    },
    register,
};

fn register_context(kind: u8, side: u8) -> crate::command::RegisterContext {
    let p = CommandRegisterPayload {
        size: size_of::<CommandRegisterPayload>() as u32,
        kind,
        side,
        reserved: [0; 2],
    };
    unsafe { std::mem::transmute(p) }
}

/// Output lines collected from an executor, with their error flags.
struct Lines(Vec<(String, bool)>);

unsafe extern "C" fn out(user: *mut c_void, text: FishStr, error: u8) {
    unsafe {
        (*user.cast::<Lines>())
            .0
            .push((text.as_str().unwrap().to_string(), error != 0))
    };
}

/// Runs executor `index` of the last registration with parsed arguments.
fn run(index: u32, args: &[(&str, u8, &str)]) -> (FishStatus, i32, Vec<(String, bool)>) {
    let reg = REGISTERED.lock().unwrap().pop().expect("a registration");
    let wire: Vec<CommandArgV1> = args
        .iter()
        .map(|(n, k, v)| CommandArgV1 {
            name: FishStr::new(n),
            kind: *k,
            value: FishStr::new(v),
        })
        .collect();
    let mut lines = Lines(Vec::new());
    let mut result = 0i32;
    let ctx = CommandCtxV1 {
        size: size_of::<CommandCtxV1>() as u32,
        is_player: 1,
        level: 2,
        reserved: [0; 2],
        source_name: FishStr::new("Alex"),
        source_uuid: 9u128.to_be_bytes(),
        pos: [1.0, 2.0, 3.0],
        has_pos: 1,
        reserved2: [0; 7],
        args: wire.as_ptr(),
        arg_count: wire.len(),
        out: (&raw mut lines).cast(),
        output: out,
        result: &raw mut result,
    };
    let status = unsafe { (reg.exec)(reg.user as *mut c_void, index, &ctx) };
    if let Some(f) = reg.drop_user {
        unsafe { f(reg.user as *mut c_void) };
    }
    (status, result, lines.0)
}

#[test]
fn a_command_tree_is_serialised_and_its_executors_run_with_typed_arguments() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    use crate::command::{ArgKind, argument, literal};
    crate::fake::install();
    REGISTERED.lock().unwrap().clear();
    let tree = literal("demo")
        .requires(2)
        .executes(|c| {
            c.feedback(&format!(
                "{} {:?} {} {:?}",
                c.source_name(),
                c.source_uuid(),
                c.level(),
                c.position()
            ));
            Ok(c.is_player() as i32)
        })
        .then(
            argument("n", ArgKind::Integer { min: 1, max: 9 })
                .suggests(&["1", "2"])
                .then(
                    argument(
                        "d",
                        ArgKind::Double {
                            min: 0.5,
                            max: f64::INFINITY,
                        },
                    )
                    .executes(|c| {
                        c.feedback(&format!(
                            "{} {} {}",
                            c.get_integer("n")?,
                            c.get_double("d")?,
                            c.get_bool("b").is_err()
                        ));
                        Ok(5)
                    }),
                ),
        )
        .then(literal("redir").redirect("other"))
        .then(
            literal("pos").then(argument("p", ArgKind::BlockPos).executes(|c| {
                c.feedback(&c.get_block_pos("p")?.to_string());
                Ok(1)
            })),
        )
        .then(
            literal("txt").then(argument("t", ArgKind::Greedy).executes(|c| {
                c.feedback(c.get_string("t")?);
                c.get_integer("t").map(|_| 0)
            })),
        )
        .then(
            literal("flag")
                .then(argument("b", ArgKind::Bool).executes(|c| Ok(i32::from(c.get_bool("b")?)))),
        )
        .then(literal("boom").executes(|_| panic!("boom")))
        .then(
            literal("kinds").then(
                argument("w", ArgKind::Word).then(
                    argument("s", ArgKind::String).then(
                        argument("p", ArgKind::Player)
                            .then(argument("e", ArgKind::Entity).executes(|_| Ok(0))),
                    ),
                ),
            ),
        );
    register_context(KIND_SERVER, 0).register(tree).unwrap();
    let json = REGISTERED.lock().unwrap()[0].tree.clone();
    for part in [
        r#""name":"demo","type":"literal","level":2,"exec":0"#,
        r#""name":"n","type":"integer","min":1,"max":9,"suggests":["1","2"]"#,
        r#""name":"d","type":"double","min":0.5"#,
        r#""redirect":"other""#,
        r#""type":"block_pos""#,
        r#""type":"greedy""#,
        r#""type":"word""#,
        r#""type":"string""#,
        r#""type":"player""#,
        r#""type":"entity""#,
    ] {
        assert!(json.contains(part), "{part} in {json}");
    }
    assert!(!json.contains("\"max\":inf"), "{json}");
    assert_eq!(REGISTERED.lock().unwrap()[0].kind, KIND_SERVER);

    let (st, v, lines) = run(0, &[]);
    assert_eq!((st, v), (FishStatus::OK, 1));
    assert_eq!(
        lines,
        [("Alex Some(9) 2 Some([1.0, 2.0, 3.0])".to_string(), false)]
    );

    register_context(KIND_SERVER, 0)
        .register(
            literal("a").then(
                argument("n", ArgKind::Integer { min: 0, max: 9 }).executes(|c| {
                    c.feedback(&format!("n={}", c.get_integer("n")?));
                    c.get_string("missing").map(|_| 0)
                }),
            ),
        )
        .unwrap();
    let (_, _, lines) = run(0, &[("n", arg::INTEGER, "4")]);
    assert_eq!(lines[0], ("n=4".to_string(), false));
    assert_eq!(
        lines[1],
        ("the command has no argument \"missing\"".to_string(), true)
    );
}

#[test]
fn executors_report_wrong_types_bad_values_and_panics_without_unwinding() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    use crate::command::{ArgKind, CommandError, argument, literal};
    crate::fake::install();
    REGISTERED.lock().unwrap().clear();
    let go = |build: crate::command::CommandBuilder, args: &[(&str, u8, &str)]| {
        register_context(KIND_CLIENT, 1).register(build).unwrap();
        assert_eq!(REGISTERED.lock().unwrap().last().unwrap().kind, KIND_CLIENT);
        run(0, args)
    };
    let (_, _, l) = go(
        literal("t").then(argument("x", ArgKind::Word).executes(|c| c.get_integer("x").map(|_| 0))),
        &[("x", arg::WORD, "abc")],
    );
    assert_eq!(l, [("argument \"x\" has another type".to_string(), true)]);
    let (_, _, l) = go(
        literal("t").then(
            argument("x", ArgKind::Integer { min: 0, max: 9 })
                .executes(|c| c.get_double("x").map(|_| 0)),
        ),
        &[("x", arg::INTEGER, "5")],
    );
    assert_eq!(l[0].0, "argument \"x\" has another type");
    let (_, _, l) = go(
        literal("t").then(
            argument("x", ArgKind::Integer { min: 0, max: 9 })
                .executes(|c| c.get_integer("x").map(|_| 0)),
        ),
        &[("x", arg::INTEGER, "oops")],
    );
    assert_eq!(l[0].0, "argument \"x\" is not an integer");
    let (_, _, l) = go(
        literal("t").then(
            argument("x", ArgKind::Double { min: 0.0, max: 1.0 })
                .executes(|c| c.get_double("x").map(|_| 0)),
        ),
        &[("x", arg::DOUBLE, "zz")],
    );
    assert_eq!(l[0].0, "argument \"x\" is not a number");
    let (_, _, l) = go(
        literal("t")
            .then(argument("x", ArgKind::BlockPos).executes(|c| c.get_block_pos("x").map(|_| 0))),
        &[("x", arg::BLOCK_POS, "1 two 3")],
    );
    assert_eq!(l[0].0, "argument \"x\" is not a block position");
    let (_, v, l) = go(
        literal("t").executes(|_| Err(CommandError::from("nope"))),
        &[],
    );
    assert_eq!((v, l), (0, vec![("nope".to_string(), true)]));
    let (_, _, l) = go(
        literal("t").executes(|_| Err(String::from("owned").into())),
        &[],
    );
    assert_eq!(l[0].0, "owned");
    assert_eq!(CommandError::from("shown").to_string(), "shown");
    // an index the tree does not have
    register_context(KIND_SERVER, 0)
        .register(literal("t").executes(|_| Ok(0)))
        .unwrap();
    let (st, _, _) = run(5, &[]);
    assert_eq!(st, FishStatus::NOT_FOUND);
    // a panicking executor is caught at the boundary
    register_context(KIND_SERVER, 0)
        .register(literal("t").executes(|_| panic!("boom")))
        .unwrap();
    let (st, _, _) = run(0, &[]);
    assert_eq!(st, FishStatus::PANICKED);
}

#[test]
fn a_refused_registration_gives_the_executors_back_to_be_dropped() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    use crate::command::literal;
    crate::fake::install();
    REGISTERED.lock().unwrap().clear();
    *REGISTER_STATUS.lock().unwrap() = FishStatus::INVALID_ARGUMENT;
    let result = register_context(KIND_SERVER, 0).register(literal("t").executes(|_| Ok(0)));
    *REGISTER_STATUS.lock().unwrap() = FishStatus::OK;
    assert_eq!(
        result.unwrap_err().status(),
        Some(FishStatus::INVALID_ARGUMENT)
    );
    assert!(REGISTERED.lock().unwrap().is_empty());
    assert_eq!(
        register_context(KIND_SERVER, 1).side(),
        crate::loader::Environment::Server
    );
    assert_eq!(
        register_context(KIND_SERVER, 0).side(),
        crate::loader::Environment::Client
    );
}

#[test]
fn f3_lines_are_added_through_the_host() {
    use fish_abi::debug::DebugPayload;

    unsafe extern "C" fn add(user: *mut c_void, line: FishStr) {
        unsafe { (*user.cast::<Vec<String>>()).push(line.as_str().unwrap().to_string()) };
    }
    let mut lines: Vec<String> = Vec::new();
    let p = DebugPayload {
        size: size_of::<DebugPayload>() as u32,
        add,
        user: (&raw mut lines).cast(),
    };
    let c: crate::event::debug::DebugContext = unsafe { std::mem::transmute(p) };
    c.add_line("one");
    c.add_line("two");
    assert_eq!(lines, ["one", "two"]);
}
