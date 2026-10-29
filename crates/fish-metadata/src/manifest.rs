//! The `fish.mod.toml` model and parser.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use semver::Version;
use serde::{Deserialize, Deserializer};

use crate::id::{ModId, RefId};
use crate::version::VersionReqSet;

/// Highest manifest schema this Fish understands.
pub const SUPPORTED_SCHEMA: u32 = 1;

/// Which side the game is running as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    /// A client (window, integrated server included).
    Client,
    /// A dedicated server.
    Server,
}

/// Where a mod may load (`environment` in the manifest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Environment {
    /// `"*"`: everywhere.
    #[default]
    Any,
    /// `"client"` only.
    Client,
    /// `"server"` only.
    Server,
}

impl Environment {
    /// True if a mod with this environment loads on `side`.
    pub fn allows(self, side: Side) -> bool {
        matches!(
            (self, side),
            (Environment::Any, _)
                | (Environment::Client, Side::Client)
                | (Environment::Server, Side::Server)
        )
    }
}

impl<'de> Deserialize<'de> for Environment {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match String::deserialize(d)?.as_str() {
            "*" => Ok(Environment::Any),
            "client" => Ok(Environment::Client),
            "server" => Ok(Environment::Server),
            other => Err(serde::de::Error::custom(format!(
                "invalid environment {other:?}: expected \"*\", \"client\" or \"server\""
            ))),
        }
    }
}

/// Dependency table: id → requirement set.
pub type DepMap = BTreeMap<ModId, VersionReqSet>;

/// `[library]` table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    /// Library base name; the file is `DLL_PREFIX + name + DLL_SUFFIX`.
    pub name: String,
}

/// A parsed, validated mod manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    /// Manifest schema (currently 1).
    pub schema: u32,
    /// Unique mod id.
    pub id: ModId,
    /// Mod version.
    pub version: Version,
    /// Display name (defaults to the id).
    pub name: String,
    /// Free-text description.
    pub description: String,
    /// Authors.
    pub authors: Vec<String>,
    /// Contributors.
    pub contributors: Vec<String>,
    /// SPDX license string (not validated).
    pub license: Option<String>,
    /// Icon path inside the archive.
    pub icon: Option<String>,
    /// Where the mod loads.
    pub environment: Environment,
    /// Alias ids this mod also answers to.
    pub provides: Vec<ModId>,
    /// Nested archives (archive paths).
    pub nested: Vec<String>,
    /// Contact links (`homepage`, `sources`, …).
    pub contact: BTreeMap<String, String>,
    /// Native library, if the mod has code.
    pub library: Option<Library>,
    /// Entrypoint key → exported names.
    pub entrypoints: BTreeMap<String, Vec<String>>,
    /// Hard dependencies.
    pub depends: DepMap,
    /// Soft dependencies; a miss warns.
    pub recommends: DepMap,
    /// Informational dependencies.
    pub suggests: DepMap,
    /// Matching mods produce a warning.
    pub conflicts: DepMap,
    /// Matching mods are a hard error.
    pub breaks: DepMap,
    /// Arbitrary mod-defined data.
    pub custom: toml::Table,
}

impl Manifest {
    /// Names exported for entrypoint `key` (empty if none).
    pub fn entrypoints_for(&self, key: &str) -> &[String] {
        self.entrypoints.get(key).map_or(&[], Vec::as_slice)
    }

    /// Synthetic manifest for a loader-provided builtin.
    pub(crate) fn builtin(id: &'static str, version: Version) -> Manifest {
        Manifest {
            schema: SUPPORTED_SCHEMA,
            id: ModId::builtin(id),
            version,
            name: id.to_string(),
            description: String::new(),
            authors: vec![],
            contributors: vec![],
            license: None,
            icon: None,
            environment: Environment::Any,
            provides: vec![],
            nested: vec![],
            contact: BTreeMap::new(),
            library: None,
            entrypoints: BTreeMap::new(),
            depends: DepMap::new(),
            recommends: DepMap::new(),
            suggests: DepMap::new(),
            conflicts: DepMap::new(),
            breaks: DepMap::new(),
            custom: toml::Table::new(),
        }
    }
}

/// A manifest problem, with the position in the TOML text when known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError {
    /// Human readable message.
    pub message: String,
    /// 1-based line, if the problem has a source position.
    pub line: Option<usize>,
    /// 1-based column, if the problem has a source position.
    pub column: Option<usize>,
}

impl ManifestError {
    fn plain(message: impl Into<String>) -> Self {
        ManifestError {
            message: message.into(),
            line: None,
            column: None,
        }
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(l), Some(c)) => write!(f, "{l}:{c}: {}", self.message),
            _ => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ManifestError {}

fn position(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}

fn from_toml_error(text: &str, e: &toml::de::Error) -> ManifestError {
    let (line, column) = match e.span() {
        Some(span) => {
            let (l, c) = position(text, span.start);
            (Some(l), Some(c))
        }
        None => (None, None),
    };
    ManifestError {
        message: e.message().to_string(),
        line,
        column,
    }
}

fn de_dep_map<'de, D: Deserializer<'de>>(d: D) -> Result<DepMap, D::Error> {
    let raw = BTreeMap::<RefId, VersionReqSet>::deserialize(d)?;
    Ok(raw.into_iter().map(|(k, v)| (k.0, v)).collect())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    schema: u32,
    id: ModId,
    version: Version,
    name: Option<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    authors: Vec<String>,
    #[serde(default)]
    contributors: Vec<String>,
    license: Option<String>,
    icon: Option<String>,
    #[serde(default)]
    environment: Environment,
    #[serde(default)]
    provides: Vec<ModId>,
    #[serde(default)]
    nested: Vec<String>,
    #[serde(default)]
    contact: BTreeMap<String, String>,
    library: Option<Library>,
    #[serde(default)]
    entrypoints: BTreeMap<String, Vec<String>>,
    #[serde(default, deserialize_with = "de_dep_map")]
    depends: DepMap,
    #[serde(default, deserialize_with = "de_dep_map")]
    recommends: DepMap,
    #[serde(default, deserialize_with = "de_dep_map")]
    suggests: DepMap,
    #[serde(default, deserialize_with = "de_dep_map")]
    conflicts: DepMap,
    #[serde(default, deserialize_with = "de_dep_map")]
    breaks: DepMap,
    #[serde(default)]
    custom: toml::Table,
}

/// True for a relative, `/`-separated archive path without `..`, `\`, `:` or NUL.
fn is_safe_archive_path(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains(['\\', ':', '\0'])
        && p.split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

fn is_c_ident(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn validate(raw: Raw) -> Result<Manifest, ManifestError> {
    let err = |m: String| Err(ManifestError::plain(m));
    if let Some(icon) = &raw.icon
        && !is_safe_archive_path(icon)
    {
        return err(format!("icon {icon:?} is not a safe relative archive path"));
    }
    for n in &raw.nested {
        if !is_safe_archive_path(n) || !n.ends_with(".fish") {
            return err(format!(
                "nested entry {n:?} must be a relative path to a .fish archive"
            ));
        }
    }
    let mut seen = BTreeSet::new();
    for p in &raw.provides {
        if *p == raw.id {
            return err("provides must not contain the mod's own id".into());
        }
        if !seen.insert(p) {
            return err(format!("provides lists {p:?} twice"));
        }
    }
    if let Some(lib) = &raw.library
        && !is_c_ident(&lib.name)
    {
        return err(format!(
            "library name {:?} must match [A-Za-z_][A-Za-z0-9_]*",
            lib.name
        ));
    }
    for (key, names) in &raw.entrypoints {
        if key.is_empty() {
            return err("entrypoint keys must not be empty".into());
        }
        for n in names {
            if !is_c_ident(n) {
                return err(format!(
                    "entrypoint {key} names {n:?}, which is not a valid export name"
                ));
            }
        }
    }
    if raw.entrypoints.values().any(|v| !v.is_empty()) && raw.library.is_none() {
        return err("entrypoints are declared but there is no [library] table".into());
    }
    let name = raw.name.unwrap_or_else(|| raw.id.to_string());
    Ok(Manifest {
        schema: raw.schema,
        id: raw.id,
        version: raw.version,
        name,
        description: raw.description,
        authors: raw.authors,
        contributors: raw.contributors,
        license: raw.license,
        icon: raw.icon,
        environment: raw.environment,
        provides: raw.provides,
        nested: raw.nested,
        contact: raw.contact,
        library: raw.library,
        entrypoints: raw.entrypoints,
        depends: raw.depends,
        recommends: raw.recommends,
        suggests: raw.suggests,
        conflicts: raw.conflicts,
        breaks: raw.breaks,
        custom: raw.custom,
    })
}

/// Parses and validates a `fish.mod.toml`.
///
/// Unknown keys are errors. A schema newer than [`SUPPORTED_SCHEMA`] is refused before anything
/// else is looked at, so manifests of future Fish versions get a clear message.
pub fn parse(text: &str) -> Result<Manifest, ManifestError> {
    let loose: toml::Table = text.parse().map_err(|e| from_toml_error(text, &e))?;
    match loose.get("schema") {
        Some(toml::Value::Integer(n)) if *n > i64::from(SUPPORTED_SCHEMA) => {
            return Err(ManifestError::plain(format!(
                "this mod was made for a newer Fish (schema {n}); update Fish"
            )));
        }
        Some(toml::Value::Integer(n)) if *n < 1 => {
            return Err(ManifestError::plain(format!("invalid schema {n}")));
        }
        _ => {}
    }
    let raw: Raw = toml::from_str(text).map_err(|e| from_toml_error(text, &e))?;
    validate(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: &str = "schema = 1\nid = \"example_mod\"\nversion = \"1.0.0\"\n";

    fn with(extra: &str) -> String {
        format!("{MIN}{extra}")
    }

    #[test]
    fn minimal_manifest_defaults() {
        let m = parse(MIN).unwrap();
        assert_eq!(m.id.as_str(), "example_mod");
        assert_eq!(m.name, "example_mod");
        assert_eq!(m.environment, Environment::Any);
        assert!(m.entrypoints_for("main").is_empty());
        assert!(m.library.is_none());
    }

    #[test]
    fn full_manifest() {
        let text = r#"
schema = 1
id = "example_mod"
version = "1.2.3-beta.1"
name = "Example Mod"
description = "d"
authors = ["a"]
contributors = ["c"]
license = "MIT"
icon = "icon.png"
environment = "client"
provides = ["alias_id"]
nested = ["nested/lib.fish"]

[contact]
homepage = "https://x"

[library]
name = "example_mod"

[entrypoints]
main = ["init"]
client = ["init_client"]
"fish:modmenu" = ["menu_api"]

[depends]
fish = ">=0.1.0"
other = [">=1", "<0.5"]
[recommends]
rr = "*"
[suggests]
ss = "*"
[conflicts]
cc = "<2"
[breaks]
bb = "<1"

[custom]
k = 1
"#;
        let m = parse(text).unwrap();
        assert_eq!(m.name, "Example Mod");
        assert_eq!(m.environment, Environment::Client);
        assert_eq!(m.entrypoints_for("main"), ["init"]);
        assert_eq!(m.entrypoints_for("fish:modmenu"), ["menu_api"]);
        assert_eq!(m.depends.len(), 2);
        assert_eq!(
            m.depends[&ModId::parse_reference("other").unwrap()].to_string(),
            ">=1 or <0.5"
        );
        assert_eq!(m.custom["k"].as_integer(), Some(1));
        assert_eq!(m.contact["homepage"], "https://x");
        assert_eq!(m.provides[0].as_str(), "alias_id");
        assert_eq!(m.library.unwrap().name, "example_mod");
    }

    #[test]
    fn environment_rules() {
        assert!(Environment::Any.allows(Side::Client) && Environment::Any.allows(Side::Server));
        assert!(
            Environment::Client.allows(Side::Client) && !Environment::Client.allows(Side::Server)
        );
        assert!(
            Environment::Server.allows(Side::Server) && !Environment::Server.allows(Side::Client)
        );
        assert_eq!(
            parse(&with("environment = \"server\""))
                .unwrap()
                .environment,
            Environment::Server
        );
        assert!(
            parse(&with("environment = \"both\""))
                .unwrap_err()
                .message
                .contains("invalid environment")
        );
    }

    #[test]
    fn schema_handling() {
        let e = parse("schema = 2\nid = \"x\"\nversion = \"1.0.0\"\nnew_field = 1\n").unwrap_err();
        assert_eq!(
            e.message,
            "this mod was made for a newer Fish (schema 2); update Fish"
        );
        assert!(
            parse("schema = 0\nid = \"xx\"\nversion = \"1.0.0\"")
                .unwrap_err()
                .message
                .contains("invalid schema")
        );
        assert!(
            parse("id = \"xx\"\nversion = \"1.0.0\"")
                .unwrap_err()
                .message
                .contains("schema")
        );
    }

    #[test]
    fn required_and_unknown_fields() {
        assert!(
            parse("schema = 1\nid = \"xx\"")
                .unwrap_err()
                .message
                .contains("version")
        );
        assert!(
            parse("schema = 1\nversion = \"1.0.0\"")
                .unwrap_err()
                .message
                .contains("id")
        );
        assert!(
            parse(&with("surprise = 1"))
                .unwrap_err()
                .message
                .contains("unknown field")
        );
        assert!(parse(&with("[library]\nname = \"a\"\nextra = 1")).is_err());
        assert!(parse("schema = 1\nid = \"xx\"\nversion = \"one\"").is_err());
    }

    #[test]
    fn reserved_and_invalid_ids() {
        let e = parse("schema = 1\nid = \"minecraft\"\nversion = \"1.0.0\"").unwrap_err();
        assert_eq!(e.message, "id minecraft is reserved by Fish");
        assert!(
            parse("schema = 1\nid = \"Bad\"\nversion = \"1.0.0\"")
                .unwrap_err()
                .message
                .contains("invalid mod id")
        );
        assert!(
            parse(&with("provides = [\"fish\"]"))
                .unwrap_err()
                .message
                .contains("reserved")
        );
        // references may name reserved ids
        assert!(parse(&with("[depends]\nminecraft = \">=26\"")).is_ok());
        assert!(parse(&with("[depends]\nBad = \"*\"")).is_err());
    }

    #[test]
    fn error_carries_position() {
        let e = parse("schema = 1\nid = \"xx\"\nversion = \"nope\"\n").unwrap_err();
        assert_eq!((e.line, e.column), (Some(3), Some(11)), "{e}");
        assert!(e.to_string().starts_with("3:11: "));
        let e = parse("schema = = 1").unwrap_err();
        assert!(e.line.is_some());
        assert_eq!(ManifestError::plain("x").to_string(), "x");
    }

    #[test]
    fn provides_rules() {
        assert!(
            parse(&with("provides = [\"example_mod\"]"))
                .unwrap_err()
                .message
                .contains("own id")
        );
        assert!(
            parse(&with("provides = [\"aa\", \"aa\"]"))
                .unwrap_err()
                .message
                .contains("twice")
        );
    }

    #[test]
    fn path_rules() {
        assert!(
            parse(&with("icon = \"../x.png\""))
                .unwrap_err()
                .message
                .contains("icon")
        );
        assert!(parse(&with("icon = \"/x.png\"")).is_err());
        assert!(parse(&with("icon = \"a\\\\b.png\"")).is_err());
        assert!(parse(&with("icon = \"c:x.png\"")).is_err());
        assert!(parse(&with("icon = \"a//b.png\"")).is_err());
        assert!(parse(&with("icon = \"assets/icon.png\"")).is_ok());
        assert!(
            parse(&with("nested = [\"n/x.zip\"]"))
                .unwrap_err()
                .message
                .contains("nested")
        );
        assert!(parse(&with("nested = [\"../x.fish\"]")).is_err());
    }

    #[test]
    fn library_and_entrypoint_rules() {
        assert!(
            parse(&with("[library]\nname = \"bad name\""))
                .unwrap_err()
                .message
                .contains("library name")
        );
        assert!(parse(&with("[library]\nname = \"1x\"")).is_err());
        assert!(
            parse(&with("[entrypoints]\nmain = [\"init\"]"))
                .unwrap_err()
                .message
                .contains("no [library]")
        );
        let lib = "[library]\nname = \"m\"\n[entrypoints]\n";
        assert!(
            parse(&with(&format!("{lib}main = [\"bad-name\"]")))
                .unwrap_err()
                .message
                .contains("export name")
        );
        assert!(parse(&with(&format!("{lib}\"\" = [\"ok\"]"))).is_err());
        assert!(parse(&with(&format!("{lib}main = []"))).is_ok());
        assert!(parse(&with("[entrypoints]\nmain = []")).is_ok());
    }

    #[test]
    fn bad_requirement_is_positioned() {
        let e = parse(&with("[depends]\nother = \"???\"")).unwrap_err();
        assert!(e.message.contains("invalid version requirement"));
        assert_eq!(e.line, Some(5));
    }

    #[test]
    fn archive_path_helper() {
        assert!(is_safe_archive_path("a/b.png"));
        assert!(!is_safe_archive_path(""));
        assert!(!is_safe_archive_path("./a"));
        assert!(!is_safe_archive_path("a\0b"));
    }
}
