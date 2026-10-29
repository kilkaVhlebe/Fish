//! Command trees registered by mods: parsing, permissions and execution.
//!
//! Mods register Brigadier-style trees through the `fish:commands` service (see
//! [`fish_abi::commands`]). The loader owns the trees: it parses a typed line against them, checks
//! the source's permission level, converts the arguments and calls the mod's executor.

use std::ffi::c_void;
use std::sync::Arc;

use fish_abi::commands::{
    CommandArgV1, CommandCtxV1, CommandExecFn, KIND_CLIENT, KIND_SERVER, arg,
};
use fish_abi::{DropUserFn, FishStatus, FishStr};
use fish_hooks::{CommandOutcome, CommandSource};
use serde::Deserialize;

/// Deepest tree accepted.
const MAX_DEPTH: usize = 64;
/// Most nodes in one registered tree.
const MAX_NODES: usize = 4096;

struct UserPtr(*mut c_void);

// SAFETY: the pointer is only handed back to the mod's own callbacks, which are `Send + Sync`
// by the `fish-api` contract (listeners and executors are `Fn + Send + Sync`).
unsafe impl Send for UserPtr {}
// SAFETY: see above.
unsafe impl Sync for UserPtr {}

/// The mod-side executor of a registered tree.
pub(crate) struct Executor {
    exec: CommandExecFn,
    user: UserPtr,
    drop_user: Option<DropUserFn>,
    pub(crate) owner: u64,
}

impl Drop for Executor {
    fn drop(&mut self) {
        if let Some(f) = self.drop_user {
            // SAFETY: `user` was handed over with its destructor by `register`; this runs once.
            unsafe { f(self.user.0) };
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ArgType {
    Word,
    Str,
    Greedy,
    Integer(i32, i32),
    Double(f64, f64),
    Bool,
    Player,
    Entity,
    BlockPos,
}

impl ArgType {
    fn wire(&self) -> u8 {
        match self {
            ArgType::Word => arg::WORD,
            ArgType::Str => arg::STRING,
            ArgType::Greedy => arg::GREEDY,
            ArgType::Integer(..) => arg::INTEGER,
            ArgType::Double(..) => arg::DOUBLE,
            ArgType::Bool => arg::BOOL,
            ArgType::Player => arg::PLAYER,
            ArgType::Entity => arg::ENTITY,
            ArgType::BlockPos => arg::BLOCK_POS,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum NodeKind {
    Literal,
    Argument(ArgType),
}

#[derive(Debug, Clone)]
struct Node {
    name: String,
    kind: NodeKind,
    level: Option<u8>,
    exec: Option<u32>,
    redirect: Option<String>,
    suggests: Vec<String>,
    children: Vec<Node>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    level: Option<u8>,
    exec: Option<u32>,
    redirect: Option<String>,
    #[serde(default)]
    suggests: Vec<String>,
    #[serde(default)]
    children: Vec<Spec>,
    min: Option<f64>,
    max: Option<f64>,
}

fn valid_literal(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'+' | b'-'))
}

fn valid_arg_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && s.len() <= 64
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn build(spec: Spec, depth: usize, count: &mut usize) -> Result<Node, String> {
    *count += 1;
    if *count > MAX_NODES {
        return Err(format!("a command tree may have at most {MAX_NODES} nodes"));
    }
    if depth > MAX_DEPTH {
        return Err(format!(
            "a command tree may be at most {MAX_DEPTH} levels deep"
        ));
    }
    let kind = match spec.ty.as_str() {
        "literal" => {
            if !valid_literal(&spec.name) {
                return Err(format!("invalid literal {:?}", spec.name));
            }
            NodeKind::Literal
        }
        other => {
            if !valid_arg_name(&spec.name) {
                return Err(format!("invalid argument name {:?}", spec.name));
            }
            NodeKind::Argument(match other {
                "word" => ArgType::Word,
                "string" => ArgType::Str,
                "greedy" => ArgType::Greedy,
                "integer" => {
                    let lo = spec.min.unwrap_or(f64::from(i32::MIN));
                    let hi = spec.max.unwrap_or(f64::from(i32::MAX));
                    ArgType::Integer(
                        lo.max(f64::from(i32::MIN)) as i32,
                        hi.min(f64::from(i32::MAX)) as i32,
                    )
                }
                "double" => ArgType::Double(
                    spec.min.unwrap_or(f64::NEG_INFINITY),
                    spec.max.unwrap_or(f64::INFINITY),
                ),
                "bool" => ArgType::Bool,
                "player" => ArgType::Player,
                "entity" => ArgType::Entity,
                "block_pos" => ArgType::BlockPos,
                _ => return Err(format!("unknown node type {other:?}")),
            })
        }
    };
    if spec.level.is_some_and(|l| l > 4) {
        return Err("permission levels are 0..=4".to_string());
    }
    let mut children = Vec::with_capacity(spec.children.len());
    for c in spec.children {
        let child = build(c, depth + 1, count)?;
        if children.iter().any(|o: &Node| o.name == child.name) {
            return Err(format!(
                "{:?} appears twice under {:?}",
                child.name, spec.name
            ));
        }
        children.push(child);
    }
    if let Some(i) = children
        .iter()
        .position(|c| matches!(c.kind, NodeKind::Argument(ArgType::Greedy)))
        && !children[i].children.is_empty()
    {
        return Err("a greedy argument must be the last node".to_string());
    }
    Ok(Node {
        name: spec.name,
        kind,
        level: spec.level,
        exec: spec.exec,
        redirect: spec.redirect,
        suggests: spec.suggests,
        children,
    })
}

struct Root {
    node: Node,
    executor: Arc<Executor>,
}

/// The registered trees of one side.
#[derive(Default)]
pub(crate) struct Commands {
    server: Vec<Root>,
    client: Vec<Root>,
}

/// What registering a tree decided.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Registered {
    /// Installed.
    Added,
    /// Another mod owns the root name; this tree was ignored (the message says who).
    Duplicate(u64),
}

impl Commands {
    /// Parses and installs a tree. `owner_of` names the mod that owns a token (for messages).
    pub(crate) fn register(
        &mut self,
        kind: u8,
        tree: &str,
        executor: Executor,
    ) -> Result<Registered, String> {
        let spec: Spec =
            serde_json::from_str(tree).map_err(|e| format!("invalid command tree: {e}"))?;
        if spec.ty != "literal" {
            return Err("the root of a command tree must be a literal".to_string());
        }
        let mut count = 0;
        let node = build(spec, 0, &mut count)?;
        let roots = match kind {
            KIND_SERVER => &mut self.server,
            KIND_CLIENT => &mut self.client,
            _ => return Err(format!("unknown command tree kind {kind}")),
        };
        if let Some(existing) = roots.iter().find(|r| r.node.name == node.name) {
            return Ok(Registered::Duplicate(existing.executor.owner));
        }
        roots.push(Root {
            node,
            executor: Arc::new(executor),
        });
        Ok(Registered::Added)
    }

    /// Root names of one kind, in registration order.
    pub(crate) fn roots(&self, kind: u8) -> Vec<String> {
        self.of(kind).iter().map(|r| r.node.name.clone()).collect()
    }

    fn of(&self, kind: u8) -> &[Root] {
        if kind == KIND_CLIENT {
            &self.client
        } else {
            &self.server
        }
    }

    /// Drops every tree (before the mod libraries unload).
    pub(crate) fn clear(&mut self) {
        self.server.clear();
        self.client.clear();
    }

    /// True when the first word of `line` is a registered root of `kind`.
    pub(crate) fn owns(&self, kind: u8, line: &str) -> bool {
        let word = line.split(' ').next().unwrap_or("");
        self.of(kind).iter().any(|r| r.node.name == word)
    }

    /// Parses `line` for `source`; the executor and arguments are ready to run.
    pub(crate) fn parse(
        &self,
        kind: u8,
        source: &CommandSource<'_>,
        line: &str,
    ) -> Result<Parsed, String> {
        let roots = self.of(kind);
        let word_end = line.find(' ').unwrap_or(line.len());
        let Some(root) = roots.iter().find(|r| r.node.name == line[..word_end]) else {
            return Err(unknown(line, 0));
        };
        let mut node = &root.node;
        let mut level = node.level.unwrap_or(0);
        if source.level < level {
            return Err(unknown(line, 0));
        }
        let mut cursor = word_end;
        let mut args: Vec<(String, u8, String)> = Vec::new();
        loop {
            // A redirect continues at another root's children.
            let candidates: &[Node] = match &node.redirect {
                Some(target) => match roots.iter().find(|r| &r.node.name == target) {
                    Some(r) => &r.node.children,
                    None => &[],
                },
                None => &node.children,
            };
            let at_end = cursor >= line.len();
            if at_end {
                return match node.exec {
                    Some(index) => Ok(Parsed {
                        executor: root.executor.clone(),
                        index,
                        args,
                    }),
                    None => Err(unknown(line, line.len())),
                };
            }
            // exactly one separating space
            if !line[cursor..].starts_with(' ') {
                return Err(unknown(line, cursor));
            }
            cursor += 1;
            if cursor >= line.len() && node.exec.is_some() {
                // trailing space: treated like the end of the command
                return Ok(Parsed {
                    executor: root.executor.clone(),
                    index: node.exec.unwrap_or(0),
                    args,
                });
            }
            let mut matched: Option<Matched<'_>> = None;
            let mut first_error: Option<String> = None;
            for child in candidates {
                let allowed = child.level.unwrap_or(level);
                if source.level < allowed {
                    continue;
                }
                match &child.kind {
                    NodeKind::Literal => {
                        let end = cursor + line[cursor..].find(' ').unwrap_or(line.len() - cursor);
                        if line[cursor..end] == child.name {
                            matched = Some((child, end, None));
                            break;
                        }
                    }
                    NodeKind::Argument(ty) => match read_arg(ty, line, cursor, source) {
                        Ok((end, value)) => {
                            matched = Some((child, end, Some((ty.wire(), value))));
                            break;
                        }
                        Err(e) => {
                            first_error.get_or_insert(e);
                        }
                    },
                }
            }
            let Some((child, end, value)) = matched else {
                return Err(match first_error {
                    Some(e) => format!("{e} at position {cursor}: {}<--[HERE]", &line[..cursor]),
                    None => unknown(line, cursor),
                });
            };
            if let Some((wire, v)) = value {
                args.push((child.name.clone(), wire, v));
            }
            level = child.level.unwrap_or(level);
            node = child;
            cursor = end;
        }
    }

    /// The suggestions of the argument that would come next after `line` (static lists only).
    pub(crate) fn suggestions(
        &self,
        kind: u8,
        source: &CommandSource<'_>,
        line: &str,
    ) -> Vec<String> {
        let prefix_end = line.rfind(' ').map_or(0, |i| i + 1);
        let (head, partial) = line.split_at(prefix_end);
        let head = head.trim_end();
        if head.is_empty() {
            return self
                .of(kind)
                .iter()
                .filter(|r| {
                    r.node.name.starts_with(partial) && source.level >= r.node.level.unwrap_or(0)
                })
                .map(|r| r.node.name.clone())
                .collect();
        }
        let roots = self.of(kind);
        let Some(root) = roots
            .iter()
            .find(|r| head.split(' ').next() == Some(r.node.name.as_str()))
        else {
            return Vec::new();
        };
        // follow literals and, for arguments, any child that accepts the word
        let mut node = &root.node;
        for word in head.split(' ').skip(1) {
            let next = node.children.iter().find(|c| match &c.kind {
                NodeKind::Literal => c.name == word,
                NodeKind::Argument(_) => true,
            });
            match next {
                Some(n) => node = n,
                None => return Vec::new(),
            }
        }
        let mut out = Vec::new();
        for c in &node.children {
            if source.level < c.level.unwrap_or(0) {
                continue;
            }
            match &c.kind {
                NodeKind::Literal if c.name.starts_with(partial) => out.push(c.name.clone()),
                NodeKind::Argument(_) => out.extend(
                    c.suggests
                        .iter()
                        .filter(|s| s.starts_with(partial))
                        .cloned(),
                ),
                NodeKind::Literal => {}
            }
        }
        out
    }
}

/// A matched child: the node, where it ends, and the argument value it parsed.
type Matched<'a> = (&'a Node, usize, Option<(u8, String)>);

fn unknown(line: &str, at: usize) -> String {
    let at = at.min(line.len());
    format!("Unknown or incomplete command: {}<--[HERE]", &line[..at])
}

/// A parsed line: what to call and with which arguments.
pub(crate) struct Parsed {
    pub(crate) executor: Arc<Executor>,
    index: u32,
    args: Vec<(String, u8, String)>,
}

fn word_end(line: &str, from: usize) -> usize {
    from + line[from..].find(' ').unwrap_or(line.len() - from)
}

fn read_arg(
    ty: &ArgType,
    line: &str,
    at: usize,
    source: &CommandSource<'_>,
) -> Result<(usize, String), String> {
    let rest = &line[at..];
    if rest.is_empty() {
        return Err("Expected an argument".to_string());
    }
    match ty {
        ArgType::Word => {
            let end = word_end(line, at);
            Ok((end, line[at..end].to_string()))
        }
        ArgType::Greedy => Ok((line.len(), rest.to_string())),
        ArgType::Str => {
            if let Some(body) = rest.strip_prefix('"') {
                let mut out = String::new();
                let mut chars = body.char_indices();
                while let Some((i, c)) = chars.next() {
                    match c {
                        '\\' => match chars.next() {
                            Some((_, n @ ('"' | '\\'))) => out.push(n),
                            _ => return Err("Invalid escape in a quoted string".to_string()),
                        },
                        '"' => return Ok((at + 1 + i + 1, out)),
                        _ => out.push(c),
                    }
                }
                Err("Unclosed quoted string".to_string())
            } else {
                let end = word_end(line, at);
                Ok((end, line[at..end].to_string()))
            }
        }
        ArgType::Integer(lo, hi) => {
            let end = word_end(line, at);
            let word = &line[at..end];
            let n: i32 = word
                .parse()
                .map_err(|_| format!("Expected integer, found {word:?}"))?;
            if n < *lo {
                return Err(format!("Integer must not be less than {lo}, found {n}"));
            }
            if n > *hi {
                return Err(format!("Integer must not be more than {hi}, found {n}"));
            }
            Ok((end, n.to_string()))
        }
        ArgType::Double(lo, hi) => {
            let end = word_end(line, at);
            let word = &line[at..end];
            let n: f64 = word
                .parse()
                .ok()
                .filter(|n: &f64| n.is_finite())
                .ok_or_else(|| format!("Expected double, found {word:?}"))?;
            if n < *lo {
                return Err(format!("Double must not be less than {lo}, found {n}"));
            }
            if n > *hi {
                return Err(format!("Double must not be more than {hi}, found {n}"));
            }
            Ok((end, n.to_string()))
        }
        ArgType::Bool => {
            let end = word_end(line, at);
            match &line[at..end] {
                w @ ("true" | "false") => Ok((end, w.to_string())),
                w => Err(format!("Expected bool, found {w:?}")),
            }
        }
        ArgType::Player => {
            let end = word_end(line, at);
            let w = &line[at..end];
            if (1..=16).contains(&w.len())
                && w.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                Ok((end, w.to_string()))
            } else {
                Err(format!("Expected a player name, found {w:?}"))
            }
        }
        ArgType::Entity => {
            let end = word_end(line, at);
            Ok((end, line[at..end].to_string()))
        }
        ArgType::BlockPos => {
            let mut cursor = at;
            let mut parts = Vec::with_capacity(3);
            for axis in 0..3 {
                if axis > 0 {
                    if !line[cursor..].starts_with(' ') {
                        return Err("Expected three coordinates".to_string());
                    }
                    cursor += 1;
                }
                let end = word_end(line, cursor);
                let w = &line[cursor..end];
                if w.is_empty() {
                    return Err("Expected three coordinates".to_string());
                }
                let value = if let Some(offset) = w.strip_prefix('~') {
                    let base = source
                        .pos
                        .ok_or_else(|| "Relative coordinates need a position".to_string())?[axis];
                    let delta: f64 = if offset.is_empty() {
                        0.0
                    } else {
                        offset
                            .parse()
                            .map_err(|_| format!("Expected a coordinate, found {w:?}"))?
                    };
                    (base + delta).floor() as i64
                } else {
                    w.parse::<i64>()
                        .map_err(|_| format!("Expected a coordinate, found {w:?}"))?
                };
                if value.unsigned_abs() > 30_000_000 {
                    return Err(format!("Coordinate {value} is out of the world"));
                }
                parts.push(value);
                cursor = end;
            }
            Ok((cursor, format!("{} {} {}", parts[0], parts[1], parts[2])))
        }
    }
}

struct Output {
    lines: Vec<(String, bool)>,
}

unsafe extern "C" fn collect(out: *mut c_void, text: FishStr, error: u8) {
    // SAFETY: `out` is the `Output` of `run`; the text is valid UTF-8 or skipped.
    unsafe {
        if let Ok(s) = text.as_str() {
            (*out.cast::<Output>())
                .lines
                .push((s.to_string(), error != 0));
        }
    }
}

/// What running a parsed command produced.
pub(crate) struct Ran {
    pub(crate) outcome: CommandOutcome,
    /// The executor panicked or returned an unexpected status: the owner should be stopped.
    pub(crate) broken: Option<String>,
}

impl Parsed {
    /// Calls the executor.
    pub(crate) fn run(&self, source: &CommandSource<'_>) -> Ran {
        let names: Vec<String> = self.args.iter().map(|a| a.0.clone()).collect();
        let values: Vec<String> = self.args.iter().map(|a| a.2.clone()).collect();
        let wire: Vec<CommandArgV1> = self
            .args
            .iter()
            .enumerate()
            .map(|(i, a)| CommandArgV1 {
                name: FishStr::new(&names[i]),
                kind: a.1,
                value: FishStr::new(&values[i]),
            })
            .collect();
        let mut out = Output { lines: Vec::new() };
        let mut result = 0i32;
        let ctx = CommandCtxV1 {
            size: size_of::<CommandCtxV1>() as u32,
            is_player: u8::from(source.is_player),
            level: source.level,
            reserved: [0; 2],
            source_name: FishStr::new(source.name),
            source_uuid: source.uuid,
            pos: source.pos.unwrap_or_default(),
            has_pos: u8::from(source.pos.is_some()),
            reserved2: [0; 7],
            args: wire.as_ptr(),
            arg_count: wire.len(),
            out: (&raw mut out).cast(),
            output: collect,
            result: &raw mut result,
        };
        let exec = self.executor.exec;
        let user = self.executor.user.0;
        let index = self.index;
        // SAFETY: the executor and `user` come from one registration; `ctx` and what it points to
        // outlive the call. Executors are `extern "C"`: a mod's trampoline catches its own panics.
        let status = unsafe { exec(user, index, &ctx) };
        let mut broken = None;
        let mut messages: Vec<String> = Vec::new();
        let mut failed = false;
        for (text, error) in out.lines {
            failed |= error;
            messages.push(text);
        }
        if status != FishStatus::OK {
            failed = true;
            broken = Some(format!("a command executor returned {}", status.name()));
            messages.push("The command failed internally".to_string());
        }
        Ran {
            outcome: CommandOutcome {
                handled: true,
                ok: !failed,
                result: if failed { 0 } else { result },
                messages,
            },
            broken,
        }
    }
}

/// Builds an [`Executor`].
pub(crate) fn executor(
    exec: CommandExecFn,
    user: *mut c_void,
    drop_user: Option<DropUserFn>,
    owner: u64,
) -> Executor {
    Executor {
        exec,
        user: UserPtr(user),
        drop_user,
        owner,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn echo(
        user: *mut c_void,
        index: u32,
        ctx: *const CommandCtxV1,
    ) -> FishStatus {
        // SAFETY: the test passes a valid context and no user data.
        let ctx = unsafe { &*ctx };
        let _ = user;
        let mut text = format!("#{index}");
        for i in 0..ctx.arg_count {
            // SAFETY: `args` has `arg_count` entries.
            let a = unsafe { &*ctx.args.add(i) };
            // SAFETY: valid UTF-8 from the loader.
            let (n, v) = unsafe { (a.name.as_str().unwrap(), a.value.as_str().unwrap()) };
            text.push_str(&format!(" {n}={v}"));
        }
        // SAFETY: output and out come together.
        unsafe { (ctx.output)(ctx.out, FishStr::new(&text), 0) };
        // SAFETY: result is valid.
        unsafe { *ctx.result = 7 };
        FishStatus::OK
    }

    unsafe extern "C" fn broken(_: *mut c_void, _: u32, _: *const CommandCtxV1) -> FishStatus {
        FishStatus::PANICKED
    }

    const TREE: &str = r#"{"name":"t","type":"literal","exec":0,"children":[
        {"name":"n","type":"integer","min":1,"max":9,"exec":1},
        {"name":"word","type":"literal","children":[{"name":"w","type":"word","exec":2}]},
        {"name":"q","type":"literal","children":[{"name":"s","type":"string","exec":3}]},
        {"name":"d","type":"literal","children":[{"name":"x","type":"double","min":0.5,"exec":4}]},
        {"name":"b","type":"literal","children":[{"name":"flag","type":"bool","exec":5}]},
        {"name":"p","type":"literal","children":[{"name":"who","type":"player","exec":6}]},
        {"name":"e","type":"literal","children":[{"name":"target","type":"entity","exec":7}]},
        {"name":"pos","type":"literal","children":[{"name":"at","type":"block_pos","exec":8}]},
        {"name":"g","type":"literal","children":[{"name":"rest","type":"greedy","exec":9}]},
        {"name":"sudo","type":"literal","level":4,"children":[{"name":"x","type":"word","exec":10}]},
        {"name":"again","type":"literal","redirect":"t"}
    ]}"#;

    fn source(level: u8) -> CommandSource<'static> {
        CommandSource {
            name: "Alex",
            uuid: [1; 16],
            level,
            is_player: true,
            pos: Some([10.5, 64.0, -3.5]),
        }
    }

    fn commands() -> Commands {
        let mut c = Commands::default();
        assert_eq!(
            c.register(
                KIND_SERVER,
                TREE,
                executor(echo, std::ptr::null_mut(), None, 1)
            ),
            Ok(Registered::Added)
        );
        c
    }

    fn run(c: &Commands, level: u8, line: &str) -> Result<String, String> {
        let p = c.parse(KIND_SERVER, &source(level), line)?;
        let ran = p.run(&source(level));
        Ok(ran.outcome.messages.join("|"))
    }

    #[test]
    fn arguments_are_validated_and_delivered() {
        let c = commands();
        assert_eq!(run(&c, 0, "t"), Ok("#0".into()));
        assert_eq!(run(&c, 0, "t 5"), Ok("#1 n=5".into()));
        assert_eq!(run(&c, 0, "t word hi"), Ok("#2 w=hi".into()));
        assert_eq!(run(&c, 0, "t q plain"), Ok("#3 s=plain".into()));
        assert_eq!(
            run(&c, 0, r#"t q "a b \"c\" \\""#),
            Ok(r#"#3 s=a b "c" \"#.into())
        );
        assert_eq!(run(&c, 0, "t d 1.5"), Ok("#4 x=1.5".into()));
        assert_eq!(run(&c, 0, "t b true"), Ok("#5 flag=true".into()));
        assert_eq!(run(&c, 0, "t p Steve_1"), Ok("#6 who=Steve_1".into()));
        assert_eq!(
            run(&c, 0, "t e @a[distance=..5]"),
            Ok("#7 target=@a[distance=..5]".into())
        );
        assert_eq!(run(&c, 0, "t pos 1 2 3"), Ok("#8 at=1 2 3".into()));
        assert_eq!(run(&c, 0, "t pos ~ ~1.5 ~-4"), Ok("#8 at=10 65 -8".into()));
        assert_eq!(
            run(&c, 0, "t g any thing  at all"),
            Ok("#9 rest=any thing  at all".into())
        );
    }

    #[test]
    fn bad_arguments_name_the_problem_and_where() {
        let c = commands();
        let err = |line: &str| run(&c, 0, line).unwrap_err();
        assert!(
            err("t 0").contains("Integer must not be less than 1, found 0 at position 2"),
            "{}",
            err("t 0")
        );
        assert!(err("t 10").contains("must not be more than 9"));
        assert!(err("t zz").contains("Expected integer"));
        assert!(err("t d 0.1").contains("Double must not be less than 0.5"));
        assert!(err("t d NaN").contains("Expected double"));
        assert!(err("t b maybe").contains("Expected bool"));
        assert!(err("t p way_too_long_for_a_name").contains("Expected a player name"));
        assert!(err("t pos 1 2").contains("Expected three coordinates"));
        assert!(err("t pos 1 x 3").contains("Expected a coordinate"));
        assert!(err("t pos 99999999 0 0").contains("out of the world"));
        assert!(err(r#"t q "open"#).contains("Unclosed quoted string"));
        assert!(err(r#"t q "bad\n""#).contains("Invalid escape"));
        assert_eq!(err("nope"), "Unknown or incomplete command: <--[HERE]");
        assert_eq!(
            err("t word"),
            "Unknown or incomplete command: t word<--[HERE]"
        );
        assert_eq!(err("tt"), "Unknown or incomplete command: <--[HERE]");
    }

    #[test]
    fn permissions_hide_nodes_below_the_level() {
        let c = commands();
        assert_eq!(run(&c, 4, "t sudo x"), Ok("#10 x=x".into()));
        assert!(run(&c, 3, "t sudo x").is_err());
        assert!(run(&c, 0, "t sudo x").is_err(), "hidden nodes never match");
    }

    #[test]
    fn redirects_continue_at_the_target_root() {
        let c = commands();
        assert_eq!(run(&c, 0, "t again 4"), Ok("#1 n=4".into()));
        assert_eq!(run(&c, 0, "t again again word hi"), Ok("#2 w=hi".into()));
        assert!(
            run(&c, 0, "t again").is_err(),
            "a redirect node does not execute"
        );
    }

    #[test]
    fn registration_rejects_bad_trees_and_duplicates() {
        let mut c = commands();
        let exec = || executor(echo, std::ptr::null_mut(), None, 2);
        let bad = |c: &mut Commands, tree: &str| c.register(KIND_SERVER, tree, exec()).unwrap_err();
        assert!(bad(&mut c, "not json").contains("invalid command tree"));
        assert!(bad(&mut c, r#"{"name":"x","type":"word"}"#).contains("must be a literal"));
        assert!(bad(&mut c, r#"{"name":"bad name","type":"literal"}"#).contains("invalid literal"));
        assert!(bad(&mut c, r#"{"name":"x","type":"literal","level":9}"#).contains("0..=4"));
        assert!(
            bad(
                &mut c,
                r#"{"name":"x","type":"literal","children":[{"name":"Up","type":"word"}]}"#
            )
            .contains("invalid argument name")
        );
        assert!(
            bad(
                &mut c,
                r#"{"name":"x","type":"literal","children":[{"name":"a","type":"mystery"}]}"#
            )
            .contains("unknown node type")
        );
        assert!(bad(&mut c, r#"{"name":"x","type":"literal","children":[{"name":"a","type":"literal"},{"name":"a","type":"literal"}]}"#).contains("appears twice"));
        assert!(bad(&mut c, r#"{"name":"x","type":"literal","children":[{"name":"g","type":"greedy","children":[{"name":"y","type":"literal"}]}]}"#).contains("greedy"));
        assert!(
            c.register(9, TREE, exec())
                .unwrap_err()
                .contains("unknown command tree kind")
        );
        // the first registration keeps the name; the same name on the other side is separate
        assert_eq!(
            c.register(KIND_SERVER, TREE, exec()),
            Ok(Registered::Duplicate(1))
        );
        assert_eq!(c.register(KIND_CLIENT, TREE, exec()), Ok(Registered::Added));
        assert_eq!(c.roots(KIND_SERVER), ["t"]);
        assert!(c.owns(KIND_CLIENT, "t 5") && !c.owns(KIND_CLIENT, "tt"));
        c.clear();
        assert!(c.roots(KIND_CLIENT).is_empty());
    }

    #[test]
    fn trees_are_limited_in_depth_and_size() {
        let mut c = Commands::default();
        let exec = || executor(echo, std::ptr::null_mut(), None, 1);
        let mut deep = String::from(r#"{"name":"x","type":"literal"}"#);
        for i in 0..70 {
            deep = format!(r#"{{"name":"n{i}","type":"literal","children":[{deep}]}}"#);
        }
        // serde_json refuses nesting past 128 first; the explicit limit is the backstop
        let deep_err = c.register(KIND_SERVER, &deep, exec()).unwrap_err();
        assert!(
            deep_err.contains("levels deep") || deep_err.contains("recursion limit"),
            "{deep_err}"
        );
        let wide: Vec<String> = (0..4100)
            .map(|i| format!(r#"{{"name":"n{i}","type":"literal"}}"#))
            .collect();
        let wide = format!(
            r#"{{"name":"w","type":"literal","children":[{}]}}"#,
            wide.join(",")
        );
        assert!(
            c.register(KIND_SERVER, &wide, exec())
                .unwrap_err()
                .contains("at most 4096 nodes")
        );
    }

    #[test]
    fn suggestions_list_literals_and_fixed_values() {
        let mut c = Commands::default();
        let tree = r#"{"name":"m","type":"literal","children":[
            {"name":"set","type":"literal","children":[{"name":"mode","type":"word","suggests":["fast","slow","fixed"],"exec":0}]},
            {"name":"secret","type":"literal","level":4}]}"#;
        c.register(
            KIND_SERVER,
            tree,
            executor(echo, std::ptr::null_mut(), None, 1),
        )
        .unwrap();
        let s = |level, line: &str| {
            let mut v = c.suggestions(KIND_SERVER, &source(level), line);
            v.sort();
            v
        };
        assert_eq!(s(0, "m"), ["m"]);
        assert_eq!(s(0, ""), ["m"]);
        assert_eq!(s(0, "m "), ["set"]);
        assert_eq!(s(4, "m "), ["secret", "set"]);
        assert_eq!(s(0, "m s"), ["set"]);
        assert_eq!(s(0, "m set f"), ["fast", "fixed"]);
        assert!(s(0, "zz ").is_empty() && s(0, "m nope ").is_empty());
    }

    #[test]
    fn an_executor_that_reports_a_panic_is_reported_as_broken() {
        let mut c = Commands::default();
        c.register(
            KIND_SERVER,
            r#"{"name":"boom","type":"literal","exec":0}"#,
            executor(broken, std::ptr::null_mut(), None, 3),
        )
        .unwrap();
        let ran = c
            .parse(KIND_SERVER, &source(0), "boom")
            .unwrap()
            .run(&source(0));
        assert!(
            ran.broken
                .is_some_and(|m| m.contains("PANICKED") || m.contains("panick"))
        );
        assert!(ran.outcome.handled && !ran.outcome.ok);
    }
}
