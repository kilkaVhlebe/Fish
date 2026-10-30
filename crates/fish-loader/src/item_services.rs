//! Host side of `fish:item` and `fish:item_group`, and the loader's item operations the hooks use.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering;

use fish_abi::items::{
    HANDLER_PANICKED, ItemGroupServiceV1, ItemHandlerFn, ItemPayloadV1, ItemServiceV1,
    ItemSettingsV1, event, rarity, set, slot,
};
use fish_abi::{ActionResult, DropUserFn, FishHandle, FishServiceHeader, FishStatus, FishStr};
use fish_hooks::{
    ClientView, ItemDef, ItemEvent, ItemEventKind, ItemGroupAddition, ItemGroupDef, Outcome,
};

use crate::block_services::NO_GAME;
use crate::blocks::{UserBox, check_id};
use crate::client_services::role_byte;
use crate::inner::Inner;
use crate::items::{ItemEntry, TITLE_MAX, check_item_def, patch_creative, valid_item_ref};

/// The most tooltip lines one handler call may add.
const TOOLTIP_LINES_MAX: usize = 16;
/// The most entries one tab or addition may list.
const ENTRIES_MAX: usize = 4096;

fn guard(f: impl FnOnce() -> FishStatus) -> FishStatus {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(FishStatus::PANICKED)
}

/// Reads what a mod registered into an item definition; `Err` is the reason.
///
/// # Safety
/// The strings of `s` must be valid for the call.
unsafe fn read_def(
    id: &str,
    mod_id: &str,
    s: &ItemSettingsV1,
    events: u32,
    has_handler: bool,
) -> Result<ItemDef, String> {
    if (s.size as usize) < size_of::<ItemSettingsV1>() {
        return Err("the settings table is too small".to_string());
    }
    if s.set & !set::ALL != 0 {
        return Err(format!("the settings set unknown bits ({:#x})", s.set));
    }
    if events & !event::ALL != 0 {
        return Err(format!("the events mask {events:#x} has unknown events"));
    }
    // SAFETY: valid string for the call (the caller's contract).
    let template = unsafe { s.template.as_str() }
        .map_err(|_| "the template is not UTF-8".to_string())?
        .to_string();
    let rarity = if s.set & set::RARITY != 0 {
        Some(
            rarity::NAMES
                .get(usize::from(s.rarity))
                .ok_or_else(|| format!("the rarity {} is unknown", s.rarity))?
                .to_string(),
        )
    } else {
        None
    };
    let def = ItemDef {
        id: id.to_string(),
        mod_id: mod_id.to_string(),
        template,
        max_stack: (s.set & set::MAX_STACK != 0).then_some(s.max_stack),
        rarity,
        // An item without a handler has no events to ask for.
        events: if has_handler { events } else { 0 },
    };
    check_item_def(&def)?;
    Ok(def)
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn register(
    host: *mut c_void,
    token: u64,
    id: FishStr,
    settings: *const ItemSettingsV1,
    events: u32,
    handler: Option<ItemHandlerFn>,
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
                "{who} registered the item {id}: the minecraft namespace belongs to the game"
            ));
            return FishStatus::INVALID_ARGUMENT;
        }
        if let Err(why) = check_id(id, rec.manifest.id.as_str()) {
            inner.warn(format!("{who} registered the invalid item {id}: {why}"));
            return FishStatus::INVALID_ARGUMENT;
        }
        if inner.net.frozen.load(Ordering::Acquire) {
            inner.warn(format!(
                "registries are frozen: the item {id} of {who} was not added"
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
                inner.warn(format!("{who} registered the invalid item {id}: {why}"));
                return FishStatus::INVALID_ARGUMENT;
            }
        };
        // A block's item has the block's id: Horizon refuses both under one id.
        if inner
            .blocks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .has_block(id)
        {
            inner.warn(format!(
                "{who} registered the item {id}, which is also a block (a block has an item of its own)"
            ));
            return FishStatus::ALREADY_EXISTS;
        }
        let added = inner
            .items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .register(ItemEntry {
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
                    "{who} registered the item {id}, which {first} already registered"
                ));
                FishStatus::ALREADY_EXISTS
            }
        }
    })
}

/// Reads `count` strings at `entries`; `Err` is the reason.
///
/// # Safety
/// `entries` must point at `count` valid strings unless it is null.
unsafe fn read_entries(entries: *const FishStr, count: usize) -> Result<Vec<String>, String> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if entries.is_null() {
        return Err("the entry list is null".to_string());
    }
    if count > ENTRIES_MAX {
        return Err(format!("{count} entries are more than {ENTRIES_MAX}"));
    }
    // SAFETY: non-null and `count` long (the caller's contract).
    let list = unsafe { std::slice::from_raw_parts(entries, count) };
    let mut out = Vec::with_capacity(count);
    for e in list {
        // SAFETY: valid string for the call.
        let text = unsafe { e.as_str() }.map_err(|_| "an entry is not UTF-8".to_string())?;
        if !valid_item_ref(text) {
            return Err(format!(
                "the entry {text:?} is not an item id (namespace:path)"
            ));
        }
        out.push(text.to_string());
    }
    Ok(out)
}

unsafe extern "C" fn register_group(
    host: *mut c_void,
    token: u64,
    id: FishStr,
    title: FishStr,
    icon: FishStr,
    entries: *const FishStr,
    entry_count: usize,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract: `host` is the loader's `Inner`.
        let inner = unsafe { &*host.cast::<Inner>() };
        // SAFETY: valid strings for the call.
        let (Ok(id), Ok(title), Ok(icon)) =
            (unsafe { (id.as_str(), title.as_str(), icon.as_str()) })
        else {
            return FishStatus::INVALID_ARGUMENT;
        };
        let Some(rec) = inner.record(token) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        let who = rec.manifest.name.clone();
        let refuse = |why: String| {
            inner.warn(format!(
                "{who} registered the invalid item group {id}: {why}"
            ));
            FishStatus::INVALID_ARGUMENT
        };
        if id.starts_with("minecraft:") {
            return refuse("the minecraft namespace belongs to the game".to_string());
        }
        if let Err(why) = check_id(id, rec.manifest.id.as_str()) {
            return refuse(why);
        }
        if inner.net.frozen.load(Ordering::Acquire) {
            inner.warn(format!(
                "registries are frozen: the item group {id} of {who} was not added"
            ));
            return FishStatus::INVALID_ARGUMENT;
        }
        if title.chars().count() > TITLE_MAX || title.chars().any(char::is_control) {
            return refuse(format!(
                "the title is longer than {TITLE_MAX} characters or has control characters"
            ));
        }
        if !valid_item_ref(icon) {
            return refuse(format!(
                "the icon {icon:?} is not an item id (namespace:path)"
            ));
        }
        // SAFETY: the list is valid for the call (ABI contract).
        let entries = match unsafe { read_entries(entries, entry_count) } {
            Ok(e) => e,
            Err(why) => return refuse(why),
        };
        let def = ItemGroupDef {
            id: id.to_string(),
            mod_id: rec.manifest.id.as_str().to_string(),
            title: title.to_string(),
            icon: icon.to_string(),
            entries,
        };
        let added = inner
            .groups
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .register(def, token);
        match added {
            Ok(()) => FishStatus::OK,
            Err(first) => {
                let first = inner
                    .record(first)
                    .map_or_else(|| "another mod".to_string(), |r| r.manifest.name.clone());
                inner.warn(format!(
                    "{who} registered the item group {id}, which {first} already registered"
                ));
                FishStatus::ALREADY_EXISTS
            }
        }
    })
}

unsafe extern "C" fn modify_entries(
    host: *mut c_void,
    token: u64,
    group: FishStr,
    entries: *const FishStr,
    entry_count: usize,
) -> FishStatus {
    guard(|| {
        // SAFETY: ABI contract: `host` is the loader's `Inner`.
        let inner = unsafe { &*host.cast::<Inner>() };
        // SAFETY: valid string for the call.
        let Ok(group) = (unsafe { group.as_str() }) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        let Some(rec) = inner.record(token) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        let who = rec.manifest.name.clone();
        let refuse = |why: String| {
            inner.warn(format!(
                "{who} modified the invalid item group {group}: {why}"
            ));
            FishStatus::INVALID_ARGUMENT
        };
        if !valid_item_ref(group) {
            return refuse("the group must be namespace:path".to_string());
        }
        if inner.net.frozen.load(Ordering::Acquire) {
            inner.warn(format!(
                "registries are frozen: the entries {who} added to {group} were not added"
            ));
            return FishStatus::INVALID_ARGUMENT;
        }
        // SAFETY: the list is valid for the call (ABI contract).
        let entries = match unsafe { read_entries(entries, entry_count) } {
            Ok(e) => e,
            Err(why) => return refuse(why),
        };
        inner.groups.lock().unwrap_or_else(|e| e.into_inner()).add(
            ItemGroupAddition {
                group: group.to_string(),
                mod_id: rec.manifest.id.as_str().to_string(),
                entries,
            },
            token,
        );
        FishStatus::OK
    })
}

/// Receives the lines a tooltip handler adds.
unsafe extern "C" fn collect_line(lines: *mut c_void, line: FishStr) {
    // SAFETY: `lines` is the `Vec<String>` `item_tooltip` passed in the payload.
    let lines = unsafe { &mut *lines.cast::<Vec<String>>() };
    // SAFETY: valid string for the call.
    if let Ok(text) = unsafe { line.as_str() }
        && lines.len() < TOOLTIP_LINES_MAX
    {
        lines.push(text.to_string());
    }
}

impl Inner {
    fn item_owner_failed(&self, token: u64) -> bool {
        self.record(token).is_some_and(|r| r.is_failed())
    }

    /// The items of the mods that did not fail, in registration order.
    pub(crate) fn items_list(&self, out: &mut Vec<ItemDef>) {
        let entries = self
            .items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries();
        out.extend(
            entries
                .iter()
                .filter(|e| !self.item_owner_failed(e.owner))
                .map(|e| e.def.clone()),
        );
    }

    /// The events any item handles.
    pub(crate) fn item_event_mask(&self) -> u32 {
        self.items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .event_mask()
    }

    /// True when `item` is an item a mod registered.
    pub(crate) fn item_known(&self, item: &str) -> bool {
        self.items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .has(item)
    }

    /// Calls `entry`'s handler for `kind`; returns the handler's byte, or `None` when the item does
    /// not want the event. A panic stops the mod and answers `None`.
    fn call_item(
        &self,
        game: Option<&mut dyn ClientView>,
        ev: &ItemEvent<'_>,
        add_line: Option<&mut Vec<String>>,
    ) -> Option<u8> {
        let entry = self
            .items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .find(ev.item)?;
        let handler = entry.handler?;
        let kind = ev.kind as u8;
        if entry.def.events & (1 << kind) == 0 || self.item_owner_failed(entry.owner) {
            return None;
        }
        let lines: *mut c_void =
            add_line.map_or(std::ptr::null_mut(), |l| std::ptr::from_mut(l).cast());
        let call = |view: FishHandle| -> u8 {
            let payload = ItemPayloadV1 {
                size: size_of::<ItemPayloadV1>() as u32,
                event: kind,
                role: role_byte(ev.role),
                hand: ev.hand,
                face: ev.face,
                slot: ev.slot,
                count: ev.count,
                pos: ev.pos,
                selected: u8::from(ev.selected),
                reserved: [0; 3],
                entity_id: ev.entity_id,
                actor_uuid: ev.actor_uuid,
                item: FishStr::new(&entry.def.id),
                actor: FishStr::new(ev.actor),
                dimension: FishStr::new(ev.dimension),
                target: FishStr::new(ev.target),
                add_line: (!lines.is_null()).then_some(collect_line as _),
                lines,
                view,
            };
            // SAFETY: the handler and its user pointer were registered together; the payload, its
            // strings and the line list live through the call.
            unsafe { handler(entry.user.ptr(), kind, &raw const payload) }
        };
        let result = match game {
            Some(g) => self.handles.client_scope(g, call),
            None => call(NO_GAME),
        };
        if result == HANDLER_PANICKED {
            self.fail_mod(
                entry.owner,
                format!("item {} panicked handling {}", entry.def.id, ev.kind.name()),
                true,
            );
            return None;
        }
        Some(result)
    }

    /// Calls the handler of the item `ev.item`.
    pub(crate) fn item_event(
        &self,
        game: Option<&mut dyn ClientView>,
        ev: &ItemEvent<'_>,
    ) -> Outcome {
        let Some(result) = self.call_item(game, ev, None) else {
            return Outcome::Pass;
        };
        if !matches!(
            ev.kind,
            ItemEventKind::Use | ItemEventKind::UseOnBlock | ItemEventKind::UseOnEntity
        ) {
            return Outcome::Pass;
        }
        match ActionResult::from_u8(result) {
            Some(ActionResult::Pass) | None => Outcome::Pass,
            Some(ActionResult::Fail) => Outcome::Cancel,
            Some(_) => Outcome::Handled,
        }
    }

    /// Appends the tooltip lines the mod of `item` adds.
    pub(crate) fn item_tooltip(&self, item: &str, count: u32, lines: &mut Vec<String>) {
        let mut ev = ItemEvent::new(ItemEventKind::AppendTooltip, item, count);
        ev.slot = slot::NONE;
        let mut added = Vec::new();
        self.call_item(None, &ev, Some(&mut added));
        lines.extend(added);
    }

    /// The creative inventory text with the Fish tabs and entries applied; `None` when nothing
    /// changes (no tab and no addition, or a document that cannot be read).
    pub(crate) fn creative_inventory(&self, json: &str) -> Option<String> {
        let (groups, additions) = self
            .groups
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .live(|owner| !self.item_owner_failed(owner));
        if groups.is_empty() && additions.is_empty() {
            return None;
        }
        let known = |id: &str| {
            let (namespace, _) = id.split_once(':')?;
            if !self
                .records()
                .iter()
                .any(|r| r.manifest.id.as_str() == namespace)
            {
                return None;
            }
            let is_item = self.items.lock().unwrap_or_else(|e| e.into_inner()).has(id);
            let is_block = self
                .blocks
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .has_block(id);
            Some(is_item || is_block)
        };
        match patch_creative(json, &groups, &additions, known) {
            Ok((text, report)) => {
                self.emit(
                    log::Level::Info,
                    "fish_loader",
                    format!(
                        "Fish creative inventory: {} tab(s), {} addition(s)",
                        report.shown.len(),
                        report.applied
                    ),
                );
                if !report.hidden.is_empty() {
                    self.warn(format!(
                        "the creative screen has no free place for the tab(s) {}: their items are only in the search tab",
                        report.hidden.join(", ")
                    ));
                }
                for (group, who) in &report.unknown {
                    self.warn(format!(
                        "{who} added items to the creative tab {group}, which does not exist"
                    ));
                }
                for (group, item) in &report.skipped {
                    self.warn(format!(
                        "the creative tab {group} lists {item}, which no mod registered: left out"
                    ));
                }
                Some(text)
            }
            Err(why) => {
                self.warn(format!(
                    "the creative inventory could not be extended: {why}"
                ));
                None
            }
        }
    }
}

/// The `fish:item` table.
pub(crate) static ITEM: ItemServiceV1 = ItemServiceV1 {
    header: FishServiceHeader {
        size: size_of::<ItemServiceV1>(),
        version: 1,
    },
    register,
};

/// The `fish:item_group` table.
pub(crate) static ITEM_GROUP: ItemGroupServiceV1 = ItemGroupServiceV1 {
    header: FishServiceHeader {
        size: size_of::<ItemGroupServiceV1>(),
        version: 1,
    },
    register: register_group,
    modify_entries,
};

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Mutex};

    use fish_hooks::{PlayerSnapshot, Role};
    use fish_metadata::Side;

    use super::*;
    use crate::boot::{BootConfig, Loader, boot};

    /// A booted loader with the mods `ruby` and `gem` (with assets), and its host pointer.
    fn host() -> (Loader, *mut c_void, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("fish-mods")).unwrap();
        for (id, name) in [("ruby", "Ruby"), ("gem", "Gem")] {
            let mut spec = fish_pack::PackSpec::new(format!(
                "schema = 1\nid = \"{id}\"\nversion = \"1.2.3\"\nname = \"{name}\"\ndescription = \"About {name}\"\n"
            ));
            spec.add_bytes(&format!("assets/{id}/models/item/x.json"), b"{}".to_vec())
                .unwrap();
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

    fn warned(loader: &Loader, text: &str) -> bool {
        loader
            .inner
            .warnings
            .lock()
            .unwrap()
            .iter()
            .any(|w| w.contains(text))
    }

    fn settings(template: &'static str) -> ItemSettingsV1 {
        ItemSettingsV1 {
            size: size_of::<ItemSettingsV1>() as u32,
            set: 0,
            max_stack: 0,
            rarity: 0,
            reserved: [0; 2],
            template: FishStr::from_static(template),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn reg(
        host: *mut c_void,
        token: u64,
        id: &str,
        s: &ItemSettingsV1,
        events: u32,
        handler: Option<ItemHandlerFn>,
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
            &settings("minecraft:stick"),
            0,
            None,
            std::ptr::null_mut(),
            None,
        )
    }

    fn strs(list: &[&'static str]) -> Vec<FishStr> {
        list.iter().map(|s| FishStr::from_static(s)).collect()
    }

    fn group(
        host: *mut c_void,
        token: u64,
        id: &str,
        title: &str,
        icon: &str,
        entries: &[FishStr],
    ) -> FishStatus {
        // SAFETY: a live host; the strings and the list outlive the call.
        unsafe {
            register_group(
                host,
                token,
                FishStr::new(id),
                FishStr::new(title),
                FishStr::new(icon),
                entries.as_ptr(),
                entries.len(),
            )
        }
    }

    fn add(host: *mut c_void, token: u64, group: &str, entries: &[FishStr]) -> FishStatus {
        // SAFETY: a live host; the strings and the list outlive the call.
        unsafe {
            modify_entries(
                host,
                token,
                FishStr::new(group),
                entries.as_ptr(),
                entries.len(),
            )
        }
    }

    #[test]
    fn bad_registrations_are_refused_with_a_warning_naming_the_item() {
        let (loader, h, _dir) = host();
        let t = token(&loader, "ruby");
        let ok = settings("minecraft:stick");
        let check = |id: &str, s: &ItemSettingsV1, why: &str| {
            let st = reg(h, t, id, s, 0, None, std::ptr::null_mut(), None);
            assert_eq!(st, FishStatus::INVALID_ARGUMENT, "{id}");
            assert!(
                warned(&loader, why),
                "{why} not in {:?}",
                loader.inner.warnings.lock().unwrap()
            );
        };
        check("minecraft:stick2", &ok, "belongs to the game");
        check("nocolon", &ok, "namespace:path");
        check("gem:x", &ok, "is not the id of the mod");
        check("ruby:Bad", &ok, "the path must be");
        let mut s = ok;
        s.size = 4;
        check("ruby:small", &s, "too small");
        let mut s = ok;
        s.set = 1 << 20;
        check("ruby:bits", &s, "unknown bits");
        check("ruby:tmpl", &settings("stick"), "must be minecraft:<path>");
        for (id, stack) in [("ruby:zero", 0), ("ruby:big", 100)] {
            let mut s = ok;
            s.set = set::MAX_STACK;
            s.max_stack = stack;
            check(id, &s, "the stack size");
        }
        let mut s = ok;
        s.set = set::RARITY;
        s.rarity = 9;
        check("ruby:rare", &s, "the rarity 9 is unknown");
        let bad_utf8 = [0xff_u8];
        let mut s = ok;
        s.template = FishStr {
            ptr: bad_utf8.as_ptr(),
            len: 1,
        };
        check("ruby:utf8", &s, "not UTF-8");
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
        // a null table, a bad id and an unknown mod
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
        assert!(loader.inner.items.lock().unwrap().entries().is_empty());
    }

    #[test]
    fn registration_keeps_the_first_item_and_stops_at_the_freeze() {
        let (loader, h, _dir) = host();
        let t = token(&loader, "ruby");
        assert_eq!(plain(h, t, "ruby:one"), FishStatus::OK);
        assert_eq!(plain(h, t, "ruby:one"), FishStatus::ALREADY_EXISTS);
        assert!(warned(&loader, "ruby:one, which Ruby already registered"));
        loader.inner.net.frozen.store(true, Ordering::Release);
        assert_eq!(plain(h, t, "ruby:late"), FishStatus::INVALID_ARGUMENT);
        assert!(warned(&loader, "registries are frozen") && warned(&loader, "ruby:late"));
        assert_eq!(loader.inner.items.lock().unwrap().entries().len(), 1);
        assert!(loader.inner.item_known("ruby:one") && !loader.inner.item_known("ruby:late"));
    }

    #[test]
    fn an_item_and_a_block_never_share_an_id() {
        let (loader, h, _dir) = host();
        let t = token(&loader, "ruby");
        let s = fish_abi::blocks::BlockSettingsV1 {
            size: size_of::<fish_abi::blocks::BlockSettingsV1>() as u32,
            set: 0,
            hardness: 0.0,
            resistance: 0.0,
            friction: 0.0,
            light: 0,
            requires_tool: 0,
            map_color: 0,
            reserved: 0,
            template: FishStr::from_static("minecraft:stone"),
            sounds: FishStr::EMPTY,
        };
        let block = |id: &str| {
            // SAFETY: a live host; the strings and the table outlive the call.
            unsafe {
                (crate::block_services::BLOCK.register)(
                    h,
                    t,
                    FishStr::new(id),
                    &raw const s,
                    0,
                    None,
                    std::ptr::null_mut(),
                    None,
                )
            }
        };
        assert_eq!(block("ruby:both"), FishStatus::OK);
        assert_eq!(plain(h, t, "ruby:both"), FishStatus::ALREADY_EXISTS);
        assert!(warned(&loader, "which is also a block"));
        assert_eq!(plain(h, t, "ruby:only_item"), FishStatus::OK);
        assert_eq!(block("ruby:only_item"), FishStatus::ALREADY_EXISTS);
        assert!(warned(&loader, "which is also an item"));
    }

    #[test]
    fn settings_become_the_definition_and_unset_ones_stay_none() {
        let (loader, h, _dir) = host();
        let t = token(&loader, "ruby");
        let mut s = settings("minecraft:diamond");
        s.set = set::ALL;
        s.max_stack = 16;
        s.rarity = rarity::EPIC;
        assert_eq!(
            reg(
                h,
                t,
                "ruby:full",
                &s,
                0b11,
                None,
                std::ptr::null_mut(),
                None
            ),
            FishStatus::OK
        );
        assert_eq!(plain(h, t, "ruby:bare"), FishStatus::OK);
        let mut defs = Vec::new();
        loader.inner.items_list(&mut defs);
        assert_eq!(defs.len(), 2);
        assert_eq!(
            defs[0],
            ItemDef {
                id: "ruby:full".into(),
                mod_id: "ruby".into(),
                template: "minecraft:diamond".into(),
                max_stack: Some(16),
                rarity: Some("epic".into()),
                // no handler: the events asked for do not count
                events: 0,
            }
        );
        assert_eq!((defs[1].max_stack, defs[1].rarity.clone()), (None, None));
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

    use std::sync::atomic::{AtomicBool, AtomicU8};

    unsafe extern "C" fn probe_handler(
        user: *mut c_void,
        event: u8,
        payload: *const ItemPayloadV1,
    ) -> u8 {
        // SAFETY: `user` is the `Probe` the tests pass; the payload is valid for the call.
        let (probe, p) = unsafe { (&*user.cast::<Probe>(), &*payload) };
        // SAFETY: the strings are valid for the call.
        let (item, actor, dim, target) = unsafe {
            (
                p.item.as_str().unwrap(),
                p.actor.as_str().unwrap(),
                p.dimension.as_str().unwrap(),
                p.target.as_str().unwrap(),
            )
        };
        if p.view.index != u32::MAX {
            probe.with_view.store(true, Ordering::SeqCst);
        }
        if let Some(add_line) = p.add_line {
            // SAFETY: the sink and its argument come from the host together.
            unsafe {
                add_line(p.lines, FishStr::from_static("§7one"));
                add_line(p.lines, FishStr::from_static("two"));
            }
        }
        probe.calls.lock().unwrap().push(format!(
            "{event} {item} count={} hand={} face={} slot={} sel={} pos={:?} entity={} target={target} role={} actor={actor}/{} dim={dim} lines={} size={}",
            p.count,
            p.hand,
            p.face,
            p.slot,
            p.selected,
            p.pos,
            p.entity_id,
            p.role,
            p.actor_uuid[0],
            p.add_line.is_some(),
            p.size
        ));
        probe.answer.load(Ordering::SeqCst)
    }

    unsafe extern "C" fn panicking_handler(_: *mut c_void, _: u8, _: *const ItemPayloadV1) -> u8 {
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
        fn game_dir(&self) -> std::path::PathBuf {
            std::path::PathBuf::new()
        }
    }

    fn event(kind: ItemEventKind, item: &'static str) -> ItemEvent<'static> {
        let mut e = ItemEvent::new(kind, item, 3);
        e.hand = 1;
        e.face = 4;
        e.slot = 12;
        e.selected = true;
        e.pos = [1, 2, 3];
        e.target = "minecraft:stone";
        e.entity_id = 77;
        e.actor = "Steve";
        e.actor_uuid = [9; 16];
        e.dimension = "minecraft:overworld";
        e
    }

    #[test]
    fn events_reach_the_handler_with_the_payload_and_only_if_asked_for() {
        let (loader, h, _dir) = host();
        let t = token(&loader, "ruby");
        let probe = Probe::new(ActionResult::Pass.as_u8());
        let user = (&raw const *probe).cast_mut().cast();
        let s = settings("minecraft:stick");
        let asked = 1 << event::USE | 1 << event::INVENTORY_TICK | 1 << event::APPEND_TOOLTIP;
        assert_eq!(
            reg(h, t, "ruby:gem", &s, asked, Some(probe_handler), user, None),
            FishStatus::OK
        );
        assert_eq!(plain(h, t, "ruby:plain"), FishStatus::OK);
        assert_eq!(loader.inner.item_event_mask(), asked);

        let out = loader
            .inner
            .item_event(None, &event(ItemEventKind::InventoryTick, "ruby:gem"));
        assert_eq!(out, Outcome::Pass);
        assert_eq!(
            probe.calls.lock().unwrap().last().unwrap(),
            &format!(
                "4 ruby:gem count=3 hand=1 face=4 slot=12 sel=1 pos=[1, 2, 3] entity=77 target=minecraft:stone role={} actor=Steve/9 dim=minecraft:overworld lines=false size={}",
                role_byte(fish_hooks::Role::Window),
                size_of::<ItemPayloadV1>()
            )
        );
        assert!(!probe.with_view.load(Ordering::SeqCst));

        // with a game the handle is live
        let mut view = View;
        loader
            .inner
            .item_event(Some(&mut view), &event(ItemEventKind::Use, "ruby:gem"));
        assert!(probe.with_view.load(Ordering::SeqCst));

        // events the item did not ask for, items without a handler and unknown items get nothing
        for (kind, item) in [
            (ItemEventKind::UseOnBlock, "ruby:gem"),
            (ItemEventKind::FinishUsing, "ruby:gem"),
            (ItemEventKind::Use, "ruby:plain"),
            (ItemEventKind::Use, "ruby:nobody"),
            (ItemEventKind::Use, "minecraft:stick"),
        ] {
            assert_eq!(
                loader.inner.item_event(None, &event(kind, item)),
                Outcome::Pass
            );
        }
        assert_eq!(probe.calls.lock().unwrap().len(), 2);

        // the tooltip collects the lines the handler adds
        let mut lines = vec!["vanilla".to_string()];
        loader.inner.item_tooltip("ruby:gem", 5, &mut lines);
        assert_eq!(lines, ["vanilla", "§7one", "two"]);
        assert!(
            probe
                .calls
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .starts_with("5 ruby:gem count=5 hand=0 face=255 slot=-1 sel=0")
        );
        let mut none = Vec::new();
        loader.inner.item_tooltip("ruby:plain", 1, &mut none);
        loader.inner.item_tooltip("minecraft:stick", 1, &mut none);
        assert!(none.is_empty());
    }

    #[test]
    fn only_the_use_events_decide_and_results_map_to_outcomes() {
        let (loader, h, _dir) = host();
        let t = token(&loader, "ruby");
        let probe = Probe::new(0);
        let user = (&raw const *probe).cast_mut().cast();
        let s = settings("minecraft:stick");
        assert_eq!(
            reg(
                h,
                t,
                "ruby:a",
                &s,
                event::ALL,
                Some(probe_handler),
                user,
                None
            ),
            FishStatus::OK
        );
        let outcome = |kind| loader.inner.item_event(None, &event(kind, "ruby:a"));
        for (answer, expected) in [
            (ActionResult::Pass.as_u8(), Outcome::Pass),
            (ActionResult::Fail.as_u8(), Outcome::Cancel),
            (ActionResult::Success.as_u8(), Outcome::Handled),
            (ActionResult::Consume.as_u8(), Outcome::Handled),
            (200, Outcome::Pass),
        ] {
            probe.answer.store(answer, Ordering::SeqCst);
            for kind in [
                ItemEventKind::Use,
                ItemEventKind::UseOnBlock,
                ItemEventKind::UseOnEntity,
            ] {
                assert_eq!(outcome(kind), expected, "{kind:?} answer {answer}");
            }
            for kind in [ItemEventKind::FinishUsing, ItemEventKind::InventoryTick] {
                assert_eq!(outcome(kind), Outcome::Pass, "{kind:?} answer {answer}");
            }
        }
    }

    #[test]
    fn a_panicking_handler_stops_its_mod_and_the_mod_is_skipped_after() {
        let (loader, h, _dir) = host();
        let (t, other) = (token(&loader, "ruby"), token(&loader, "gem"));
        let s = settings("minecraft:stick");
        let bad = 1 << event::USE;
        assert_eq!(
            reg(
                h,
                t,
                "ruby:bad",
                &s,
                bad,
                Some(panicking_handler),
                std::ptr::null_mut(),
                None
            ),
            FishStatus::OK
        );
        assert_eq!(
            reg(
                h,
                other,
                "gem:bad",
                &s,
                bad,
                Some(panicking_handler),
                std::ptr::null_mut(),
                None
            ),
            FishStatus::OK
        );
        assert_eq!(
            loader
                .inner
                .item_event(None, &event(ItemEventKind::Use, "ruby:bad")),
            Outcome::Pass
        );
        assert!(loader.inner.record(t).unwrap().is_failed());
        assert!(!loader.inner.record(other).unwrap().is_failed());
        let mut defs = Vec::new();
        loader.inner.items_list(&mut defs);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].id, "gem:bad");
        // the stopped mod's items no longer answer
        assert_eq!(
            loader
                .inner
                .item_event(None, &event(ItemEventKind::Use, "ruby:bad")),
            Outcome::Pass
        );
    }

    #[test]
    fn user_pointers_are_dropped_on_refusal_and_with_the_loader() {
        static DROPS: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "C" fn count(_: *mut c_void) {
            DROPS.fetch_add(1, Ordering::SeqCst);
        }
        let (loader, h, _dir) = host();
        let t = token(&loader, "ruby");
        let s = settings("minecraft:stick");
        let ask = |id: &str, s: &ItemSettingsV1| {
            reg(
                h,
                t,
                id,
                s,
                0,
                Some(panicking_handler),
                std::ptr::null_mut(),
                Some(count),
            )
        };
        assert_eq!(ask("ruby:kept", &s), FishStatus::OK);
        assert_eq!(DROPS.load(Ordering::SeqCst), 0);
        assert_eq!(ask("ruby:kept", &s), FishStatus::ALREADY_EXISTS);
        assert_eq!(ask("minecraft:no", &s), FishStatus::INVALID_ARGUMENT);
        assert_eq!(DROPS.load(Ordering::SeqCst), 2);
        drop(loader);
        assert_eq!(DROPS.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn groups_are_checked_registered_once_and_extended() {
        let (loader, h, _dir) = host();
        let t = token(&loader, "ruby");
        let entries = strs(&["ruby:a", "minecraft:stone"]);
        assert_eq!(
            group(h, t, "ruby:tab", "Ruby", "ruby:a", &entries),
            FishStatus::OK
        );
        assert_eq!(
            group(h, t, "ruby:tab", "Ruby", "ruby:a", &entries),
            FishStatus::ALREADY_EXISTS
        );
        assert!(warned(&loader, "ruby:tab, which Ruby already registered"));
        for (id, title, icon, why) in [
            ("minecraft:tab", "T", "ruby:a", "belongs to the game"),
            ("gem:tab", "T", "ruby:a", "is not the id of the mod"),
            ("ruby:long", &"x".repeat(65), "ruby:a", "the title"),
            ("ruby:ctl", "a\nb", "ruby:a", "the title"),
            ("ruby:icon", "T", "not an id", "the icon"),
        ] {
            assert_eq!(
                group(h, t, id, title, icon, &entries),
                FishStatus::INVALID_ARGUMENT,
                "{id}"
            );
            assert!(warned(&loader, why), "{why}");
        }
        assert_eq!(
            group(h, t, "ruby:entry", "T", "ruby:a", &strs(&["bad entry"])),
            FishStatus::INVALID_ARGUMENT
        );
        assert!(warned(&loader, "is not an item id"));
        // a null list with a count
        // SAFETY: a live host; the null list is checked for.
        let st = unsafe {
            register_group(
                h,
                t,
                FishStr::new("ruby:null"),
                FishStr::EMPTY,
                FishStr::new("ruby:a"),
                std::ptr::null(),
                2,
            )
        };
        assert_eq!(st, FishStatus::INVALID_ARGUMENT);
        assert_eq!(
            group(h, 99, "ruby:ghost", "", "ruby:a", &[]),
            FishStatus::INVALID_ARGUMENT
        );

        assert_eq!(
            add(h, t, "minecraft:ingredients", &strs(&["ruby:a"])),
            FishStatus::OK
        );
        assert_eq!(add(h, t, "minecraft:ingredients", &[]), FishStatus::OK);
        assert_eq!(
            add(h, t, "nogroup", &strs(&["ruby:a"])),
            FishStatus::INVALID_ARGUMENT
        );
        assert_eq!(
            add(h, t, "minecraft:combat", &strs(&["x"])),
            FishStatus::INVALID_ARGUMENT
        );
        assert_eq!(
            add(h, 99, "minecraft:combat", &[]),
            FishStatus::INVALID_ARGUMENT
        );
        loader.inner.net.frozen.store(true, Ordering::Release);
        assert_eq!(
            group(h, t, "ruby:late", "", "ruby:a", &[]),
            FishStatus::INVALID_ARGUMENT
        );
        assert_eq!(
            add(h, t, "minecraft:combat", &[]),
            FishStatus::INVALID_ARGUMENT
        );
        assert!(warned(
            &loader,
            "the item group ruby:late of Ruby was not added"
        ));
        let (groups, additions) = loader.inner.groups.lock().unwrap().live(|_| true);
        assert_eq!((groups.len(), additions.len()), (1, 2));
    }

    fn creative() -> String {
        serde_json::json!({"tabs": [
            {"id": "minecraft:ingredients", "type": "CATEGORY", "row": "BOTTOM", "column": 3,
             "title": "Ingredients", "icon": "{count:1,id:\"minecraft:iron_ingot\"}",
             "visible": true, "items": ["{count:1,id:\"minecraft:iron_ingot\"}"]},
            {"id": "minecraft:search", "type": "SEARCH", "row": "TOP", "column": 6,
             "title": "Search", "icon": "{count:1,id:\"minecraft:compass\"}",
             "visible": true, "items": []},
            {"id": "minecraft:inventory", "type": "INVENTORY", "row": "BOTTOM", "column": 6,
             "title": "Inventory", "icon": "{count:1,id:\"minecraft:chest\"}",
             "visible": true, "items": []},
        ]})
        .to_string()
    }

    #[test]
    fn the_creative_inventory_is_patched_for_live_mods_and_known_items_only() {
        let (loader, h, _dir) = host();
        let (t, other) = (token(&loader, "ruby"), token(&loader, "gem"));
        assert_eq!(loader.inner.creative_inventory(&creative()), None);
        assert_eq!(plain(h, t, "ruby:a"), FishStatus::OK);
        let entries = strs(&["ruby:a", "ruby:typo", "minecraft:stone", "unrelated:thing"]);
        assert_eq!(
            group(h, t, "ruby:tab", "", "ruby:a", &entries),
            FishStatus::OK
        );
        assert_eq!(
            group(h, other, "gem:tab", "Gem", "ruby:a", &[]),
            FishStatus::OK
        );
        assert_eq!(
            add(h, t, "minecraft:ingredients", &strs(&["ruby:a"])),
            FishStatus::OK
        );
        assert_eq!(
            add(h, t, "minecraft:nowhere", &strs(&["ruby:a"])),
            FishStatus::OK
        );

        let text = loader.inner.creative_inventory(&creative()).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        let tabs = doc["tabs"].as_array().unwrap();
        let find = |id: &str| tabs.iter().find(|t| t["id"] == id).unwrap();
        assert_eq!(find("ruby:tab")["title"], "Tab");
        assert_eq!(find("gem:tab")["title"], "Gem");
        let ids = |tab: &serde_json::Value| -> Vec<String> {
            tab["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i.as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(
            ids(find("ruby:tab")),
            [
                "{count:1,id:\"ruby:a\"}",
                "{count:1,id:\"minecraft:stone\"}",
                "{count:1,id:\"unrelated:thing\"}"
            ]
        );
        assert!(
            ids(find("minecraft:ingredients")).contains(&"{count:1,id:\"ruby:a\"}".to_string())
        );
        assert!(warned(&loader, "the creative tab ruby:tab lists ruby:typo"));
        assert!(warned(
            &loader,
            "creative tab minecraft:nowhere, which does not exist"
        ));

        // a stopped mod's tabs and entries are gone
        loader.inner.fail_mod(other, "test".to_string(), false);
        let doc: serde_json::Value =
            serde_json::from_str(&loader.inner.creative_inventory(&creative()).unwrap()).unwrap();
        assert!(
            doc["tabs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|t| t["id"] != "gem:tab")
        );

        // a document that is not a creative inventory is left alone
        assert_eq!(loader.inner.creative_inventory("[]"), None);
        assert!(warned(&loader, "could not be extended"));
    }

    #[test]
    fn content_of_items_is_written_with_the_mods_assets() {
        let (loader, h, dir) = host();
        let t = token(&loader, "ruby");
        let mut s = settings("minecraft:diamond");
        s.set = set::ALL;
        s.max_stack = 8;
        s.rarity = rarity::RARE;
        assert_eq!(
            reg(h, t, "ruby:gem", &s, 0, None, std::ptr::null_mut(), None),
            FishStatus::OK
        );
        let mut archives = std::collections::HashMap::new();
        let rec = loader.inner.record(t).unwrap();
        let a = fish_pack::read(dir.path().join("fish-mods").join("ruby.fish")).unwrap();
        archives.insert(rec.origin.clone(), a);
        loader.inner.keep_block_archives(&mut archives);
        let folders = loader.inner.blocks_content();
        let root = dir.path().join(".fish").join("content");
        assert_eq!(folders, [root.join("ruby")]);
        let json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("ruby/data/ruby/horizon/item/gem.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"template": "minecraft:diamond", "max_stack_size": 8, "rarity": "rare"})
        );
    }

    #[test]
    fn the_tables_are_registered_and_sized() {
        assert_eq!(ITEM.header.size, size_of::<ItemServiceV1>());
        assert_eq!(ITEM.header.version, 1);
        assert_eq!(ITEM_GROUP.header.size, size_of::<ItemGroupServiceV1>());
        assert_eq!(ITEM_GROUP.header.version, 1);
    }
}
