//! Host side of `fish:block`, and the loader's block operations the hooks use.

use std::collections::HashMap;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use fish_abi::blocks::{
    BlockHandlerFn, BlockPayloadV1, BlockServiceV1, BlockSettingsV1, HANDLER_PANICKED, event, set,
};
use fish_abi::{ActionResult, DropUserFn, FishHandle, FishServiceHeader, FishStatus, FishStr};
use fish_hooks::{BlockDef, BlockEvent, BlockEventKind, BlockRun, ClientView, Outcome};
use fish_metadata::Origin;
use fish_pack::FishArchive;

use crate::blocks::{BlockEntry, ContentMod, UserBox, check_def, check_id, write_content};
use crate::client_services::role_byte;
use crate::inner::Inner;

fn guard(f: impl FnOnce() -> FishStatus) -> FishStatus {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(FishStatus::PANICKED)
}

/// Reads what a mod registered into a block definition; `Err` is the reason.
///
/// # Safety
/// The strings of `s` must be valid for the call.
unsafe fn read_def(
    id: &str,
    mod_id: &str,
    s: &BlockSettingsV1,
    events: u32,
    has_handler: bool,
) -> Result<BlockDef, String> {
    if (s.size as usize) < size_of::<BlockSettingsV1>() {
        return Err("the settings table is too small".to_string());
    }
    if s.set & !set::ALL != 0 {
        return Err(format!("the settings set unknown bits ({:#x})", s.set));
    }
    if events & !event::ALL != 0 {
        return Err(format!("the events mask {events:#x} has unknown events"));
    }
    // SAFETY: valid strings for the call (the caller's contract).
    let template = unsafe { s.template.as_str() }
        .map_err(|_| "the template is not UTF-8".to_string())?
        .to_string();
    let sounds = if s.set & set::SOUNDS != 0 {
        // SAFETY: as above.
        Some(
            unsafe { s.sounds.as_str() }
                .map_err(|_| "the sounds are not UTF-8".to_string())?
                .to_string(),
        )
    } else {
        None
    };
    let pick = |bit: u32| s.set & bit != 0;
    let def = BlockDef {
        id: id.to_string(),
        mod_id: mod_id.to_string(),
        template,
        hardness: pick(set::HARDNESS).then_some(s.hardness),
        resistance: pick(set::RESISTANCE).then_some(s.resistance),
        light: pick(set::LIGHT).then_some(s.light),
        requires_tool: pick(set::REQUIRES_TOOL).then_some(s.requires_tool != 0),
        friction: pick(set::FRICTION).then_some(s.friction),
        map_color: pick(set::MAP_COLOR).then_some(s.map_color),
        sounds,
        // A block without a handler has no events to ask for.
        events: if has_handler { events } else { 0 },
    };
    check_def(&def)?;
    Ok(def)
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn register(
    host: *mut c_void,
    token: u64,
    id: FishStr,
    settings: *const BlockSettingsV1,
    events: u32,
    handler: Option<BlockHandlerFn>,
    user: *mut c_void,
    drop_user: Option<DropUserFn>,
) -> FishStatus {
    // Owns `user` from here on: every refusal below drops it.
    let user = UserBox::new(user, drop_user);
    guard(|| {
        // SAFETY: ABI contract: `host` is the loader's `Inner`.
        let inner = unsafe { &*host.cast::<Inner>() };
        // SAFETY: valid string for the call.
        let Ok(id) = (unsafe { id.as_str() }) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        if settings.is_null() {
            return FishStatus::INVALID_ARGUMENT;
        }
        let Some(rec) = inner.record(token) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        let who = rec.manifest.name.clone();
        if id.starts_with("minecraft:") {
            inner.warn(format!(
                "{who} registered the block {id}: the minecraft namespace belongs to the game"
            ));
            return FishStatus::INVALID_ARGUMENT;
        }
        if let Err(why) = check_id(id, rec.manifest.id.as_str()) {
            inner.warn(format!("{who} registered the invalid block {id}: {why}"));
            return FishStatus::INVALID_ARGUMENT;
        }
        if inner.net.frozen.load(Ordering::Acquire) {
            inner.warn(format!(
                "registries are frozen: the block {id} of {who} was not added"
            ));
            return FishStatus::INVALID_ARGUMENT;
        }
        // SAFETY: the table and its strings are valid for the call (ABI contract).
        let def = match unsafe {
            read_def(
                id,
                rec.manifest.id.as_str(),
                &*settings,
                events,
                handler.is_some(),
            )
        } {
            Ok(def) => def,
            Err(why) => {
                inner.warn(format!("{who} registered the invalid block {id}: {why}"));
                return FishStatus::INVALID_ARGUMENT;
            }
        };
        let added = inner
            .blocks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .register(BlockEntry {
                def,
                owner: token,
                handler,
                user,
            });
        match added {
            Ok(()) => FishStatus::OK,
            Err(first) => {
                let first = inner
                    .record(first)
                    .map_or_else(|| "another mod".to_string(), |r| r.manifest.name.clone());
                inner.warn(format!(
                    "{who} registered the block {id}, which {first} already registered"
                ));
                FishStatus::ALREADY_EXISTS
            }
        }
    })
}

unsafe extern "C" fn state_range(
    host: *mut c_void,
    id: FishStr,
    first: *mut u32,
    count: *mut u32,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract.
        let inner = unsafe { &*host.cast::<Inner>() };
        // SAFETY: valid string for the call.
        let Ok(id) = (unsafe { id.as_str() }) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        if first.is_null() || count.is_null() {
            return FishStatus::INVALID_ARGUMENT;
        }
        let range = inner
            .blocks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .range(id);
        let Some((f, c)) = range else {
            return FishStatus::NOT_FOUND;
        };
        // SAFETY: both pointers were checked non-null and are writable (ABI contract).
        unsafe {
            *first = f;
            *count = c;
        }
        FishStatus::OK
    })
}

/// The no-game handle of an event raised without a game.
const NO_GAME: FishHandle = FishHandle {
    index: u32::MAX,
    generation: 0,
};

impl Inner {
    /// The blocks of the mods that did not fail, in registration order.
    pub(crate) fn blocks_list(&self, out: &mut Vec<BlockDef>) {
        let entries = self
            .blocks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries();
        out.extend(
            entries
                .iter()
                .filter(|e| !self.owner_failed(e.owner))
                .map(|e| e.def.clone()),
        );
    }

    fn owner_failed(&self, token: u64) -> bool {
        self.record(token).is_some_and(|r| r.is_failed())
    }

    /// Writes the content mod of every mod that did not fail and registered blocks, and returns the
    /// folders.
    pub(crate) fn blocks_content(&self) -> Vec<PathBuf> {
        let entries = self
            .blocks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries();
        let mut groups: Vec<(u64, Vec<&BlockDef>)> = Vec::new();
        for e in entries.iter().filter(|e| !self.owner_failed(e.owner)) {
            match groups.iter_mut().find(|(owner, _)| *owner == e.owner) {
                Some((_, defs)) => defs.push(&e.def),
                None => groups.push((e.owner, vec![&e.def])),
            }
        }
        let root = Path::new(&self.game_dir).join(".fish").join("content");
        let (mut folders, mut warnings) = (Vec::new(), Vec::new());
        for (owner, defs) in groups {
            let Some(rec) = self.record(owner) else {
                continue;
            };
            let m = ContentMod {
                id: rec.manifest.id.as_str(),
                name: &rec.manifest.name,
                version: &rec.version_text,
                description: &rec.manifest.description,
            };
            let mut store = self.blocks.lock().unwrap_or_else(|e| e.into_inner());
            let archive = store.archive_mut(owner);
            if archive.is_none() {
                warnings.push(format!(
                    "the archive of {} is not available: its assets are not copied for its blocks",
                    m.name
                ));
            }
            match write_content(&root, &m, &defs, archive) {
                Ok((folder, mut w)) => {
                    folders.push(folder);
                    warnings.append(&mut w);
                }
                Err(why) => {
                    warnings.push(format!("the blocks of {} were not added: {why}", m.name))
                }
            }
        }
        for w in warnings {
            self.warn(w);
        }
        folders
    }

    /// Keeps the archives of the mods that registered blocks (their assets are copied later).
    pub(crate) fn keep_block_archives(&self, archives: &mut HashMap<Origin, FishArchive>) {
        let mut store = self.blocks.lock().unwrap_or_else(|e| e.into_inner());
        for rec in self.records() {
            if store.has_blocks_of(rec.token)
                && let Some(archive) = archives.remove(&rec.origin)
            {
                store.keep_archive(rec.token, archive);
            }
        }
    }

    /// Horizon built the block states: remembers the runs of the Fish blocks.
    pub(crate) fn blocks_built(&self, runs: &[BlockRun]) {
        let (built, any) = {
            let mut store = self.blocks.lock().unwrap_or_else(|e| e.into_inner());
            let any = !store.entries().is_empty();
            (store.set_runs(runs), any)
        };
        if built.kept > 0 {
            self.emit(
                log::Level::Info,
                "fish_loader",
                format!(
                    "Fish blocks: {} block(s), states {}..{}",
                    built.kept, built.first, built.end
                ),
            );
        }
        if any {
            for id in &built.missing {
                let owner = id.split(':').next().unwrap_or_default();
                self.warn(format!(
                    "block {id} of {owner} has no states: Horizon did not add it"
                ));
            }
        }
        for id in &built.repeated {
            self.warn(format!(
                "block {id} has states in more than one run: only the first run is used"
            ));
        }
    }

    /// True when `state` belongs to a Fish block.
    pub(crate) fn block_is_state(&self, state: u32) -> bool {
        self.blocks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .find(state)
            .is_some()
    }

    /// Calls the handler of the block that owns `ev.state`.
    pub(crate) fn block_event(
        &self,
        game: Option<&mut dyn ClientView>,
        ev: &BlockEvent<'_>,
    ) -> Outcome {
        let found = self
            .blocks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .find(ev.state);
        let Some((entry, first)) = found else {
            return Outcome::Pass;
        };
        let kind = ev.kind as u8;
        let Some(handler) = entry.handler else {
            return Outcome::Pass;
        };
        if entry.def.events & (1 << kind) == 0 || self.owner_failed(entry.owner) {
            return Outcome::Pass;
        }
        let call = |view: FishHandle| -> u8 {
            let payload = BlockPayloadV1 {
                size: size_of::<BlockPayloadV1>() as u32,
                event: kind,
                role: role_byte(ev.role),
                hand: ev.hand,
                face: ev.face,
                state: ev.state,
                state_index: ev.state - first,
                pos: ev.pos,
                other_pos: ev.other_pos,
                actor_uuid: ev.actor_uuid,
                block: FishStr::new(&entry.def.id),
                actor: FishStr::new(ev.actor),
                dimension: FishStr::new(ev.dimension),
                item: FishStr::new(ev.item),
                view,
            };
            // SAFETY: the handler and its user pointer were registered together; the payload and
            // its strings live through the call.
            unsafe { handler(entry.user.ptr(), kind, &raw const payload) }
        };
        let result = match game {
            Some(g) => self.handles.client_scope(g, call),
            None => call(NO_GAME),
        };
        if result == HANDLER_PANICKED {
            self.fail_mod(
                entry.owner,
                format!(
                    "block {} panicked handling {}",
                    entry.def.id,
                    ev.kind.name()
                ),
                true,
            );
            return Outcome::Pass;
        }
        if ev.kind != BlockEventKind::Use {
            return Outcome::Pass;
        }
        match ActionResult::from_u8(result) {
            Some(ActionResult::Pass) | None => Outcome::Pass,
            Some(ActionResult::Fail) => Outcome::Cancel,
            Some(_) => Outcome::Handled,
        }
    }
}

/// The `fish:block` table.
pub(crate) static BLOCK: BlockServiceV1 = BlockServiceV1 {
    header: FishServiceHeader {
        size: size_of::<BlockServiceV1>(),
        version: 1,
    },
    register,
    state_range,
};

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32};
    use std::sync::{Arc, Mutex};

    use fish_hooks::{PlayerSnapshot, Role};
    use fish_metadata::Side;

    use super::*;
    use crate::boot::{BootConfig, Loader, boot};

    /// A booted loader with the mods `ruby` ("Ruby") and `gem` (no `assets`), and its host pointer.
    #[allow(clippy::type_complexity)]
    fn host() -> (Loader, *mut c_void, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("fish-mods")).unwrap();
        let files: [(&str, &str, Vec<(&str, &[u8])>); 2] = [
            (
                "ruby",
                "Ruby",
                vec![
                    ("assets/ruby/lang/en_us.json", b"{}".as_slice()),
                    ("assets/foreign/x.json", b"{}".as_slice()),
                    ("data/c/tags/block/gems.json", b"{}".as_slice()),
                ],
            ),
            ("gem", "Gem", Vec::new()),
        ];
        for (id, name, files) in files {
            let mut spec = fish_pack::PackSpec::new(format!(
                "schema = 1\nid = \"{id}\"\nversion = \"1.2.3\"\nname = \"{name}\"\ndescription = \"About {name}\"\n"
            ));
            for (n, b) in files {
                spec.add_bytes(n, b.to_vec()).unwrap();
            }
            fish_pack::write(
                spec,
                &dir.path().join("fish-mods").join(format!("{id}.fish")),
            )
            .unwrap();
        }
        let mut cfg = BootConfig::new(Side::Client, dir.path());
        cfg.log_sink = Some(Arc::new(|_| {}));
        let loader = boot(cfg).unwrap();
        // Entrypoints are over, so the registries are frozen: open them for these tests.
        loader.inner.net.frozen.store(false, Ordering::Release);
        let ptr = Arc::as_ptr(&loader.inner).cast_mut().cast();
        (loader, ptr, dir)
    }

    fn token(loader: &Loader, id: &str) -> u64 {
        loader
            .inner
            .records()
            .iter()
            .find(|r| r.manifest.id.as_str() == id)
            .unwrap()
            .token
    }

    fn settings(template: &'static str) -> BlockSettingsV1 {
        BlockSettingsV1 {
            size: size_of::<BlockSettingsV1>() as u32,
            set: 0,
            hardness: 0.0,
            resistance: 0.0,
            friction: 0.0,
            light: 0,
            requires_tool: 0,
            map_color: 0,
            reserved: 0,
            template: FishStr::from_static(template),
            sounds: FishStr::EMPTY,
        }
    }

    fn with(f: impl FnOnce(&mut BlockSettingsV1)) -> BlockSettingsV1 {
        let mut s = settings("minecraft:stone");
        f(&mut s);
        s
    }

    #[allow(clippy::too_many_arguments)]
    fn reg(
        host: *mut c_void,
        token: u64,
        id: &str,
        s: &BlockSettingsV1,
        events: u32,
        handler: Option<BlockHandlerFn>,
        user: *mut c_void,
        drop_user: Option<DropUserFn>,
    ) -> FishStatus {
        // SAFETY: the host pointer is a live `Inner`; the strings and the table outlive the call.
        unsafe {
            register(
                host,
                token,
                FishStr::new(id),
                s,
                events,
                handler,
                user,
                drop_user,
            )
        }
    }

    fn plain(host: *mut c_void, token: u64, id: &str) -> FishStatus {
        reg(
            host,
            token,
            id,
            &settings("minecraft:stone"),
            0,
            None,
            std::ptr::null_mut(),
            None,
        )
    }

    fn warned(loader: &Loader, text: &str) -> bool {
        loader
            .inner
            .warnings
            .lock()
            .unwrap()
            .iter()
            .any(|w| w.contains(text))
    }

    fn range(host: *mut c_void, id: &str) -> (FishStatus, u32, u32) {
        let (mut f, mut c) = (7, 7);
        // SAFETY: a live host and writable outputs.
        let st = unsafe { state_range(host, FishStr::new(id), &mut f, &mut c) };
        (st, f, c)
    }

    #[test]
    fn bad_registrations_are_refused_with_a_warning_naming_the_block() {
        let (loader, h, _dir) = host();
        let t = token(&loader, "ruby");
        let ok = settings("minecraft:stone");
        let check = |id: &str, s: &BlockSettingsV1, why: &str| {
            let st = reg(h, t, id, s, 0, None, std::ptr::null_mut(), None);
            assert_eq!(st, FishStatus::INVALID_ARGUMENT, "{id}");
            assert!(
                warned(&loader, why),
                "{why} not in {:?}",
                loader.inner.warnings.lock().unwrap()
            );
        };
        check("minecraft:stone2", &ok, "belongs to the game");
        check("nocolon", &ok, "namespace:path");
        check("gem:x", &ok, "is not the id of the mod");
        check("ruby:Bad", &ok, "the path must be");
        check("ruby:small", &with(|s| s.size = 4), "too small");
        check("ruby:bits", &with(|s| s.set = 1 << 20), "unknown bits");
        check("ruby:tmpl", &settings("stone"), "must be minecraft:<path>");
        check(
            "ruby:hard",
            &with(|s| {
                s.set = set::HARDNESS;
                s.hardness = -3.0;
            }),
            "hardness",
        );
        check(
            "ruby:res",
            &with(|s| {
                s.set = set::RESISTANCE;
                s.resistance = f32::NAN;
            }),
            "resistance",
        );
        check(
            "ruby:fric",
            &with(|s| {
                s.set = set::FRICTION;
                s.friction = 3.0;
            }),
            "friction",
        );
        check(
            "ruby:light",
            &with(|s| {
                s.set = set::LIGHT;
                s.light = 16;
            }),
            "light",
        );
        check(
            "ruby:color",
            &with(|s| {
                s.set = set::MAP_COLOR;
                s.map_color = 62;
            }),
            "map colour",
        );
        check(
            "ruby:sound",
            &with(|s| {
                s.set = set::SOUNDS;
                s.sounds = FishStr::from_static("block.stone");
            }),
            "sounds",
        );
        let bad_utf8 = [0xff_u8];
        check(
            "ruby:utf8",
            &with(|s| {
                s.set = set::SOUNDS;
                s.sounds = FishStr {
                    ptr: bad_utf8.as_ptr(),
                    len: 1,
                };
            }),
            "not UTF-8",
        );
        let st = reg(
            h,
            t,
            "ruby:events",
            &ok,
            1 << 20,
            None,
            std::ptr::null_mut(),
            None,
        );
        assert_eq!(st, FishStatus::INVALID_ARGUMENT);
        assert!(warned(&loader, "unknown events"));
        // a null table, a null id and an unknown mod
        // SAFETY: the host pointer is live; a null table is checked for.
        let null = unsafe {
            register(
                h,
                t,
                FishStr::new("ruby:n"),
                std::ptr::null(),
                0,
                None,
                std::ptr::null_mut(),
                None,
            )
        };
        assert_eq!(null, FishStatus::INVALID_ARGUMENT);
        let bad_id = FishStr {
            ptr: bad_utf8.as_ptr(),
            len: 1,
        };
        // SAFETY: as above.
        let st = unsafe { register(h, t, bad_id, &ok, 0, None, std::ptr::null_mut(), None) };
        assert_eq!(st, FishStatus::INVALID_ARGUMENT);
        assert_eq!(plain(h, 99, "ruby:ghost"), FishStatus::INVALID_ARGUMENT);
        assert!(loader.inner.blocks.lock().unwrap().entries().is_empty());
    }

    #[test]
    fn registration_keeps_the_first_block_and_stops_at_the_freeze() {
        let (loader, h, _dir) = host();
        let t = token(&loader, "ruby");
        assert_eq!(plain(h, t, "ruby:one"), FishStatus::OK);
        assert_eq!(plain(h, t, "ruby:one"), FishStatus::ALREADY_EXISTS);
        assert!(warned(&loader, "ruby:one, which Ruby already registered"));
        loader.inner.net.frozen.store(true, Ordering::Release);
        assert_eq!(plain(h, t, "ruby:late"), FishStatus::INVALID_ARGUMENT);
        assert!(warned(&loader, "registries are frozen") && warned(&loader, "ruby:late"));
        assert_eq!(loader.inner.blocks.lock().unwrap().entries().len(), 1);
    }

    #[test]
    fn settings_become_the_definition_and_unset_ones_stay_none() {
        let (loader, h, _dir) = host();
        let t = token(&loader, "ruby");
        let mut s = settings("minecraft:amethyst_block");
        s.set = set::ALL;
        s.hardness = 1.5;
        s.resistance = 9.0;
        s.friction = 0.5;
        s.light = 12;
        s.requires_tool = 1;
        s.map_color = 20;
        s.sounds = FishStr::from_static("minecraft:block.glass");
        assert_eq!(
            reg(h, t, "ruby:full", &s, 0, None, std::ptr::null_mut(), None),
            FishStatus::OK
        );
        assert_eq!(plain(h, t, "ruby:bare"), FishStatus::OK);
        let mut defs = Vec::new();
        loader.inner.blocks_list(&mut defs);
        assert_eq!(defs.len(), 2);
        assert_eq!(
            defs[0],
            BlockDef {
                id: "ruby:full".into(),
                mod_id: "ruby".into(),
                template: "minecraft:amethyst_block".into(),
                hardness: Some(1.5),
                resistance: Some(9.0),
                light: Some(12),
                requires_tool: Some(true),
                friction: Some(0.5),
                map_color: Some(20),
                sounds: Some("minecraft:block.glass".into()),
                events: 0,
            }
        );
        assert_eq!(
            (defs[1].hardness, defs[1].light, defs[1].sounds.clone()),
            (None, None, None)
        );
    }

    /// What the test handler records and answers.
    struct Probe {
        calls: Mutex<Vec<String>>,
        answer: AtomicU8,
        with_view: AtomicBool,
    }

    impl Probe {
        fn new(answer: u8) -> Box<Probe> {
            Box::new(Probe {
                calls: Mutex::new(Vec::new()),
                answer: AtomicU8::new(answer),
                with_view: AtomicBool::new(false),
            })
        }
    }

    unsafe extern "C" fn probe_handler(
        user: *mut c_void,
        event: u8,
        payload: *const BlockPayloadV1,
    ) -> u8 {
        // SAFETY: `user` is the `Probe` the tests pass; the payload is valid for the call.
        let (probe, p) = unsafe { (&*user.cast::<Probe>(), &*payload) };
        // SAFETY: the strings are valid for the call.
        let (block, actor, dim, item) = unsafe {
            (
                p.block.as_str().unwrap(),
                p.actor.as_str().unwrap(),
                p.dimension.as_str().unwrap(),
                p.item.as_str().unwrap(),
            )
        };
        if p.view.index != u32::MAX {
            probe.with_view.store(true, Ordering::SeqCst);
        }
        probe.calls.lock().unwrap().push(format!(
            "{event} {block} state={} index={} pos={:?} other={:?} face={} hand={} role={} actor={actor}/{} dim={dim} item={item} size={}",
            p.state,
            p.state_index,
            p.pos,
            p.other_pos,
            p.face,
            p.hand,
            p.role,
            p.actor_uuid[0],
            p.size
        ));

        probe.answer.load(Ordering::SeqCst)
    }

    unsafe extern "C" fn flagging_handler(_: *mut c_void, _: u8, _: *const BlockPayloadV1) -> u8 {
        HANDLER_PANICKED
    }

    struct View;
    impl ClientView for View {
        fn role(&self) -> Role {
            Role::Window
        }
        fn player(&self) -> Option<PlayerSnapshot> {
            None
        }
        fn show_chat(&mut self, _: &str) {}
        fn game_dir(&self) -> PathBuf {
            PathBuf::new()
        }
    }

    fn event(kind: BlockEventKind, state: u32) -> BlockEvent<'static> {
        let mut e = BlockEvent::new(kind, state, [1, 2, 3]);
        e.other_pos = [4, 5, 6];
        e.face = 1;
        e.hand = 1;
        e.actor = "Steve";
        e.actor_uuid = [9; 16];
        e.item = "minecraft:stick";
        e
    }

    const ALL_EVENTS: u32 = event::ALL;

    #[test]
    fn states_are_found_and_events_reach_the_handler_with_its_payload() {
        let (loader, h, _dir) = host();
        let t = token(&loader, "ruby");
        let probe = Probe::new(ActionResult::Pass.as_u8());
        let user = (&raw const *probe).cast_mut().cast();
        let s = settings("minecraft:stone");
        assert_eq!(
            reg(
                h,
                t,
                "ruby:slab",
                &s,
                ALL_EVENTS,
                Some(probe_handler),
                user,
                None
            ),
            FishStatus::OK
        );
        assert_eq!(plain(h, t, "ruby:plain"), FishStatus::OK);

        // before the runs are known
        assert_eq!(range(h, "ruby:slab").0, FishStatus::NOT_FOUND);
        assert!(!loader.inner.block_is_state(100));
        assert_eq!(
            loader
                .inner
                .block_event(None, &event(BlockEventKind::Use, 100)),
            Outcome::Pass
        );

        let run = |block: &str, first, count| BlockRun {
            block: block.into(),
            first,
            count,
        };
        loader
            .inner
            .blocks_built(&[run("ruby:slab", 100, 3), run("ruby:plain", 103, 1)]);
        assert_eq!(range(h, "ruby:slab"), (FishStatus::OK, 100, 3));
        assert_eq!(range(h, "ruby:plain"), (FishStatus::OK, 103, 1));
        assert_eq!(range(h, "ruby:none").0, FishStatus::NOT_FOUND);
        assert_eq!(range(h, "ruby:slab").0, FishStatus::OK);
        for (state, is) in [
            (99, false),
            (100, true),
            (102, true),
            (103, true),
            (104, false),
        ] {
            assert_eq!(loader.inner.block_is_state(state), is, "{state}");
        }

        // the payload, without a game
        let out = loader
            .inner
            .block_event(None, &event(BlockEventKind::NeighborChanged, 102));
        assert_eq!(out, Outcome::Pass);
        assert_eq!(
            probe.calls.lock().unwrap().last().unwrap(),
            &format!(
                "3 ruby:slab state=102 index=2 pos=[1, 2, 3] other=[4, 5, 6] face=1 hand=1 role={} actor=Steve/9 dim=minecraft:overworld item=minecraft:stick size={}",
                role_byte(Role::Window),
                size_of::<BlockPayloadV1>()
            )
        );
        assert!(!probe.with_view.load(Ordering::SeqCst));

        // with a game the handle is live
        let mut view = View;
        loader
            .inner
            .block_event(Some(&mut view), &event(BlockEventKind::Place, 100));
        assert!(probe.with_view.load(Ordering::SeqCst));
        assert!(
            probe
                .calls
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .starts_with("1 ruby:slab state=100 index=0")
        );

        // a block without a handler gets nothing
        assert_eq!(
            loader
                .inner
                .block_event(None, &event(BlockEventKind::Use, 103)),
            Outcome::Pass
        );
        assert_eq!(probe.calls.lock().unwrap().len(), 2);
    }

    #[test]
    fn only_use_decides_and_results_map_to_outcomes() {
        let (loader, h, _dir) = host();
        let t = token(&loader, "ruby");
        let probe = Probe::new(0);
        let user = (&raw const *probe).cast_mut().cast();
        let s = settings("minecraft:stone");
        let use_only = 1 << event::USE;
        assert_eq!(
            reg(
                h,
                t,
                "ruby:a",
                &s,
                use_only,
                Some(probe_handler),
                user,
                None
            ),
            FishStatus::OK
        );
        loader.inner.blocks_built(&[BlockRun {
            block: "ruby:a".into(),
            first: 50,
            count: 1,
        }]);
        let outcome = |kind| loader.inner.block_event(None, &event(kind, 50));
        for (answer, expected) in [
            (ActionResult::Pass.as_u8(), Outcome::Pass),
            (ActionResult::Fail.as_u8(), Outcome::Cancel),
            (ActionResult::Success.as_u8(), Outcome::Handled),
            (ActionResult::Consume.as_u8(), Outcome::Handled),
            (77, Outcome::Pass),
        ] {
            probe.answer.store(answer, Ordering::SeqCst);
            assert_eq!(outcome(BlockEventKind::Use), expected, "answer {answer}");
        }
        // other events are masked out of this block, and never decide anyway
        let calls = probe.calls.lock().unwrap().len();
        for kind in &BlockEventKind::ALL[1..] {
            assert_eq!(outcome(*kind), Outcome::Pass);
        }
        assert_eq!(
            probe.calls.lock().unwrap().len(),
            calls,
            "masked events are not delivered"
        );

        // a block that wants everything: only Use decides
        let all = Probe::new(ActionResult::Fail.as_u8());
        let user = (&raw const *all).cast_mut().cast();
        assert_eq!(
            reg(
                h,
                t,
                "ruby:b",
                &s,
                ALL_EVENTS,
                Some(probe_handler),
                user,
                None
            ),
            FishStatus::OK
        );
        loader.inner.blocks_built(&[BlockRun {
            block: "ruby:b".into(),
            first: 60,
            count: 1,
        }]);
        for kind in BlockEventKind::ALL {
            let want = if kind == BlockEventKind::Use {
                Outcome::Cancel
            } else {
                Outcome::Pass
            };
            assert_eq!(
                loader.inner.block_event(None, &event(kind, 60)),
                want,
                "{kind:?}"
            );
        }
        assert_eq!(all.calls.lock().unwrap().len(), 6);
    }

    #[test]
    fn a_handler_without_events_in_its_mask_is_never_called() {
        let (loader, h, _dir) = host();
        let t = token(&loader, "ruby");
        let probe = Probe::new(3);
        let user = (&raw const *probe).cast_mut().cast();
        let s = settings("minecraft:stone");
        assert_eq!(
            reg(h, t, "ruby:m", &s, 0, Some(probe_handler), user, None),
            FishStatus::OK
        );
        loader.inner.blocks_built(&[BlockRun {
            block: "ruby:m".into(),
            first: 5,
            count: 1,
        }]);
        assert_eq!(
            loader
                .inner
                .block_event(None, &event(BlockEventKind::Use, 5)),
            Outcome::Pass
        );
        assert!(probe.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn a_panicking_handler_stops_its_mod_and_the_mod_is_skipped_after() {
        {
            let handler: BlockHandlerFn = flagging_handler;
            let (loader, h, _dir) = host();
            let (t, other) = (token(&loader, "ruby"), token(&loader, "gem"));
            let s = settings("minecraft:stone");
            assert_eq!(
                reg(
                    h,
                    t,
                    "ruby:p",
                    &s,
                    ALL_EVENTS,
                    Some(handler),
                    std::ptr::null_mut(),
                    None
                ),
                FishStatus::OK
            );
            assert_eq!(
                reg(
                    h,
                    other,
                    "gem:q",
                    &s,
                    ALL_EVENTS,
                    Some(handler),
                    std::ptr::null_mut(),
                    None
                ),
                FishStatus::OK
            );
            loader.inner.blocks_built(&[
                BlockRun {
                    block: "gem:q".into(),
                    first: 9,
                    count: 1,
                },
                BlockRun {
                    block: "ruby:p".into(),
                    first: 10,
                    count: 1,
                },
            ]);
            assert_eq!(
                loader
                    .inner
                    .block_event(None, &event(BlockEventKind::Use, 10)),
                Outcome::Pass
            );
            let rec = loader.inner.record(t).unwrap();
            assert!(rec.is_failed());
            assert!(!loader.inner.record(other).unwrap().is_failed());
            assert!(
                loader
                    .inner
                    .notices
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|n| n.contains("block ruby:p panicked handling use")),
                "{:?}",
                loader.inner.notices.lock().unwrap()
            );
            // the failed mod's blocks are left out and its handler is skipped
            let mut defs = Vec::new();
            loader.inner.blocks_list(&mut defs);
            assert_eq!(
                defs.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
                ["gem:q"]
            );
            assert_eq!(
                loader
                    .inner
                    .block_event(None, &event(BlockEventKind::Use, 10)),
                Outcome::Pass
            );
            // the other mod's block still dispatches (and fails its own mod)
            loader
                .inner
                .block_event(None, &event(BlockEventKind::Broken, 9));
            assert!(loader.inner.record(other).unwrap().is_failed());
            assert!(loader.inner.blocks_content().is_empty());
        }
    }

    #[test]
    fn user_pointers_are_dropped_on_refusal_and_with_the_loader() {
        static DROPPED: AtomicU32 = AtomicU32::new(0);
        unsafe extern "C" fn count(_: *mut c_void) {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
        let (loader, h, _dir) = host();
        let t = token(&loader, "ruby");
        let ok = settings("minecraft:stone");
        let bad = settings("stone");
        let p = std::ptr::null_mut();
        assert_eq!(
            reg(h, t, "ruby:a", &ok, 0, None, p, Some(count)),
            FishStatus::OK
        );
        assert_eq!(DROPPED.load(Ordering::SeqCst), 0);
        assert_eq!(
            reg(h, t, "ruby:bad", &bad, 0, None, p, Some(count)),
            FishStatus::INVALID_ARGUMENT
        );
        assert_eq!(
            reg(h, t, "ruby:a", &ok, 0, None, p, Some(count)),
            FishStatus::ALREADY_EXISTS
        );
        assert_eq!(
            reg(h, 99, "ruby:z", &ok, 0, None, p, Some(count)),
            FishStatus::INVALID_ARGUMENT
        );
        assert_eq!(DROPPED.load(Ordering::SeqCst), 3);
        drop(loader);
        assert_eq!(DROPPED.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn blocks_built_reports_found_and_missing_blocks() {
        let (loader, h, _dir) = host();
        let t = token(&loader, "ruby");
        loader.inner.blocks_built(&[]);
        assert!(!warned(&loader, "has no states"));
        assert_eq!(plain(h, t, "ruby:a"), FishStatus::OK);
        assert_eq!(plain(h, t, "ruby:b"), FishStatus::OK);
        loader.inner.blocks_built(&[
            BlockRun {
                block: "ruby:a".into(),
                first: 7,
                count: 2,
            },
            BlockRun {
                block: "ruby:a".into(),
                first: 20,
                count: 1,
            },
        ]);
        assert!(warned(
            &loader,
            "block ruby:b of ruby has no states: Horizon did not add it"
        ));
        assert!(warned(
            &loader,
            "block ruby:a has states in more than one run"
        ));
        assert!(!warned(&loader, "block ruby:a of ruby has no states"));
        assert_eq!(range(h, "ruby:a"), (FishStatus::OK, 7, 2));
        assert_eq!(range(h, "ruby:b").0, FishStatus::NOT_FOUND);
    }

    #[test]
    fn state_range_checks_its_arguments() {
        let (_loader, h, _dir) = host();
        let (mut f, mut c) = (0, 0);
        // SAFETY: a live host; null outputs are checked for.
        let st = unsafe { state_range(h, FishStr::new("a:b"), std::ptr::null_mut(), &mut c) };
        assert_eq!(st, FishStatus::INVALID_ARGUMENT);
        // SAFETY: as above.
        let st = unsafe { state_range(h, FishStr::new("a:b"), &mut f, std::ptr::null_mut()) };
        assert_eq!(st, FishStatus::INVALID_ARGUMENT);
        let bad = [0xff_u8];
        let id = FishStr {
            ptr: bad.as_ptr(),
            len: 1,
        };
        // SAFETY: as above.
        let st = unsafe { state_range(h, id, &mut f, &mut c) };
        assert_eq!(st, FishStatus::INVALID_ARGUMENT);
    }

    #[test]
    fn content_is_written_for_mods_with_blocks_and_copies_their_assets() {
        let (loader, h, dir) = host();
        let (t, other) = (token(&loader, "ruby"), token(&loader, "gem"));
        assert!(loader.inner.blocks_content().is_empty());
        assert!(!dir.path().join(".fish").join("content").exists());
        assert_eq!(plain(h, t, "ruby:block"), FishStatus::OK);
        assert_eq!(plain(h, other, "gem:stone"), FishStatus::OK);

        // boot dropped the archives: hand over the ones on disk, as boot does for block mods
        let mut archives = HashMap::new();
        for id in ["ruby", "gem"] {
            let rec = loader.inner.record(token(&loader, id)).unwrap();
            let a =
                fish_pack::read(dir.path().join("fish-mods").join(format!("{id}.fish"))).unwrap();
            archives.insert(rec.origin.clone(), a);
        }
        loader.inner.keep_block_archives(&mut archives);
        assert!(archives.is_empty());

        let folders = loader.inner.blocks_content();
        let root = dir.path().join(".fish").join("content");
        assert_eq!(folders, [root.join("ruby"), root.join("gem")]);
        let toml = std::fs::read_to_string(root.join("ruby").join("horizon-mod.toml")).unwrap();
        assert!(
            toml.contains("id = \"ruby\"") && toml.contains("name = \"Ruby\""),
            "{toml}"
        );
        assert!(
            toml.contains("version = \"1.2.3\"") && toml.contains("description = \"About Ruby\""),
            "{toml}"
        );
        for file in [
            "data/ruby/horizon/block/block.json",
            "assets/ruby/lang/en_us.json",
            "data/c/tags/block/gems.json",
        ] {
            assert!(root.join("ruby").join(file).is_file(), "{file}");
        }
        assert!(!root.join("ruby/assets/foreign").exists());
        assert!(warned(&loader, "not in its own namespace"));
        assert!(root.join("gem/data/gem/horizon/block/stone.json").is_file());

        // a second call keeps the files (and their times) and fails no mod
        let stamp = std::fs::metadata(root.join("ruby/assets/ruby/lang/en_us.json"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(loader.inner.blocks_content(), folders);
        assert_eq!(
            std::fs::metadata(root.join("ruby/assets/ruby/lang/en_us.json"))
                .unwrap()
                .modified()
                .unwrap(),
            stamp
        );
    }

    #[test]
    fn content_without_a_kept_archive_warns_and_still_writes_the_blocks() {
        let (loader, h, dir) = host();
        let t = token(&loader, "gem");
        assert_eq!(plain(h, t, "gem:stone"), FishStatus::OK);
        let folders = loader.inner.blocks_content();
        assert_eq!(folders.len(), 1);
        assert!(warned(&loader, "the archive of Gem is not available"));
        assert!(
            dir.path()
                .join(".fish/content/gem/horizon-mod.toml")
                .is_file()
        );
    }

    #[test]
    fn content_that_cannot_be_written_is_a_warning_not_a_folder() {
        let (loader, h, dir) = host();
        let t = token(&loader, "gem");
        assert_eq!(plain(h, t, "gem:stone"), FishStatus::OK);
        std::fs::create_dir_all(dir.path().join(".fish")).unwrap();
        std::fs::write(dir.path().join(".fish").join("content"), b"a file").unwrap();
        assert!(loader.inner.blocks_content().is_empty());
        assert!(warned(&loader, "the blocks of Gem were not added"));
    }

    #[test]
    fn the_table_is_registered_and_sized() {
        assert_eq!(BLOCK.header.size, size_of::<BlockServiceV1>());
        assert_eq!(BLOCK.header.version, 1);
    }
}
