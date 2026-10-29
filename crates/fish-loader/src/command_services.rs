//! Host side of `fish:commands`.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};

use fish_abi::commands::{CommandExecFn, CommandsServiceV1};
use fish_abi::{DropUserFn, FishServiceHeader, FishStatus, FishStr};

use crate::commands::{Registered, executor};
use crate::inner::Inner;

unsafe extern "C" fn register(
    host: *mut c_void,
    token: u64,
    kind: u8,
    tree: FishStr,
    exec: CommandExecFn,
    user: *mut c_void,
    drop_user: Option<DropUserFn>,
) -> FishStatus {
    // SAFETY: ABI contract: `host` is the loader's `Inner`.
    let inner = unsafe { &*host.cast::<Inner>() };
    let ex = executor(exec, user, drop_user, token);
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the tree string is valid for the call.
        let Ok(tree) = (unsafe { tree.as_str() }) else {
            return FishStatus::INVALID_ARGUMENT;
        };
        let name = inner
            .record(token)
            .map_or_else(|| "a mod".to_string(), |r| r.manifest.name.clone());
        let mut commands = inner.commands.lock().unwrap_or_else(|e| e.into_inner());
        match commands.register(kind, tree, ex) {
            Ok(Registered::Added) => FishStatus::OK,
            Ok(Registered::Duplicate(first)) => {
                let first = inner
                    .record(first)
                    .map_or_else(|| "another mod".to_string(), |r| r.manifest.name.clone());
                drop(commands);
                inner.warn(format!(
                    "a command of {name} was ignored: its name is already taken by {first}"
                ));
                FishStatus::OK
            }
            Err(e) => {
                drop(commands);
                inner.warn(format!("{name} registered an invalid command: {e}"));
                FishStatus::INVALID_ARGUMENT
            }
        }
    }))
    .unwrap_or(FishStatus::PANICKED)
}

/// The `fish:commands` table.
pub(crate) static COMMANDS: CommandsServiceV1 = CommandsServiceV1 {
    header: FishServiceHeader {
        size: size_of::<CommandsServiceV1>(),
        version: 1,
    },
    register,
};
