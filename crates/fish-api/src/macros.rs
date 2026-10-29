//! The `export_mod!` macro and the runtime support it expands to.

use std::ffi::c_void;

use fish_abi::{FishHostV1, FishStatus, level};

use crate::host;

/// What an entrypoint function may return.
pub trait IntoInitResult {
    /// Converts to `Ok(())` or an error message.
    fn into_init_result(self) -> Result<(), String>;
}

impl IntoInitResult for () {
    fn into_init_result(self) -> Result<(), String> {
        Ok(())
    }
}

impl<E: std::fmt::Display> IntoInitResult for Result<(), E> {
    fn into_init_result(self) -> Result<(), String> {
        self.map_err(|e| e.to_string())
    }
}

/// Runs an `init` entrypoint: attaches the host, installs the log bridge, catches panics.
#[doc(hidden)]
pub fn __run_init<R: IntoInitResult>(
    host_table: *const FishHostV1,
    name: &str,
    f: impl FnOnce() -> R,
) -> FishStatus {
    if host_table.is_null() {
        return FishStatus::INVALID_ARGUMENT;
    }
    host::attach(host_table);
    host::install_logger();
    host::guard(name, || match f().into_init_result() {
        Ok(()) => FishStatus::OK,
        Err(message) => {
            host::host_log(
                level::ERROR,
                "fish_api",
                &format!("entrypoint {name} failed: {message}"),
            );
            FishStatus::INTERNAL
        }
    })
}

/// Runs an `object` entrypoint and erases the returned reference.
#[doc(hidden)]
pub fn __object<T: 'static>(f: impl FnOnce() -> &'static T) -> *const c_void {
    match crate::event::__catch("an object entrypoint", f) {
        Some(r) => std::ptr::from_ref(r).cast(),
        None => std::ptr::null(),
    }
}

/// Exports the mod: generates the `fish_mod_v1` symbol and entrypoint trampolines.
///
/// ```ignore
/// fish_api::export_mod! {
///     main: [init],
///     client: [init_client],
///     "fish:modmenu": object [menu_api],
/// }
/// ```
///
/// Each name is a function in scope. `init` entrypoints are `fn()` or `fn() -> Result<(), E>`;
/// `object` entrypoints are `fn() -> &'static T`. Keys are identifiers or string literals and
/// must match the manifest's `[entrypoints]` table.
#[macro_export]
macro_rules! export_mod {
    // --- tt-muncher: flatten into (kind, key, name) triples -----------------------------------
    (@collect [$($acc:tt)*]) => {
        $crate::export_mod!(@emit $($acc)*);
    };
    (@collect [$($acc:tt)*] $key:ident : object [ $($name:ident),* $(,)? ] $(, $($rest:tt)*)?) => {
        $crate::export_mod!(@collect [$($acc)* $( (object, stringify!($key), $name) )*] $($($rest)*)?);
    };
    (@collect [$($acc:tt)*] $key:literal : object [ $($name:ident),* $(,)? ] $(, $($rest:tt)*)?) => {
        $crate::export_mod!(@collect [$($acc)* $( (object, $key, $name) )*] $($($rest)*)?);
    };
    (@collect [$($acc:tt)*] $key:ident : [ $($name:ident),* $(,)? ] $(, $($rest:tt)*)?) => {
        $crate::export_mod!(@collect [$($acc)* $( (init, stringify!($key), $name) )*] $($($rest)*)?);
    };
    (@collect [$($acc:tt)*] $key:literal : [ $($name:ident),* $(,)? ] $(, $($rest:tt)*)?) => {
        $crate::export_mod!(@collect [$($acc)* $( (init, $key, $name) )*] $($($rest)*)?);
    };

    // --- emission ----------------------------------------------------------------------------
    (@emit $( ($kind:ident, $key:expr, $name:ident) )*) => {
        static __FISH_ENTRYPOINTS: &[$crate::abi::FishEntrypointV1] = &[
            $( $crate::export_mod!(@entry $kind, $key, $name) ),*
        ];

        static __FISH_MOD: $crate::abi::FishModV1 = $crate::abi::FishModV1 {
            size: ::core::mem::size_of::<$crate::abi::FishModV1>(),
            abi_major: $crate::abi::FISH_ABI_MAJOR,
            abi_minor: $crate::abi::FISH_ABI_MINOR,
            entrypoints: __FISH_ENTRYPOINTS.as_ptr(),
            entrypoint_count: __FISH_ENTRYPOINTS.len(),
        };

        /// Entry symbol looked up by the Fish loader.
        #[unsafe(no_mangle)]
        pub extern "C" fn fish_mod_v1() -> *const $crate::abi::FishModV1 {
            &__FISH_MOD
        }
    };

    (@entry init, $key:expr, $name:ident) => {
        $crate::abi::FishEntrypointV1 {
            key: $crate::abi::FishStr::from_static($key),
            name: $crate::abi::FishStr::from_static(stringify!($name)),
            kind: $crate::abi::ENTRYPOINT_KIND_INIT,
            init: Some({
                unsafe extern "C" fn __trampoline(
                    host: *const $crate::abi::FishHostV1,
                ) -> $crate::abi::FishStatus {
                    $crate::__run_init(host, stringify!($name), || $name())
                }
                __trampoline
            }),
            object: None,
        }
    };

    (@entry object, $key:expr, $name:ident) => {
        $crate::abi::FishEntrypointV1 {
            key: $crate::abi::FishStr::from_static($key),
            name: $crate::abi::FishStr::from_static(stringify!($name)),
            kind: $crate::abi::ENTRYPOINT_KIND_OBJECT,
            init: None,
            object: Some({
                unsafe extern "C" fn __trampoline() -> *const ::core::ffi::c_void {
                    $crate::__object(|| $name())
                }
                __trampoline
            }),
        }
    };

    ($($body:tt)*) => {
        $crate::export_mod!(@collect [] $($body)*);
    };
}
