//! Dependency resolution and load ordering.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::path::PathBuf;

use crate::id::ModId;
use crate::manifest::{DepMap, Environment, Manifest, Side};
use crate::version::{Builtins, VersionReqSet};

/// Search steps before resolution gives up.
pub const STEP_LIMIT: usize = 100_000;

/// Where a candidate was found.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    /// A development mod (`FISH_LOADER_ADD_MODS`); wins ties against installed mods.
    Dev(PathBuf),
    /// A file or directory in `fish-mods/`.
    Folder(PathBuf),
    /// An archive nested inside another mod.
    Nested {
        /// The mod that contains the archive.
        parent: ModId,
        /// Archive path inside the parent.
        path: String,
    },
    /// Supplied by the loader.
    Builtin,
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Origin::Dev(p) | Origin::Folder(p) => write!(f, "{}", p.display()),
            Origin::Nested { parent, path } => write!(f, "{parent}!/{path}"),
            Origin::Builtin => f.write_str("built in"),
        }
    }
}

/// Whether a candidate must load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LoadCondition {
    /// Must load; failure to satisfy it is an error.
    Always,
    /// Loads only if its requirements can be met.
    IfPossible,
}

/// A mod that could be loaded.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Its manifest.
    pub manifest: Manifest,
    /// Where it came from.
    pub origin: Origin,
    /// Whether it must load.
    pub condition: LoadCondition,
}

/// A selected mod.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedMod {
    /// Its manifest.
    pub manifest: Manifest,
    /// Where it came from.
    pub origin: Origin,
}

/// Why a candidate is not active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// Its `environment` excludes the running side.
    Environment(Environment),
    /// Another copy with the same id was kept.
    Duplicate {
        /// Origin of the copy that was kept.
        kept: Origin,
    },
    /// An optional (nested) mod whose requirements cannot be met.
    Unsatisfied(String),
    /// Another version of the same id was selected.
    NotSelected,
}

/// Outcome of a successful resolution.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolution {
    /// Selected mods in load order (builtins excluded).
    pub active: Vec<ResolvedMod>,
    /// Candidates that were left out, with the reason.
    pub inactive: Vec<(ModId, Reason)>,
    /// Non-fatal findings.
    pub warnings: Vec<String>,
}

/// One reason resolution failed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Problem {
    /// The mod whose requirement failed.
    pub subject: ModId,
    /// Human readable message.
    pub message: String,
}

/// Resolution failed; lists every problem found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionError {
    /// All problems, sorted.
    pub problems: Vec<Problem>,
}

impl fmt::Display for ResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, p) in self.problems.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            f.write_str(&p.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for ResolutionError {}

fn req_ok(req: &VersionReqSet, c: &Candidate) -> bool {
    if c.origin == Origin::Builtin {
        req.matches_lenient(&c.manifest.version)
    } else {
        req.matches(&c.manifest.version)
    }
}

fn describe(c: &Candidate) -> String {
    format!("mod \"{}\" {}", c.manifest.id, c.manifest.version)
}

fn keys(c: &Candidate) -> impl Iterator<Item = &ModId> {
    std::iter::once(&c.manifest.id).chain(c.manifest.provides.iter())
}

fn is_own_key(c: &Candidate, key: &ModId) -> bool {
    keys(c).any(|k| k == key)
}

/// `a` breaks `b` if any of `b`'s keys appears in `a.breaks` with a matching version.
fn breaks_hit(a: &Candidate, b: &Candidate) -> bool {
    keys(b).any(|k| a.manifest.breaks.get(k).is_some_and(|r| req_ok(r, b)))
}

struct Search<'a> {
    cands: &'a [Candidate],
    groups: Vec<(ModId, Vec<usize>)>,
    required: Vec<bool>,
    last_provider: BTreeMap<ModId, usize>,
    taken: BTreeMap<ModId, usize>,
    chosen: Vec<usize>,
    steps: usize,
    limit: usize,
}

impl<'a> Search<'a> {
    fn new(cands: &'a [Candidate], limit: usize) -> Self {
        let mut by_id: BTreeMap<&ModId, Vec<usize>> = BTreeMap::new();
        for (i, c) in cands.iter().enumerate() {
            by_id.entry(&c.manifest.id).or_default().push(i);
        }
        let groups: Vec<(ModId, Vec<usize>)> =
            by_id.into_iter().map(|(k, v)| (k.clone(), v)).collect();
        let required = groups
            .iter()
            .map(|(_, v)| {
                v.iter()
                    .any(|&i| cands[i].condition == LoadCondition::Always)
            })
            .collect();
        let mut last_provider: BTreeMap<ModId, usize> = BTreeMap::new();
        for (g, (_, members)) in groups.iter().enumerate() {
            for &i in members {
                for k in keys(&cands[i]) {
                    let e = last_provider.entry(k.clone()).or_insert(g);
                    *e = (*e).max(g);
                }
            }
        }
        Search {
            cands,
            groups,
            required,
            last_provider,
            taken: BTreeMap::new(),
            chosen: Vec::new(),
            steps: 0,
            limit,
        }
    }

    fn can_take(&self, ci: usize) -> bool {
        let c = &self.cands[ci];
        if keys(c).any(|k| self.taken.contains_key(k)) {
            return false;
        }
        !self
            .chosen
            .iter()
            .any(|&mi| breaks_hit(&self.cands[mi], c) || breaks_hit(c, &self.cands[mi]))
    }

    fn take(&mut self, ci: usize) {
        for k in keys(&self.cands[ci]) {
            self.taken.insert(k.clone(), ci);
        }
        self.chosen.push(ci);
    }

    fn untake(&mut self, ci: usize) {
        for k in keys(&self.cands[ci]) {
            self.taken.remove(k);
        }
        self.chosen.pop();
    }

    /// Checks every dependency whose providers are all decided (groups `<= upto`).
    fn deps_ok(&self, upto: usize) -> bool {
        self.chosen.iter().all(|&mi| {
            let c = &self.cands[mi];
            c.manifest.depends.iter().all(|(key, req)| {
                if is_own_key(c, key) {
                    return true;
                }
                let decided = self.last_provider.get(key).is_none_or(|&g| g <= upto);
                !decided
                    || self
                        .taken
                        .get(key)
                        .is_some_and(|&ti| req_ok(req, &self.cands[ti]))
            })
        })
    }

    fn dfs(&mut self, gi: usize) -> Result<bool, ()> {
        self.steps += 1;
        if self.steps > self.limit {
            return Err(());
        }
        if gi == self.groups.len() {
            return Ok(self.deps_ok(usize::MAX));
        }
        let options = self.groups[gi].1.clone();
        for ci in options {
            if !self.can_take(ci) {
                continue;
            }
            self.take(ci);
            if self.deps_ok(gi) && self.dfs(gi + 1)? {
                return Ok(true);
            }
            self.untake(ci);
        }
        if !self.required[gi] && self.deps_ok(gi) && self.dfs(gi + 1)? {
            return Ok(true);
        }
        Ok(false)
    }
}

/// Candidates that answer to `key` (by id or `provides`).
fn providers<'a>(cands: &'a [Candidate], key: &ModId) -> Vec<&'a Candidate> {
    cands.iter().filter(|c| is_own_key(c, key)).collect()
}

fn installed_list(found: &[&Candidate]) -> String {
    let versions: Vec<String> = found
        .iter()
        .map(|c| c.manifest.version.to_string())
        .collect();
    let origins: Vec<String> = found.iter().map(|c| c.origin.to_string()).collect();
    if found.len() == 1 {
        format!("only {} is installed ({})", versions[0], origins[0])
    } else {
        format!(
            "only {} are installed ({})",
            versions.join(", "),
            origins.join(", ")
        )
    }
}

/// Message for a dependency of `c` that no candidate satisfies; `None` if one does.
fn unmet_message(
    cands: &[Candidate],
    c: &Candidate,
    key: &ModId,
    req: &VersionReqSet,
) -> Option<String> {
    let found = providers(cands, key);
    if found.iter().any(|p| req_ok(req, p)) {
        return None;
    }
    let tail = if found.is_empty() {
        format!("but \"{key}\" is not installed")
    } else {
        format!("but {}", installed_list(&found))
    };
    Some(format!("{} needs \"{key}\" {req}, {tail}", describe(c)))
}

/// Walks the dependency closure of the required mods and reports everything wrong with it.
fn diagnose(cands: &[Candidate], search: &Search<'_>) -> Vec<Problem> {
    let mut problems = BTreeSet::new();
    let mut visited: BTreeMap<&ModId, &Candidate> = BTreeMap::new();
    let mut queue: VecDeque<&Candidate> = VecDeque::new();
    for (g, (_, members)) in search.groups.iter().enumerate() {
        if search.required[g] {
            queue.push_back(&cands[members[0]]);
        }
    }
    while let Some(c) = queue.pop_front() {
        if visited.insert(&c.manifest.id, c).is_some() {
            continue;
        }
        for (key, req) in &c.manifest.depends {
            if is_own_key(c, key) {
                continue;
            }
            match unmet_message(cands, c, key, req) {
                Some(message) => {
                    problems.insert(Problem {
                        subject: c.manifest.id.clone(),
                        message,
                    });
                }
                None => {
                    if let Some(p) = providers(cands, key).into_iter().find(|p| req_ok(req, p)) {
                        queue.push_back(p);
                    }
                }
            }
        }
    }
    let set: Vec<&Candidate> = visited.values().copied().collect();
    for a in &set {
        for b in &set {
            if a.manifest.id != b.manifest.id && breaks_hit(a, b) {
                let req = keys(b)
                    .find_map(|k| a.manifest.breaks.get(k))
                    .expect("breaks_hit");
                problems.insert(Problem {
                    subject: a.manifest.id.clone(),
                    message: format!(
                        "{} breaks \"{}\" {req}, but {} is installed ({})",
                        describe(a),
                        b.manifest.id,
                        b.manifest.version,
                        b.origin
                    ),
                });
            }
        }
    }
    if problems.is_empty() {
        let ids: Vec<String> = visited.keys().map(|k| k.to_string()).collect();
        let subject = visited.keys().next().map(|k| (*k).clone());
        if let Some(subject) = subject {
            problems.insert(Problem {
                subject,
                message: format!(
                    "no consistent set of mods exists (conflicting requirements among: {})",
                    ids.join(", ")
                ),
            });
        }
    }
    problems.into_iter().collect()
}

/// Why an optional candidate could not be used.
fn explain(cands: &[Candidate], c: &Candidate) -> String {
    c.manifest
        .depends
        .iter()
        .filter(|(k, _)| !is_own_key(c, k))
        .find_map(|(k, r)| unmet_message(cands, c, k, r))
        .unwrap_or_else(|| format!("{} conflicts with another selected mod", describe(c)))
}

fn lookup_warnings(selected: &BTreeMap<&ModId, &Candidate>, active: &[&Candidate]) -> Vec<String> {
    let mut out = Vec::new();
    let check_map = |m: &DepMap, c: &Candidate, out: &mut Vec<String>, verb: &str| {
        for (key, req) in m {
            match selected.get(key) {
                None => out.push(format!(
                    "{} {verb} \"{key}\" {req}, but it is not installed",
                    describe(c)
                )),
                Some(p) if !req_ok(req, p) => out.push(format!(
                    "{} {verb} \"{key}\" {req}, but {} is installed",
                    describe(c),
                    p.manifest.version
                )),
                Some(_) => {}
            }
        }
    };
    for c in active {
        check_map(&c.manifest.recommends, c, &mut out, "recommends");
        for (key, req) in &c.manifest.conflicts {
            if is_own_key(c, key) {
                continue;
            }
            if let Some(p) = selected.get(key)
                && req_ok(req, p)
            {
                out.push(format!(
                    "{} conflicts with \"{key}\" {}",
                    describe(c),
                    p.manifest.version
                ));
            }
        }
    }
    out
}

/// Kahn's algorithm with ties by id; a cycle is broken at the smallest id and reported.
fn load_order(active: &[&Candidate], warnings: &mut Vec<String>) -> Vec<usize> {
    let mut owner: BTreeMap<&ModId, usize> = BTreeMap::new();
    for (i, c) in active.iter().enumerate() {
        for k in keys(c) {
            owner.insert(k, i);
        }
    }
    let mut indegree = vec![0usize; active.len()];
    let mut dependents: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); active.len()];
    for (i, c) in active.iter().enumerate() {
        let targets: BTreeSet<usize> = c
            .manifest
            .depends
            .keys()
            .filter_map(|k| owner.get(k).copied())
            .filter(|&t| t != i)
            .collect();
        indegree[i] = targets.len();
        for t in targets {
            dependents[t].insert(i);
        }
    }
    // `active` is sorted by id, so index order is id order.
    let mut ready: BTreeSet<usize> = (0..active.len()).filter(|&i| indegree[i] == 0).collect();
    let mut done = vec![false; active.len()];
    let mut order = Vec::with_capacity(active.len());
    while order.len() < active.len() {
        let next = match ready.pop_first() {
            Some(n) => n,
            None => {
                let remaining: Vec<usize> = (0..active.len()).filter(|&i| !done[i]).collect();
                let names: Vec<String> = remaining
                    .iter()
                    .map(|&i| active[i].manifest.id.to_string())
                    .collect();
                warnings.push(format!("dependency cycle between: {}", names.join(", ")));
                remaining[0]
            }
        };
        done[next] = true;
        order.push(next);
        for &d in &dependents[next] {
            if done[d] {
                continue;
            }
            indegree[d] -= 1;
            if indegree[d] == 0 {
                ready.insert(d);
            }
        }
        ready.remove(&next);
    }
    order
}

/// Chooses the mods to load.
///
/// Candidates are sorted first, so the result does not depend on input order. See the crate docs
/// for the algorithm.
pub fn resolve(
    candidates: Vec<Candidate>,
    side: Side,
    builtins: &Builtins,
) -> Result<Resolution, ResolutionError> {
    resolve_with_limit(candidates, side, builtins, STEP_LIMIT)
}

#[doc(hidden)]
pub fn resolve_with_limit(
    candidates: Vec<Candidate>,
    side: Side,
    builtins: &Builtins,
    limit: usize,
) -> Result<Resolution, ResolutionError> {
    let mut all = candidates;
    all.extend(builtins.manifests().into_iter().map(|manifest| Candidate {
        manifest,
        origin: Origin::Builtin,
        condition: LoadCondition::Always,
    }));
    all.sort_by(|a, b| {
        (
            &a.manifest.id,
            Reverse(&a.manifest.version),
            &a.origin,
            a.condition,
        )
            .cmp(&(
                &b.manifest.id,
                Reverse(&b.manifest.version),
                &b.origin,
                b.condition,
            ))
    });

    let mut inactive: Vec<(ModId, Reason)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    let mut kept: Vec<Candidate> = Vec::new();
    for c in all {
        if !c.manifest.environment.allows(side) {
            inactive.push((
                c.manifest.id.clone(),
                Reason::Environment(c.manifest.environment),
            ));
        } else {
            kept.push(c);
        }
    }

    let mut cands: Vec<Candidate> = Vec::new();
    let mut always_kept: BTreeMap<ModId, usize> = BTreeMap::new();
    for c in kept {
        if c.condition == LoadCondition::Always && c.origin != Origin::Builtin {
            if let Some(&first) = always_kept.get(&c.manifest.id) {
                let k = &cands[first];
                warnings.push(format!(
                    "duplicate mod \"{}\": keeping {} ({}), discarding {} ({})",
                    c.manifest.id, k.manifest.version, k.origin, c.manifest.version, c.origin
                ));
                inactive.push((
                    c.manifest.id.clone(),
                    Reason::Duplicate {
                        kept: k.origin.clone(),
                    },
                ));
                continue;
            }
            always_kept.insert(c.manifest.id.clone(), cands.len());
        }
        cands.push(c);
    }

    let mut search = Search::new(&cands, limit);
    let solved = match search.dfs(0) {
        Ok(s) => s,
        Err(()) => {
            let subject = cands.first().map(|c| c.manifest.id.clone());
            return Err(ResolutionError {
                problems: subject
                    .map(|subject| Problem {
                        subject,
                        message: format!("dependency resolution exceeded {limit} steps"),
                    })
                    .into_iter()
                    .collect(),
            });
        }
    };
    if !solved {
        return Err(ResolutionError {
            problems: diagnose(&cands, &search),
        });
    }

    let chosen: BTreeSet<usize> = search.chosen.iter().copied().collect();
    let chosen_ids: BTreeSet<&ModId> = chosen.iter().map(|&i| &cands[i].manifest.id).collect();
    for (i, c) in cands.iter().enumerate() {
        if chosen.contains(&i) {
            continue;
        }
        let reason = if chosen_ids.contains(&c.manifest.id) {
            Reason::NotSelected
        } else {
            Reason::Unsatisfied(explain(&cands, c))
        };
        inactive.push((c.manifest.id.clone(), reason));
    }

    let mut selected: BTreeMap<&ModId, &Candidate> = BTreeMap::new();
    for &i in &chosen {
        for k in keys(&cands[i]) {
            selected.insert(k, &cands[i]);
        }
    }
    let real: Vec<&Candidate> = chosen
        .iter()
        .map(|&i| &cands[i])
        .filter(|c| c.origin != Origin::Builtin)
        .collect();
    warnings.extend(lookup_warnings(&selected, &real));
    let order = load_order(&real, &mut warnings);
    let active = order
        .into_iter()
        .map(|i| ResolvedMod {
            manifest: real[i].manifest.clone(),
            origin: real[i].origin.clone(),
        })
        .collect();
    Ok(Resolution {
        active,
        inactive,
        warnings,
    })
}
