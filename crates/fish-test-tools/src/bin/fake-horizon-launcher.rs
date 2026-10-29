//! A stand-in for `horizon-launcher`: the subset of commands the Fish installer uses, with state
//! in a JSON file (`FAKE_LAUNCHER_STATE`) so tests can inspect every call and its arguments.
//!
//! * `FAKE_LAUNCHER_SHORT`: commit suffix given to versions installed by `install` (default `6f8c7a0`).
//! * `FAKE_LAUNCHER_REJECT_EMPTY_WRAPPER` set, or a file `reject-empty-wrapper` next to the
//!   executable: `instance launch X --wrapper ""` fails.

use serde_json::{Value, json};

fn exe_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .expect("own path")
        .parent()
        .expect("exe dir")
        .to_path_buf()
}

/// `FAKE_LAUNCHER_STATE`, else `state.json` next to this executable (one copy per test).
fn state_path() -> std::path::PathBuf {
    std::env::var_os("FAKE_LAUNCHER_STATE")
        .map(Into::into)
        .unwrap_or_else(|| exe_dir().join("state.json"))
}

fn load() -> Value {
    std::fs::read_to_string(state_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| {
        let dir = state_path().parent().unwrap().to_path_buf();
        json!({
            "installed": [],
            "instances": [{"id": "horizon", "name": "Horizon", "home": dir.join("game-home"), "version": null, "wrapper": "", "selected": true}],
            "calls": []
        })
    })
}

fn save(state: &Value) {
    std::fs::write(state_path(), serde_json::to_string_pretty(state).unwrap()).unwrap();
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

fn find<'a>(state: &'a mut Value, query: &str) -> &'a mut Value {
    let idx = state["instances"].as_array().unwrap().iter().position(|i| {
        i["id"] == query
            || i["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(query))
    });
    match idx {
        Some(i) => &mut state["instances"][i],
        None => fail(&format!(
            "No instance {query}: horizon-launcher instance list shows them"
        )),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut state = load();
    state["calls"].as_array_mut().unwrap().push(json!(args));
    let words: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "--json")
        .collect();
    match words.as_slice() {
        ["versions", "--installed"] => {
            println!(
                "{}",
                json!({"installed": state["installed"], "selected": null})
            );
        }
        ["install", tag] => {
            let short = std::env::var("FAKE_LAUNCHER_SHORT").unwrap_or_else(|_| "6f8c7a0".into());
            state["installed"]
                .as_array_mut()
                .unwrap()
                .push(json!(format!("{tag}-{short}")));
        }
        ["instance", "list"] => {
            println!("{}", state["instances"]);
        }
        ["instance", "create", name] => {
            let exists = state["instances"].as_array().unwrap().iter().any(|i| {
                i["name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            });
            if exists {
                fail(&format!("An instance called {name} already exists"));
            }
            let slug = name.to_lowercase().replace(' ', "-");
            let home = state_path().parent().unwrap().join("instances").join(&slug);
            std::fs::create_dir_all(&home).unwrap();
            state["instances"].as_array_mut().unwrap().push(
                json!({"id": slug, "name": name, "home": home, "version": null, "wrapper": "", "selected": false}),
            );
        }
        ["instance", "version", inst, tag] => {
            if !state["installed"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t == tag)
            {
                fail(&format!("{tag} is not installed"));
            }
            find(&mut state, inst)["version"] = json!(tag);
        }
        ["instance", "launch", inst, rest @ ..] => {
            let i = find(&mut state, inst);
            if let ["--wrapper", value] = rest {
                if value.is_empty()
                    && (std::env::var("FAKE_LAUNCHER_REJECT_EMPTY_WRAPPER").is_ok()
                        || exe_dir().join("reject-empty-wrapper").exists())
                {
                    fail("--wrapper needs a value");
                }
                i["wrapper"] = json!(value.trim());
            }
            println!(
                "{}",
                json!({"instance": i["id"], "pre_launch": "", "wrapper": i["wrapper"], "env": {}})
            );
        }
        ["instance", "select", inst] => {
            let id = find(&mut state, inst)["id"].clone();
            for i in state["instances"].as_array_mut().unwrap() {
                i["selected"] = json!(i["id"] == id);
            }
        }
        _ => fail(&format!(
            "fake-horizon-launcher: unsupported command {words:?}"
        )),
    }
    save(&state);
}
