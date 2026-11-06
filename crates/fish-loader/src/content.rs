//! The content table entries, sound events and particle types mods registered.

use std::path::Path;

use fish_hooks::{
    CompostDef, ContentTables, FlammableDef, FuelDef, ParticleDef, PathTypeDef, SculkFrequencyDef,
    SoundDef, TransformDef,
};
use serde_json::Value;

/// Rows of one table, each with the token of the mod that registered it.
pub(crate) struct Table<T> {
    rows: Vec<(T, u64)>,
}

impl<T> Default for Table<T> {
    fn default() -> Self {
        Table { rows: Vec::new() }
    }
}

impl<T: Clone> Table<T> {
    /// Adds `row` unless `same` finds a row for the same key; `Err` holds the token of the mod that
    /// registered that row.
    fn insert(&mut self, row: T, owner: u64, same: impl Fn(&T) -> bool) -> Result<(), u64> {
        if let Some((_, first)) = self.rows.iter().find(|(r, _)| same(r)) {
            return Err(*first);
        }
        self.rows.push((row, owner));
        Ok(())
    }

    /// The rows of the mods `live` accepts, in registration order.
    fn live(&self, live: &impl Fn(u64) -> bool) -> Vec<T> {
        self.rows
            .iter()
            .filter(|(_, o)| live(*o))
            .map(|(r, _)| r.clone())
            .collect()
    }

    fn owned_by(&self, token: u64) -> bool {
        self.rows.iter().any(|(_, o)| *o == token)
    }

    fn clear(&mut self) {
        self.rows.clear();
    }
}

/// Every content table entry, sound event and particle type a loader knows.
#[derive(Default)]
pub(crate) struct ContentStore {
    fuels: Table<FuelDef>,
    compostables: Table<CompostDef>,
    flammables: Table<FlammableDef>,
    transforms: Table<TransformDef>,
    path_types: Table<PathTypeDef>,
    sculk: Table<SculkFrequencyDef>,
    sounds: Table<SoundDef>,
    particles: Table<ParticleDef>,
}

impl ContentStore {
    /// Adds a fuel; `Err` holds the token of the mod that registered the item first.
    pub(crate) fn add_fuel(&mut self, def: FuelDef, owner: u64) -> Result<(), u64> {
        let item = def.item.clone();
        self.fuels.insert(def, owner, |r| r.item == item)
    }

    /// Adds a compostable item.
    pub(crate) fn add_compostable(&mut self, def: CompostDef, owner: u64) -> Result<(), u64> {
        let item = def.item.clone();
        self.compostables.insert(def, owner, |r| r.item == item)
    }

    /// Adds the fire odds of a block.
    pub(crate) fn add_flammable(&mut self, def: FlammableDef, owner: u64) -> Result<(), u64> {
        let block = def.block.clone();
        self.flammables.insert(def, owner, |r| r.block == block)
    }

    /// Adds a tool action; the key is the kind and the block it starts from.
    pub(crate) fn add_transform(&mut self, def: TransformDef, owner: u64) -> Result<(), u64> {
        let (kind, from) = (def.kind, def.from.clone());
        self.transforms
            .insert(def, owner, |r| r.kind == kind && r.from == from)
    }

    /// Adds the path type of a block.
    pub(crate) fn add_path_type(&mut self, def: PathTypeDef, owner: u64) -> Result<(), u64> {
        let block = def.block.clone();
        self.path_types.insert(def, owner, |r| r.block == block)
    }

    /// Adds the vibration frequency of a game event.
    pub(crate) fn add_sculk(&mut self, def: SculkFrequencyDef, owner: u64) -> Result<(), u64> {
        let event = def.event.clone();
        self.sculk.insert(def, owner, |r| r.event == event)
    }

    /// Adds a sound event.
    pub(crate) fn add_sound(&mut self, def: SoundDef, owner: u64) -> Result<(), u64> {
        let id = def.id.clone();
        self.sounds.insert(def, owner, |r| r.id == id)
    }

    /// Adds a particle type.
    pub(crate) fn add_particle(&mut self, def: ParticleDef, owner: u64) -> Result<(), u64> {
        let id = def.id.clone();
        self.particles.insert(def, owner, |r| r.id == id)
    }

    /// The table entries of the mods `live` accepts.
    pub(crate) fn tables(&self, live: impl Fn(u64) -> bool) -> ContentTables {
        ContentTables {
            fuels: self.fuels.live(&live),
            compostables: self.compostables.live(&live),
            flammables: self.flammables.live(&live),
            transforms: self.transforms.live(&live),
            path_types: self.path_types.live(&live),
            sculk_frequencies: self.sculk.live(&live),
        }
    }

    /// The frequency of game event `event`, if a mod `live` accepts gave it one.
    pub(crate) fn sculk_frequency(&self, event: &str, live: impl Fn(u64) -> bool) -> Option<u8> {
        self.sculk
            .rows
            .iter()
            .find(|(r, o)| r.event == event && live(*o))
            .map(|(r, _)| r.frequency)
    }

    /// The sound events of the mods `live` accepts.
    pub(crate) fn sounds(&self, live: impl Fn(u64) -> bool) -> Vec<SoundDef> {
        self.sounds.live(&live)
    }

    /// The particle types of the mods `live` accepts.
    pub(crate) fn particles(&self, live: impl Fn(u64) -> bool) -> Vec<ParticleDef> {
        self.particles.live(&live)
    }

    /// True when `token` registered a sound event or a particle type: its assets are content.
    pub(crate) fn has_assets_of(&self, token: u64) -> bool {
        self.sounds.owned_by(token) || self.particles.owned_by(token)
    }

    /// Drops everything.
    pub(crate) fn clear(&mut self) {
        self.fuels.clear();
        self.compostables.clear();
        self.flammables.clear();
        self.transforms.clear();
        self.path_types.clear();
        self.sculk.clear();
        self.sounds.clear();
        self.particles.clear();
    }
}

/// Checks what the content folder of a mod holds for the sound events and particle types it
/// registered: `assets/<id>/sounds.json` must name each event and the `.ogg` files it lists must
/// exist, `assets/<id>/particles/<path>.json` must exist for each particle type. Returns one
/// warning per problem; the content is written anyway (Horizon drops what is wrong).
pub(crate) fn check_assets(
    folder: &Path,
    mod_id: &str,
    sounds: &[&SoundDef],
    particles: &[&ParticleDef],
) -> Vec<String> {
    let mut warnings = Vec::new();
    let own = |def_id: &str| {
        def_id
            .split_once(':')
            .map_or(def_id.to_string(), |(_, p)| p.to_string())
    };
    if !sounds.is_empty() {
        let file = folder.join(format!("assets/{mod_id}/sounds.json"));
        match std::fs::read(&file).map(|b| serde_json::from_slice::<Value>(&b)) {
            Err(_) => warnings.push(format!(
                "{mod_id} registered {} sound event(s) but has no assets/{mod_id}/sounds.json",
                sounds.len()
            )),
            Ok(Err(e)) => warnings.push(format!(
                "assets/{mod_id}/sounds.json of {mod_id} is not JSON: {e}"
            )),
            Ok(Ok(doc)) => {
                for def in sounds {
                    let key = own(&def.id);
                    let Some(event) = doc.get(&key) else {
                        warnings.push(format!(
                            "the sound event {} is not in assets/{mod_id}/sounds.json",
                            def.id
                        ));
                        continue;
                    };
                    for name in sound_files(event) {
                        let (ns, path) = name.split_once(':').unwrap_or(("minecraft", &name));
                        if ns == mod_id
                            && !folder
                                .join(format!("assets/{ns}/sounds/{path}.ogg"))
                                .is_file()
                        {
                            warnings.push(format!(
                                "the sound event {} lists {name}, but assets/{ns}/sounds/{path}.ogg does not exist",
                                def.id
                            ));
                        }
                    }
                }
            }
        }
    }
    for def in particles {
        let path = own(&def.id);
        if !folder
            .join(format!("assets/{mod_id}/particles/{path}.json"))
            .is_file()
        {
            warnings.push(format!(
                "the particle type {} has no assets/{mod_id}/particles/{path}.json",
                def.id
            ));
        }
    }
    warnings
}

/// The names of the files (not references to other events) a `sounds.json` entry lists.
fn sound_files(event: &Value) -> Vec<String> {
    event
        .get("sounds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| match s {
            Value::String(name) => Some(name.clone()),
            Value::Object(o) if o.get("type").and_then(Value::as_str) != Some("event") => {
                o.get("name").and_then(Value::as_str).map(str::to_string)
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fish_hooks::TransformKind;

    fn fuel(item: &str, ticks: u32) -> FuelDef {
        FuelDef {
            item: item.into(),
            mod_id: "m".into(),
            burn_ticks: ticks,
        }
    }

    #[test]
    fn the_first_registration_of_a_key_wins() {
        let mut store = ContentStore::default();
        assert!(store.add_fuel(fuel("a:b", 100), 1).is_ok());
        assert_eq!(store.add_fuel(fuel("a:b", 200), 2), Err(1));
        assert!(store.add_fuel(fuel("a:c", 200), 2).is_ok());
        let kept = store.tables(|_| true).fuels;
        assert_eq!(
            kept.iter().map(|f| f.burn_ticks).collect::<Vec<_>>(),
            [100, 200]
        );
    }

    #[test]
    fn a_transform_is_keyed_by_kind_and_block() {
        let mut store = ContentStore::default();
        let def = |kind, from: &str| TransformDef {
            kind,
            from: from.into(),
            to: "a:to".into(),
            mod_id: "m".into(),
        };
        assert!(
            store
                .add_transform(def(TransformKind::Strip, "a:log"), 1)
                .is_ok()
        );
        assert!(
            store
                .add_transform(def(TransformKind::Wax, "a:log"), 1)
                .is_ok()
        );
        assert_eq!(
            store.add_transform(def(TransformKind::Strip, "a:log"), 2),
            Err(1)
        );
    }

    #[test]
    fn every_table_filters_by_live_mods_and_clears() {
        let mut store = ContentStore::default();
        let m = |s: &str| s.to_string();
        store
            .add_compostable(
                CompostDef {
                    item: m("a:x"),
                    mod_id: m("m"),
                    chance: 0.5,
                },
                1,
            )
            .unwrap();
        store
            .add_flammable(
                FlammableDef {
                    block: m("a:x"),
                    mod_id: m("m"),
                    ignite_odds: 5,
                    burn_odds: 20,
                },
                2,
            )
            .unwrap();
        store
            .add_path_type(
                PathTypeDef {
                    block: m("a:x"),
                    mod_id: m("m"),
                    path_type: m("FIRE"),
                },
                1,
            )
            .unwrap();
        store
            .add_sculk(
                SculkFrequencyDef {
                    event: m("minecraft:step"),
                    mod_id: m("m"),
                    frequency: 9,
                },
                2,
            )
            .unwrap();
        store
            .add_sound(
                SoundDef {
                    id: m("m:s"),
                    mod_id: m("m"),
                },
                1,
            )
            .unwrap();
        store
            .add_particle(
                ParticleDef {
                    id: m("m:p"),
                    mod_id: m("m"),
                    always_show: true,
                },
                2,
            )
            .unwrap();
        let only_one = store.tables(|o| o == 1);
        assert_eq!(only_one.compostables.len(), 1);
        assert_eq!(only_one.path_types.len(), 1);
        assert!(only_one.flammables.is_empty() && only_one.sculk_frequencies.is_empty());
        assert_eq!(store.sculk_frequency("minecraft:step", |_| true), Some(9));
        assert_eq!(store.sculk_frequency("minecraft:step", |o| o == 1), None);
        assert_eq!(store.sculk_frequency("minecraft:swim", |_| true), None);
        assert_eq!(store.sounds(|o| o == 1).len(), 1);
        assert!(store.particles(|o| o == 1).is_empty());
        assert!(store.has_assets_of(1) && store.has_assets_of(2) && !store.has_assets_of(3));
        store.clear();
        assert!(store.tables(|_| true).is_empty());
        assert!(!store.has_assets_of(1));
    }

    fn sound(id: &str) -> SoundDef {
        SoundDef {
            id: id.into(),
            mod_id: "ruby".into(),
        }
    }

    fn write(folder: &Path, name: &str, bytes: &[u8]) {
        let path = folder.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn a_complete_content_folder_has_nothing_to_warn_about() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "assets/ruby/sounds.json",
            br#"{"chime": {"sounds": ["ruby:chime", {"name": "ruby:deep", "volume": 0.5}, {"name": "minecraft:other", "type": "event"}]}}"#,
        );
        write(dir.path(), "assets/ruby/sounds/chime.ogg", b"ogg");
        write(dir.path(), "assets/ruby/sounds/deep.ogg", b"ogg");
        write(dir.path(), "assets/ruby/particles/spark.json", b"{}");
        let particle = ParticleDef {
            id: "ruby:spark".into(),
            mod_id: "ruby".into(),
            always_show: false,
        };
        let warnings = check_assets(dir.path(), "ruby", &[&sound("ruby:chime")], &[&particle]);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn missing_sound_files_and_particle_descriptions_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let particle = ParticleDef {
            id: "ruby:spark".into(),
            mod_id: "ruby".into(),
            always_show: true,
        };
        let warnings = check_assets(dir.path(), "ruby", &[&sound("ruby:chime")], &[&particle]);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].contains("no assets/ruby/sounds.json"));
        assert!(warnings[1].contains("assets/ruby/particles/spark.json"));

        write(dir.path(), "assets/ruby/sounds.json", b"not json");
        let warnings = check_assets(dir.path(), "ruby", &[&sound("ruby:chime")], &[]);
        assert!(warnings[0].contains("is not JSON"), "{warnings:?}");

        write(
            dir.path(),
            "assets/ruby/sounds.json",
            br#"{"chime": {"sounds": ["ruby:gone", "minecraft:fine", "other:x"]}}"#,
        );
        let warnings = check_assets(
            dir.path(),
            "ruby",
            &[&sound("ruby:chime"), &sound("ruby:missing")],
            &[],
        );
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].contains("lists ruby:gone") && warnings[0].contains("gone.ogg"));
        assert!(warnings[1].contains("ruby:missing is not in"));
    }
}
