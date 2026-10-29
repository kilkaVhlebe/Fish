//! Brigadier-style commands (Fabric `CommandRegistrationCallback`).
//!
//! ```ignore
//! command::REGISTER.register(|ctx| {
//!     ctx.register(
//!         literal("fishdemo").requires(2).then(
//!             argument("count", ArgKind::Integer { min: 1, max: 10 }).executes(|c| {
//!                 c.feedback(&format!("count = {}", c.get_integer("count")?));
//!                 Ok(1)
//!             }),
//!         ),
//!     )
//! })?;
//! ```
//!
//! The tree is built in the mod, registered once, and parsed and permission-checked by the host.
//! Vanilla command names win; if two mods register the same root, the first keeps it and the second
//! gets a warning in the log. Argument values are validated by the host before your executor runs.

use std::ffi::c_void;
use std::fmt::{self, Write as _};

use fish_abi::commands::{
    CommandArgV1, CommandCtxV1, CommandRegisterPayload, CommandsServiceV1, KIND_CLIENT,
    KIND_SERVER, SERVICE_COMMANDS, arg,
};
use fish_abi::{FishStatus, FishStr};

use crate::client::service;
use crate::error::{Error, check};
use crate::event::{Combine, Event, Payload};
use crate::game::BlockPos;
use crate::host;

/// The kind of an argument node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArgKind {
    /// A single word.
    Word,
    /// A word or a `"quoted string"`.
    String,
    /// The rest of the line (must be the last node).
    Greedy,
    /// An integer in `min..=max`.
    Integer {
        /// Smallest accepted value.
        min: i32,
        /// Largest accepted value.
        max: i32,
    },
    /// A finite float in `min..=max`.
    Double {
        /// Smallest accepted value.
        min: f64,
        /// Largest accepted value.
        max: f64,
    },
    /// `true` or `false`.
    Bool,
    /// A player name.
    Player,
    /// An entity selector or name, passed on unresolved.
    Entity,
    /// Three block coordinates (`~` relative to the source's position).
    BlockPos,
}

/// Why a command failed; shown to the source in red.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError(pub String);

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CommandError {}

impl From<&str> for CommandError {
    fn from(s: &str) -> Self {
        CommandError(s.to_string())
    }
}

impl From<String> for CommandError {
    fn from(s: String) -> Self {
        CommandError(s)
    }
}

/// What an executor returns: the command's result value, or an error message.
pub type CommandResult = Result<i32, CommandError>;

type Executor = Box<dyn Fn(&CommandContext<'_>) -> CommandResult + Send + Sync + 'static>;

/// A node under construction.
pub struct CommandBuilder {
    name: String,
    arg: Option<ArgKind>,
    level: Option<u8>,
    redirect: Option<String>,
    suggests: Vec<String>,
    executor: Option<Executor>,
    children: Vec<CommandBuilder>,
}

/// A literal node: `literal("fishdemo")`.
pub fn literal(name: &str) -> CommandBuilder {
    CommandBuilder::new(name, None)
}

/// An argument node: `argument("count", ArgKind::Integer { min: 1, max: 10 })`.
pub fn argument(name: &str, kind: ArgKind) -> CommandBuilder {
    CommandBuilder::new(name, Some(kind))
}

impl CommandBuilder {
    fn new(name: &str, arg: Option<ArgKind>) -> CommandBuilder {
        CommandBuilder {
            name: name.to_string(),
            arg,
            level: None,
            redirect: None,
            suggests: Vec::new(),
            executor: None,
            children: Vec::new(),
        }
    }

    /// Requires permission level `level` (0 everyone, 2 game master, 4 owner); children inherit it.
    pub fn requires(mut self, level: u8) -> Self {
        self.level = Some(level);
        self
    }

    /// Adds a child node.
    pub fn then(mut self, child: CommandBuilder) -> Self {
        self.children.push(child);
        self
    }

    /// Runs `f` when the command ends at this node.
    pub fn executes<F>(mut self, f: F) -> Self
    where
        F: Fn(&CommandContext<'_>) -> CommandResult + Send + Sync + 'static,
    {
        self.executor = Some(Box::new(f));
        self
    }

    /// Continues parsing at the children of the root command `root` (registered by any mod).
    pub fn redirect(mut self, root: &str) -> Self {
        self.redirect = Some(root.to_string());
        self
    }

    /// Fixed completions for this argument.
    pub fn suggests(mut self, values: &[&str]) -> Self {
        self.suggests = values.iter().map(|s| (*s).to_string()).collect();
        self
    }

    fn write(self, out: &mut String, executors: &mut Vec<Executor>) {
        out.push_str("{\"name\":");
        json_str(out, &self.name);
        match self.arg {
            None => out.push_str(",\"type\":\"literal\""),
            Some(kind) => {
                let (ty, range) = match kind {
                    ArgKind::Word => ("word", None),
                    ArgKind::String => ("string", None),
                    ArgKind::Greedy => ("greedy", None),
                    ArgKind::Integer { min, max } => {
                        ("integer", Some((f64::from(min), f64::from(max))))
                    }
                    ArgKind::Double { min, max } => ("double", Some((min, max))),
                    ArgKind::Bool => ("bool", None),
                    ArgKind::Player => ("player", None),
                    ArgKind::Entity => ("entity", None),
                    ArgKind::BlockPos => ("block_pos", None),
                };
                let _ = write!(out, ",\"type\":\"{ty}\"");
                if let Some((lo, hi)) = range {
                    if lo.is_finite() {
                        let _ = write!(out, ",\"min\":{lo}");
                    }
                    if hi.is_finite() {
                        let _ = write!(out, ",\"max\":{hi}");
                    }
                }
            }
        }
        if let Some(level) = self.level {
            let _ = write!(out, ",\"level\":{level}");
        }
        if let Some(f) = self.executor {
            let _ = write!(out, ",\"exec\":{}", executors.len());
            executors.push(f);
        }
        if let Some(r) = &self.redirect {
            out.push_str(",\"redirect\":");
            json_str(out, r);
        }
        if !self.suggests.is_empty() {
            out.push_str(",\"suggests\":[");
            for (i, s) in self.suggests.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                json_str(out, s);
            }
            out.push(']');
        }
        if !self.children.is_empty() {
            out.push_str(",\"children\":[");
            for (i, c) in self.children.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                c.write(out, executors);
            }
            out.push(']');
        }
        out.push('}');
    }
}

fn json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The source and arguments of a running command.
pub struct CommandContext<'a>(&'a CommandCtxV1);

impl CommandContext<'_> {
    fn args(&self) -> &[CommandArgV1] {
        if self.0.args.is_null() {
            return &[];
        }
        // SAFETY: the host passes `arg_count` valid entries for the duration of the call.
        unsafe { std::slice::from_raw_parts(self.0.args, self.0.arg_count) }
    }

    fn find(&self, name: &str, kinds: &[u8]) -> Result<&str, CommandError> {
        let a = self
            .args()
            .iter()
            // SAFETY: the host passes valid UTF-8 for the call.
            .find(|a| unsafe { a.name.as_str() } == Ok(name))
            .ok_or_else(|| CommandError(format!("the command has no argument {name:?}")))?;
        if !kinds.contains(&a.kind) {
            return Err(CommandError(format!("argument {name:?} has another type")));
        }
        // SAFETY: as above.
        unsafe { a.value.as_str() }
            .map_err(|_| CommandError(format!("argument {name:?} is not text")))
    }

    /// The name of whoever ran the command (empty for the console).
    pub fn source_name(&self) -> &str {
        // SAFETY: valid UTF-8 for the call.
        unsafe { self.0.source_name.as_str() }.unwrap_or("")
    }

    /// The source's UUID, if it is a player.
    pub fn source_uuid(&self) -> Option<u128> {
        let n = u128::from_be_bytes(self.0.source_uuid);
        (n != 0).then_some(n)
    }

    /// The source's permission level.
    pub fn level(&self) -> u8 {
        self.0.level
    }

    /// True when a player ran the command.
    pub fn is_player(&self) -> bool {
        self.0.is_player != 0
    }

    /// Where the source stands, when known.
    pub fn position(&self) -> Option<[f64; 3]> {
        (self.0.has_pos != 0).then_some(self.0.pos)
    }

    /// A text-like argument (`word`, `string`, `greedy`, `player`, `entity`).
    pub fn get_string(&self, name: &str) -> Result<&str, CommandError> {
        self.find(
            name,
            &[
                arg::WORD,
                arg::STRING,
                arg::GREEDY,
                arg::PLAYER,
                arg::ENTITY,
            ],
        )
    }

    /// An integer argument.
    pub fn get_integer(&self, name: &str) -> Result<i32, CommandError> {
        self.find(name, &[arg::INTEGER])?
            .parse()
            .map_err(|_| CommandError(format!("argument {name:?} is not an integer")))
    }

    /// A double argument.
    pub fn get_double(&self, name: &str) -> Result<f64, CommandError> {
        self.find(name, &[arg::DOUBLE])?
            .parse()
            .map_err(|_| CommandError(format!("argument {name:?} is not a number")))
    }

    /// A bool argument.
    pub fn get_bool(&self, name: &str) -> Result<bool, CommandError> {
        Ok(self.find(name, &[arg::BOOL])? == "true")
    }

    /// A block position argument, resolved.
    pub fn get_block_pos(&self, name: &str) -> Result<BlockPos, CommandError> {
        let text = self.find(name, &[arg::BLOCK_POS])?;
        let mut parts = text.split(' ').map(|p| p.parse::<i32>());
        match (parts.next(), parts.next(), parts.next()) {
            (Some(Ok(x)), Some(Ok(y)), Some(Ok(z))) => Ok(BlockPos { x, y, z }),
            _ => Err(CommandError(format!(
                "argument {name:?} is not a block position"
            ))),
        }
    }

    /// Shows `text` to the source.
    pub fn feedback(&self, text: &str) {
        // SAFETY: `output` and `out` come together from the host for this call.
        unsafe { (self.0.output)(self.0.out, FishStr::new(text), 0) };
    }
}

/// The two points where mods register commands.
#[repr(transparent)]
pub struct RegisterContext(CommandRegisterPayload);

// SAFETY: `repr(transparent)` over an ABI payload that starts with `size: u32`.
unsafe impl Payload for RegisterContext {}

unsafe extern "C" fn execute(
    user: *mut c_void,
    index: u32,
    ctx: *const CommandCtxV1,
) -> FishStatus {
    if user.is_null() || ctx.is_null() {
        return FishStatus::INVALID_ARGUMENT;
    }
    host::guard("a command executor", || {
        // SAFETY: `user` is the executor vector boxed in `register`; `ctx` is valid for the call.
        let (executors, ctx) = unsafe { (&*user.cast::<Vec<Executor>>(), &*ctx) };
        let Some(f) = executors.get(index as usize) else {
            return FishStatus::NOT_FOUND;
        };
        let context = CommandContext(ctx);
        match f(&context) {
            Ok(v) => {
                // SAFETY: `result` is valid for the call when non-null.
                if !ctx.result.is_null() {
                    unsafe { ctx.result.write(v) };
                }
            }
            Err(e) => {
                // SAFETY: as in `feedback`.
                unsafe { (ctx.output)(ctx.out, FishStr::new(&e.0), 1) };
            }
        }
        FishStatus::OK
    })
}

unsafe extern "C" fn drop_executors(user: *mut c_void) {
    // SAFETY: `user` was created by `Box::into_raw(Box::new(Vec<Executor>))` in `register`.
    drop(unsafe { Box::from_raw(user.cast::<Vec<Executor>>()) });
}

impl RegisterContext {
    /// The side Fish runs on.
    pub fn side(&self) -> crate::loader::Environment {
        if self.0.side == fish_abi::services::SIDE_SERVER {
            crate::loader::Environment::Server
        } else {
            crate::loader::Environment::Client
        }
    }

    /// Registers a command tree; its root must be a literal.
    pub fn register(&self, root: CommandBuilder) -> Result<(), Error> {
        let (svc, host_ptr) = service::<CommandsServiceV1>(SERVICE_COMMANDS)?;
        let table = host::get()?;
        let mut json = String::new();
        let mut executors: Vec<Executor> = Vec::new();
        root.write(&mut json, &mut executors);
        let user = Box::into_raw(Box::new(executors)).cast::<c_void>();
        let kind = if self.0.kind == KIND_CLIENT {
            KIND_CLIENT
        } else {
            KIND_SERVER
        };
        // SAFETY: valid service table; the tree text outlives the call; ownership of `user` moves
        // to the host, which calls `drop_executors` exactly once.
        let status = unsafe {
            (svc.register)(
                host_ptr,
                table.mod_token,
                kind,
                FishStr::new(&json),
                execute,
                user,
                Some(drop_executors),
            )
        };
        if !status.is_ok() {
            // SAFETY: the host did not take ownership.
            unsafe { drop_executors(user) };
        }
        check("registering a command", status)
    }
}

/// Fired once after every mod's entrypoints ran. Commands registered here run on the server
/// (also in single player), with the permission level of whoever types them.
pub static REGISTER: Event<RegisterContext> =
    Event::new(fish_abi::commands::ids::REGISTER, Combine::Notify);

/// Client commands (run by the client itself; never need a server).
pub mod client {
    use super::RegisterContext;
    use crate::event::{Combine, Event};

    /// Fired once after every mod's entrypoints ran: register client commands here.
    pub static REGISTER: Event<RegisterContext> =
        Event::new(fish_abi::commands::ids::REGISTER_CLIENT, Combine::Notify);
}
