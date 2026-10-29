//! Example mod: calls every host callback with valid and invalid arguments and logs the verdict
//! (`probe: ok` or `probe: FAILED: <checks>`). Used by the loader tests.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

use fish_api::FishLoader;
use fish_api::abi::services::{FishLoaderServiceV1, FishModInfoV1, SERVICE_LOADER};
use fish_api::abi::{FishHostV1, FishStatus, FishStr, ident};

static CALLS: AtomicU32 = AtomicU32::new(0);
static OBJECT: u32 = 5;
static SHARED: u32 = 9;

unsafe extern "C" fn counting(_: *mut c_void, _: *mut c_void, _: *mut c_void) -> FishStatus {
    CALLS.fetch_add(1, Ordering::SeqCst);
    FishStatus::OK
}

unsafe extern "C" fn complaining(_: *mut c_void, _: *mut c_void, _: *mut c_void) -> FishStatus {
    FishStatus::INVALID_ARGUMENT
}

unsafe extern "C" fn no_sink(_: *mut c_void, _: FishStr, _: *const c_void) {}

fn api() -> &'static u32 {
    &OBJECT
}

fn bad_str() -> FishStr {
    static BAD: [u8; 2] = [0xff, 0xfe];
    FishStr {
        ptr: BAD.as_ptr(),
        len: 2,
    }
}

fn s(v: &'static str) -> FishStr {
    FishStr::from_static(v)
}

fn init() {
    let mut failures: Vec<&'static str> = Vec::new();
    let mut check = |name: &'static str, ok: bool| {
        if !ok {
            failures.push(name);
        }
    };
    // SAFETY: the loader keeps the host table alive for the life of the mod.
    let h: &FishHostV1 = unsafe { &*fish_api::__host_table() };
    let tok = h.mod_token;
    // SAFETY (whole block): every call passes valid or deliberately invalid-but-memory-safe
    // arguments to the loader's callbacks, which must reject them with a status.
    unsafe {
        // --- services
        let mut out: *const c_void = std::ptr::null();
        check(
            "svc unknown",
            (h.get_service)(h.host, tok, s("fish:nothing"), 1, &mut out) == FishStatus::NOT_FOUND,
        );
        check(
            "svc version",
            (h.get_service)(h.host, tok, s(SERVICE_LOADER), 2, &mut out)
                == FishStatus::VERSION_MISMATCH,
        );
        check(
            "svc utf8",
            (h.get_service)(h.host, tok, bad_str(), 1, &mut out) == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "svc null out",
            (h.get_service)(h.host, tok, s(SERVICE_LOADER), 1, std::ptr::null_mut())
                == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "svc ok",
            (h.get_service)(h.host, tok, s(SERVICE_LOADER), 1, &mut out) == FishStatus::OK
                && !out.is_null(),
        );
        let svc: &FishLoaderServiceV1 = &*out.cast();

        // --- events
        check(
            "define invalid",
            (h.event_define)(h.host, tok, s("Bad Id"), 0) == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "define combine",
            (h.event_define)(h.host, tok, s("probe:ev"), 9) == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "define utf8",
            (h.event_define)(h.host, tok, bad_str(), 0) == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "define ok",
            (h.event_define)(h.host, tok, s("probe:ev"), 0) == FishStatus::OK,
        );
        check(
            "define again",
            (h.event_define)(h.host, tok, s("probe:ev"), 0) == FishStatus::OK,
        );
        check(
            "define clash",
            (h.event_define)(h.host, tok, s("probe:ev"), 1) == FishStatus::ALREADY_EXISTS,
        );

        let mut index = u32::MAX;
        check(
            "lookup ok",
            (h.event_lookup)(h.host, s("probe:ev"), &mut index) == FishStatus::OK
                && index != u32::MAX,
        );
        check(
            "lookup missing",
            (h.event_lookup)(h.host, s("probe:none"), &mut index) == FishStatus::NOT_FOUND,
        );
        check(
            "lookup null",
            (h.event_lookup)(h.host, s("probe:ev"), std::ptr::null_mut())
                == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "lookup utf8",
            (h.event_lookup)(h.host, bad_str(), &mut index) == FishStatus::INVALID_ARGUMENT,
        );

        let reg = |id, phase, f, token| {
            (h.event_register)(h.host, token, id, phase, f, std::ptr::null_mut(), None)
        };
        check(
            "register missing",
            reg(s("probe:none"), s("fish:default"), counting as _, tok) == FishStatus::NOT_FOUND,
        );
        check(
            "register phase",
            reg(s("probe:ev"), s("bad phase"), counting as _, tok) == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "register utf8",
            reg(bad_str(), s("fish:default"), counting as _, tok) == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "register token",
            reg(s("probe:ev"), s("fish:default"), counting as _, u64::MAX)
                == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "register ok",
            reg(s("probe:ev"), s("fish:default"), counting as _, tok) == FishStatus::OK,
        );
        check(
            "register complaining",
            reg(s("probe:ev"), s("probe:late"), complaining as _, tok) == FishStatus::OK,
        );

        check(
            "order missing",
            (h.event_phase_order)(h.host, tok, s("probe:none"), s("a:b"), s("c:d"))
                == FishStatus::NOT_FOUND,
        );
        check(
            "order invalid",
            (h.event_phase_order)(h.host, tok, s("probe:ev"), s("bad phase"), s("c:d"))
                == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "order utf8",
            (h.event_phase_order)(h.host, tok, bad_str(), s("a:b"), s("c:d"))
                == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "order ok",
            (h.event_phase_order)(
                h.host,
                tok,
                s("probe:ev"),
                s("fish:default"),
                s("probe:late"),
            ) == FishStatus::OK,
        );

        let before = CALLS.load(Ordering::SeqCst);
        check(
            "invoke missing",
            (h.event_invoke)(
                h.host,
                tok,
                s("probe:none"),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) == FishStatus::NOT_FOUND,
        );
        check(
            "invoke utf8",
            (h.event_invoke)(
                h.host,
                tok,
                bad_str(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "invoke ok",
            (h.event_invoke)(
                h.host,
                tok,
                s("probe:ev"),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) == FishStatus::OK,
        );
        check(
            "invoke index ok",
            (h.event_invoke_index)(
                h.host,
                tok,
                index,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) == FishStatus::OK,
        );
        check(
            "invoke index missing",
            (h.event_invoke_index)(
                h.host,
                tok,
                99_999,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) == FishStatus::NOT_FOUND,
        );
        check("listeners ran", CALLS.load(Ordering::SeqCst) == before + 2);

        // --- loader service
        let mut info: FishModInfoV1 = std::mem::zeroed();
        check("mod_count", (svc.mod_count)(h.host) >= 1);
        check(
            "mod_at range",
            (svc.mod_at)(h.host, 9_999, &mut info) == FishStatus::NOT_FOUND,
        );
        check(
            "mod_at null",
            (svc.mod_at)(h.host, 0, std::ptr::null_mut()) == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "find utf8",
            (svc.find_mod)(h.host, bad_str(), &mut info) == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "find null",
            (svc.find_mod)(h.host, s("probe"), std::ptr::null_mut())
                == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "find missing",
            (svc.find_mod)(h.host, s("nope"), &mut info) == FishStatus::NOT_FOUND,
        );
        check(
            "find ok",
            (svc.find_mod)(h.host, s("probe"), &mut info) == FishStatus::OK,
        );

        let value = std::ptr::from_ref(&SHARED).cast_mut().cast::<c_void>();
        check(
            "put empty key",
            (svc.object_share_put)(h.host, s(""), s("t"), value) == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "put null value",
            (svc.object_share_put)(h.host, s("probe:k"), s("t"), std::ptr::null_mut())
                == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "put utf8",
            (svc.object_share_put)(h.host, bad_str(), s("t"), value)
                == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "put ok",
            (svc.object_share_put)(h.host, s("probe:k"), s("t"), value) == FishStatus::OK,
        );
        check(
            "put twice",
            (svc.object_share_put)(h.host, s("probe:k"), s("t"), value)
                == FishStatus::ALREADY_EXISTS,
        );
        let mut got: *mut c_void = std::ptr::null_mut();
        check(
            "get null",
            (svc.object_share_get)(h.host, s("probe:k"), s("t"), std::ptr::null_mut())
                == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "get utf8",
            (svc.object_share_get)(h.host, bad_str(), s("t"), &mut got)
                == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "get ok",
            (svc.object_share_get)(h.host, s("probe:k"), s("t"), &mut got) == FishStatus::OK
                && got == value,
        );
        check(
            "entrypoints utf8",
            (svc.entrypoints)(h.host, bad_str(), no_sink, std::ptr::null_mut())
                == FishStatus::INVALID_ARGUMENT,
        );
        check(
            "entrypoints ok",
            (svc.entrypoints)(h.host, s("probe:none"), no_sink, std::ptr::null_mut())
                == FishStatus::OK,
        );

        // --- log with a bad message and an unknown token must not fail
        (h.log)(h.host, 9_999, 3, s("t"), bad_str());
        (h.log)(h.host, tok, 5, s("t"), s("trace-level message"));
    }

    // --- safe wrappers against the real host
    let l = FishLoader::get();
    check(
        "wrap find",
        l.find_mod("probe")
            .is_some_and(|m| m.version == "0.1.0" && m.name == "Probe"),
    );
    check("wrap find missing", l.find_mod("nope").is_none());
    check("wrap loaded", l.is_mod_loaded("probe"));
    check("wrap mods", l.mods().iter().any(|m| m.id == "probe"));
    check(
        "wrap env",
        l.environment() == fish_api::loader::Environment::Client,
    );
    check("wrap dev", l.is_development());
    check(
        "wrap dirs",
        l.config_dir().ends_with("config") && l.game_dir().is_dir(),
    );
    let objects = l.entrypoint_objects::<u32>("probe:obj");
    check(
        "wrap objects",
        objects.len() == 1 && objects[0].0 == "probe" && *objects[0].1 == 5,
    );
    check("ident", ident::is_valid_identifier("probe:ev"));

    if failures.is_empty() {
        log::info!("probe: ok");
    } else {
        log::error!("probe: FAILED: {}", failures.join(", "));
    }
}

fish_api::export_mod! {
    main: [init],
    "probe:obj": object [api],
}
