//! Roadmap checklist parsing and en/ru comparison.

use regex::Regex;

/// One checklist line of `roadmap.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub done: bool,
}

/// Extracts `- [ ] **P<n>.<m>**` items in file order.
pub fn parse(text: &str) -> Vec<Item> {
    let re = Regex::new(r"^- \[( |x)\] \*\*(P\d+\.\d+)\*\*").expect("static regex");
    text.lines()
        .filter_map(|line| {
            let caps = re.captures(line)?;
            Some(Item {
                id: caps[2].to_string(),
                done: &caps[1] == "x",
            })
        })
        .collect()
}

/// Marks item `id` as done; returns `None` if the item is absent.
pub fn tick(text: &str, id: &str) -> Option<String> {
    let needle = format!("- [ ] **{id}**");
    let start = if text.starts_with(&needle) {
        0
    } else {
        text.find(&format!("\n{needle}"))? + 1
    };
    let mut out = text.to_string();
    out.replace_range(start + 3..start + 4, "x");
    Some(out)
}

/// Compares two parsed roadmaps; returns one message per difference.
pub fn compare(en: &[Item], ru: &[Item]) -> Vec<String> {
    let mut errors = Vec::new();
    if en.len() != ru.len() {
        errors.push(format!(
            "roadmap item count differs: en={} ru={}",
            en.len(),
            ru.len()
        ));
    }
    for (i, (a, b)) in en.iter().zip(ru).enumerate() {
        if a.id != b.id {
            errors.push(format!(
                "roadmap item #{} differs: en={} ru={}",
                i + 1,
                a.id,
                b.id
            ));
        } else if a.done != b.done {
            errors.push(format!(
                "roadmap item {} check state differs between en and ru",
                a.id
            ));
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Roadmap\n- [ ] **P0.1** One\n- [x] **P0.2** Two\nnot an item\n  - [ ] **P0.3** indented\n- [x]  **P1.1** bad spacing\n- [ ] **P10.12** Big\n";

    #[test]
    fn parses_ids_and_states() {
        let items = parse(SAMPLE);
        assert_eq!(
            items,
            vec![
                Item {
                    id: "P0.1".into(),
                    done: false
                },
                Item {
                    id: "P0.2".into(),
                    done: true
                },
                Item {
                    id: "P10.12".into(),
                    done: false
                },
            ]
        );
    }

    #[test]
    fn tick_marks_only_the_named_item() {
        let t = "- [ ] **P0.1** a\n- [ ] **P0.10** b\n- [ ] **P0.2** c\n";
        let out = tick(t, "P0.1").unwrap();
        assert_eq!(
            out,
            "- [x] **P0.1** a\n- [ ] **P0.10** b\n- [ ] **P0.2** c\n"
        );
        let out = tick(&out, "P0.2").unwrap();
        assert!(out.ends_with("- [x] **P0.2** c\n"));
        assert!(tick(&out, "P0.1").is_none());
        assert!(tick(&out, "P9.9").is_none());
    }

    #[test]
    fn compare_reports_state_id_and_count_differences() {
        let a = parse("- [ ] **P0.1** a\n- [x] **P0.2** b\n");
        assert!(compare(&a, &a).is_empty());
        let b = parse("- [ ] **P0.1** a\n- [ ] **P0.2** b\n");
        assert_eq!(compare(&a, &b).len(), 1);
        let c = parse("- [ ] **P0.1** a\n- [x] **P0.3** b\n");
        assert!(compare(&a, &c)[0].contains("differs"));
        let d = parse("- [ ] **P0.1** a\n");
        assert!(compare(&a, &d)[0].contains("count"));
    }
}
