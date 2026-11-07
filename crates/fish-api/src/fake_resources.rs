//! A fake `fish:resource` service and the tests of `resource` built on it.

use std::ffi::c_void;

use fish_abi::client::StrSink;
use fish_abi::resources::{BytesSink, ResourceServiceV1, SERVICE_RESOURCE, valid_path};
use fish_abi::{FishHandle, FishServiceHeader, FishStatus, FishStr};

use crate::client::ClientHandle;
use crate::resource::Resources;

/// The fake mod's files.
const OWN: [(&str, &[u8]); 3] = [
    ("assets/t/lang/en_us.json", b"{\"a\":\"b\"}"),
    ("assets/t/sounds.json", b"{}"),
    ("data/t/bin.dat", &[0xff, 0xfe]),
];

fn text(s: FishStr) -> String {
    // SAFETY: the API passes valid strings.
    unsafe { s.as_str() }.unwrap().to_string()
}

unsafe extern "C" fn read_own(
    _h: *mut c_void,
    _token: u64,
    path: FishStr,
    sink: BytesSink,
    user: *mut c_void,
) -> FishStatus {
    let path = text(path);
    if !valid_path(&path) {
        return FishStatus::INVALID_ARGUMENT;
    }
    match OWN.iter().find(|(n, _)| *n == path) {
        Some((_, bytes)) => {
            // SAFETY: the API's sink and user pointer.
            unsafe { sink(user, bytes.as_ptr(), bytes.len()) };
            FishStatus::OK
        }
        None => FishStatus::NOT_FOUND,
    }
}

unsafe extern "C" fn list_own(
    _h: *mut c_void,
    _token: u64,
    prefix: FishStr,
    sink: StrSink,
    user: *mut c_void,
) -> FishStatus {
    let prefix = text(prefix);
    for (name, _) in OWN.iter().filter(|(n, _)| n.starts_with(&prefix)) {
        // SAFETY: the API's sink; the string lives through the call.
        unsafe { sink(user, FishStr::from_static(name)) };
    }
    FishStatus::OK
}

/// The fake game: handle 1 has a resource root with one file, handle 2 has none.
unsafe extern "C" fn read_game(
    _h: *mut c_void,
    view: FishHandle,
    path: FishStr,
    sink: BytesSink,
    user: *mut c_void,
) -> FishStatus {
    let path = text(path);
    match (view.index, path.as_str()) {
        (2, _) => FishStatus::UNSUPPORTED,
        (1, "assets/minecraft/lang/en_us.json") => {
            let bytes = b"{\"game\":\"yes\"}";
            // SAFETY: the API's sink and user pointer.
            unsafe { sink(user, bytes.as_ptr(), bytes.len()) };
            FishStatus::OK
        }
        _ => FishStatus::NOT_FOUND,
    }
}

static RESOURCE: ResourceServiceV1 = ResourceServiceV1 {
    header: FishServiceHeader {
        size: size_of::<ResourceServiceV1>(),
        version: 1,
    },
    read_own,
    list_own,
    read_game,
};

/// The table for a service id of this fake.
pub(crate) fn lookup(id: &str) -> Option<*const c_void> {
    (id == SERVICE_RESOURCE).then_some((&raw const RESOURCE).cast())
}

fn installed() -> std::sync::MutexGuard<'static, ()> {
    let guard = crate::fake_rules::SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::fake::install();
    guard
}

#[test]
fn a_mod_reads_and_lists_its_own_files() {
    let _g = installed();
    assert_eq!(Resources::own("data/t/bin.dat").unwrap(), [0xff, 0xfe]);
    assert_eq!(
        Resources::own_text("assets/t/lang/en_us.json").unwrap(),
        "{\"a\":\"b\"}"
    );
    let err = Resources::own_text("data/t/bin.dat").unwrap_err();
    assert!(err.to_string().contains("not UTF-8"), "{err}");
    assert_eq!(
        Resources::own("assets/t/none").unwrap_err().status(),
        Some(FishStatus::NOT_FOUND)
    );
    assert_eq!(
        Resources::own("fish.mod.toml").unwrap_err().status(),
        Some(FishStatus::INVALID_ARGUMENT)
    );
    assert_eq!(
        Resources::list_own("assets/").unwrap(),
        ["assets/t/lang/en_us.json", "assets/t/sounds.json"]
    );
    assert!(Resources::list_own("data/x/").unwrap().is_empty());
}

#[test]
fn a_mod_reads_the_games_resources_through_a_game() {
    let _g = installed();
    let game = ClientHandle::from_raw(FishHandle {
        index: 1,
        generation: 1,
    });
    assert_eq!(
        Resources::game_text(&game, "assets/minecraft/lang/en_us.json").unwrap(),
        "{\"game\":\"yes\"}"
    );
    assert_eq!(
        Resources::game(&game, "assets/minecraft/x")
            .unwrap_err()
            .status(),
        Some(FishStatus::NOT_FOUND)
    );
    let server = ClientHandle::from_raw(FishHandle {
        index: 2,
        generation: 1,
    });
    assert_eq!(
        Resources::game(&server, "assets/minecraft/lang/en_us.json")
            .unwrap_err()
            .status(),
        Some(FishStatus::UNSUPPORTED)
    );
}
