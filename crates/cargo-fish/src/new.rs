//! `cargo fish new`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use fish_metadata::ModId;

/// Project templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Template {
    /// A `main` entrypoint that logs a greeting.
    Basic,
    /// Like `basic`, plus a `client` entrypoint.
    Client,
    /// Assets and data only: no library and no Cargo project.
    Content,
}

/// Derives a mod id from a crate name (`My-Mod` becomes `my_mod`) and validates it.
pub fn mod_id_for(name: &str) -> Result<String> {
    if name != name.to_ascii_lowercase() {
        bail!(
            "cannot use {name:?} as a mod name: use lowercase letters (the library file name must match the mod id)"
        );
    }
    let id = name.to_ascii_lowercase().replace('-', "_");
    ModId::parse(&id).map_err(|e| anyhow::anyhow!("cannot use {name:?} as a mod name: {e}"))?;
    Ok(id)
}

fn cargo_toml(name: &str, fish_dep: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
         [lib]\ncrate-type = [\"cdylib\"]\n\n\
         [dependencies]\n{fish_dep}\nlog = \"0.4\"\n\n\
         # Fish catches panics at the FFI boundary; `panic = \"abort\"` is refused by `cargo fish build`.\n\
         [profile.release]\npanic = \"unwind\"\n"
    )
}

fn manifest(id: &str, name: &str, template: Template) -> String {
    let head = format!(
        "schema = 1\nid = \"{id}\"\nversion = \"0.1.0\"\nname = \"{name}\"\nlicense = \"MIT\"\n"
    );
    match template {
        Template::Content => head,
        Template::Basic => format!(
            "{head}\n[library]\nname = \"{lib}\"\n\n[entrypoints]\nmain = [\"init\"]\n\n[depends]\nfish = \">=0.1.0\"\n",
            lib = id
        ),
        Template::Client => format!(
            "{head}\nenvironment = \"*\"\n\n[library]\nname = \"{lib}\"\n\n[entrypoints]\nmain = [\"init\"]\nclient = [\"init_client\"]\n\n[depends]\nfish = \">=0.1.0\"\n",
            lib = id
        ),
    }
}

fn lib_rs(id: &str, template: Template) -> String {
    match template {
        Template::Content => String::new(),
        Template::Basic => format!(
            "fn init() {{\n    log::info!(\"Hello from {id}\");\n}}\n\nfish_api::export_mod! {{\n    main: [init],\n}}\n"
        ),
        Template::Client => format!(
            "fn init() {{\n    log::info!(\"Hello from {id}\");\n}}\n\nfn init_client() {{\n    log::info!(\"Hello from the {id} client\");\n}}\n\nfish_api::export_mod! {{\n    main: [init],\n    client: [init_client],\n}}\n"
        ),
    }
}

fn test_script(id: &str, template: Template) -> String {
    match template {
        Template::Content => {
            "# Content mods have no code; add steps as features arrive.\n[[step]]\nticks = 1\n"
                .to_string()
        }
        Template::Basic => format!("[[step]]\n[step.expect]\nlogs = [\"Hello from {id}\"]\n"),
        Template::Client => format!(
            "[[step]]\n[step.expect]\nlogs = [\"Hello from {id}\", \"Hello from the {id} client\"]\n"
        ),
    }
}

/// Creates `<parent>/<name>`; returns its path.
///
/// With `fish_path` (a Fish checkout) the `fish-api` dependency is a path dependency, otherwise it
/// is the crates.io version matching this `cargo-fish`.
pub fn create(
    parent: &Path,
    name: &str,
    template: Template,
    fish_path: Option<&Path>,
) -> Result<PathBuf> {
    let id = mod_id_for(name)?;
    let root = parent.join(name);
    if root.exists()
        && std::fs::read_dir(&root)
            .map(|mut d| d.next().is_some())
            .unwrap_or(true)
    {
        bail!("{} already exists and is not empty", root.display());
    }
    let fish_dep = match fish_path {
        Some(p) => {
            let abs = std::fs::canonicalize(p)
                .with_context(|| format!("--fish-path {} does not exist", p.display()))?;
            let api = abs.join("crates/fish-api");
            if !api.join("Cargo.toml").is_file() {
                bail!(
                    "{} does not look like a Fish checkout (no crates/fish-api)",
                    abs.display()
                );
            }
            let text = api.display().to_string();
            let text = text
                .strip_prefix(r"\\?\")
                .unwrap_or(&text)
                .replace('\\', "/");
            format!("fish-api = {{ path = '{text}' }}")
        }
        None => format!("fish-api = \"{}\"", env!("CARGO_PKG_VERSION")),
    };
    let display_name = name.replace(['-', '_'], " ");
    let mut files: Vec<(String, String)> = vec![
        (
            "fish.mod.toml".into(),
            manifest(&id, &display_name, template),
        ),
        ("tests/hello.toml".into(), test_script(&id, template)),
        (
            format!("assets/{id}/lang/en_us.json"),
            format!("{{\n  \"mod.{id}.name\": \"{display_name}\"\n}}\n"),
        ),
    ];
    if template != Template::Content {
        files.push(("Cargo.toml".into(), cargo_toml(name, &fish_dep)));
        files.push(("src/lib.rs".into(), lib_rs(&id, template)));
    }
    for (rel, text) in files {
        let path = root.join(&rel);
        std::fs::create_dir_all(path.parent().expect("file has a parent"))
            .with_context(|| format!("creating {}", path.display()))?;
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_derived_and_validated() {
        assert_eq!(mod_id_for("my-mod").unwrap(), "my_mod");
        assert!(
            mod_id_for("My-Mod")
                .unwrap_err()
                .to_string()
                .contains("lowercase")
        );
        assert!(mod_id_for("x").is_err());
        assert!(mod_id_for("1abc").is_err());
        assert!(
            mod_id_for("fish")
                .unwrap_err()
                .to_string()
                .contains("reserved")
        );
    }

    #[test]
    fn templates_produce_valid_manifests() {
        for t in [Template::Basic, Template::Client, Template::Content] {
            let m = fish_metadata::parse(&manifest("demo_mod", "demo mod", t)).unwrap();
            assert_eq!(m.id.as_str(), "demo_mod");
            assert_eq!(m.library.is_some(), t != Template::Content);
        }
    }

    #[test]
    fn refuses_to_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("demo")).unwrap();
        std::fs::write(dir.path().join("demo/x"), b"1").unwrap();
        let err = create(dir.path(), "demo", Template::Basic, None).unwrap_err();
        assert!(err.to_string().contains("not empty"));
        // an existing empty directory is fine
        std::fs::create_dir_all(dir.path().join("empty_one")).unwrap();
        create(dir.path(), "empty_one", Template::Basic, None).unwrap();
        let toml = std::fs::read_to_string(dir.path().join("empty_one/Cargo.toml")).unwrap();
        assert!(toml.contains(&format!("fish-api = \"{}\"", env!("CARGO_PKG_VERSION"))));
        assert!(toml.contains("panic = \"unwind\""));
    }

    #[test]
    fn bad_fish_path_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let err = create(
            dir.path(),
            "demo",
            Template::Basic,
            Some(&dir.path().join("nope")),
        )
        .unwrap_err();
        assert!(err.to_string().contains("does not exist"));
        let err = create(dir.path(), "demo", Template::Basic, Some(dir.path())).unwrap_err();
        assert!(
            err.to_string()
                .contains("does not look like a Fish checkout")
        );
    }
}
