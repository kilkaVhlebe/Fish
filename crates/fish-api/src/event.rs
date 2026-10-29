//! Events: Fabric `Event<T>` parity on top of the host event bus.
//!
//! An [`Event`] is a cheap static handle: `static END_TICK: Event<TickPayload> = Event::new(..)`.
//! Listeners run in *phases* ordered with [`Event::add_phase_ordering`]; within a phase in
//! registration order. A listener receives `&mut P` (so `Notify` events can modify the payload)
//! and returns an [`EventResult`].
//!
//! * [`Combine::Notify`]: every listener runs; results are ignored (use `R = ()`).
//! * [`Combine::FirstNonPass`]: dispatch stops at the first listener whose result is not `Pass`;
//!   the result type's first byte is the [`ActionResult`].
//!
//! Panics in a listener never cross the FFI boundary: the trampoline returns `PANICKED` and the
//! host marks the mod failed.

pub mod client;
pub mod debug;
pub mod entity;
pub mod interaction;
pub mod lifecycle;
pub mod message;

use std::ffi::c_void;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};

use fish_abi::{COMBINE_FIRST_NON_PASS, COMBINE_NOTIFY, FishStatus, FishStr, ident, level};

use crate::error::{Error, check};
use crate::host;
use crate::identifier::ActionResult;

/// The default phase.
pub const DEFAULT_PHASE: &str = "fish:default";

/// How the results of an event's listeners combine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combine {
    /// Every listener runs.
    Notify,
    /// Stop at the first listener that does not return `Pass`.
    FirstNonPass,
}

impl Combine {
    fn wire(self) -> u8 {
        match self {
            Combine::Notify => COMBINE_NOTIFY,
            Combine::FirstNonPass => COMBINE_FIRST_NON_PASS,
        }
    }
}

/// A named phase of an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Phase(pub &'static str);

impl Phase {
    /// The default phase, `fish:default`.
    pub const DEFAULT: Phase = Phase(DEFAULT_PHASE);
}

/// Marker for event payload structs.
///
/// # Safety
/// The type must be `#[repr(C)]`, start with a `size: u32` field holding the struct's size, and
/// only ever grow by appending fields.
pub unsafe trait Payload: Sized + 'static {}

/// What a listener returns.
///
/// # Safety
/// Implementors must be `#[repr(C)]`-compatible and, for [`Combine::FirstNonPass`] events, start
/// with a `u8` holding an [`ActionResult`] wire value.
pub unsafe trait EventResult: Sized + 'static {
    /// The value an invocation starts from (`Pass`).
    fn pass() -> Self;
    /// The action this result carries.
    fn action(&self) -> ActionResult;
}

// SAFETY: `()` carries no data; Notify events pass a null `out`.
unsafe impl EventResult for () {
    fn pass() -> Self {}
    fn action(&self) -> ActionResult {
        ActionResult::Pass
    }
}

// SAFETY: `ActionResult` is `#[repr(u8)]`.
unsafe impl EventResult for ActionResult {
    fn pass() -> Self {
        ActionResult::Pass
    }
    fn action(&self) -> ActionResult {
        *self
    }
}

// SAFETY: `TypedActionResult` is `#[repr(C)]` with the action byte first.
unsafe impl<T: Default + 'static> EventResult for crate::identifier::TypedActionResult<T> {
    fn pass() -> Self {
        Self::default()
    }
    fn action(&self) -> ActionResult {
        self.result()
    }
}

/// A typed handle to a host event.
pub struct Event<P: Payload, R: EventResult = ()> {
    id: &'static str,
    combine: Combine,
    defined: AtomicBool,
    _types: PhantomData<fn(&mut P) -> R>,
}

unsafe extern "C" fn drop_box<F>(user: *mut c_void) {
    // SAFETY: `user` was created by `Box::into_raw(Box::new(f))` of the same `F` in `register_phase`.
    drop(unsafe { Box::from_raw(user.cast::<F>()) });
}

unsafe extern "C" fn trampoline<P, R, F>(
    user: *mut c_void,
    payload: *mut c_void,
    out: *mut c_void,
) -> FishStatus
where
    P: Payload,
    R: EventResult,
    F: Fn(&mut P) -> R + Send + Sync + 'static,
{
    if user.is_null() || payload.is_null() {
        return FishStatus::INVALID_ARGUMENT;
    }
    // SAFETY: every payload starts with its `u32` size (see `Payload`).
    let declared = unsafe { payload.cast::<u32>().read_unaligned() } as usize;
    if declared < std::mem::size_of::<P>() {
        return FishStatus::INVALID_ARGUMENT;
    }
    host::guard("an event listener", || {
        // SAFETY: `user` points at the `F` boxed in `register_phase` and lives until `drop_box`;
        // `payload` is a valid `P` (size checked above) exclusively borrowed for this call.
        let (f, p) = unsafe { (&*user.cast::<F>(), &mut *payload.cast::<P>()) };
        let result = f(p);
        if !out.is_null() && std::mem::size_of::<R>() > 0 {
            // SAFETY: the host passes an `out` valid for an `R` for events with a result.
            unsafe { out.cast::<R>().write(result) };
        }
        FishStatus::OK
    })
}

impl<P: Payload, R: EventResult> Event<P, R> {
    /// Declares an event. `id` must be a valid `namespace:path` identifier.
    pub const fn new(id: &'static str, combine: Combine) -> Self {
        Event {
            id,
            combine,
            defined: AtomicBool::new(false),
            _types: PhantomData,
        }
    }

    /// The wire id.
    pub const fn id(&self) -> &'static str {
        self.id
    }

    /// Defines the event on the host (idempotent). Called automatically by the other methods.
    pub fn define(&self) -> Result<(), Error> {
        if self.defined.load(Ordering::Acquire) {
            return Ok(());
        }
        if !ident::is_valid_identifier(self.id) {
            return Err(Error::InvalidArgument(format!(
                "invalid event id {:?}",
                self.id
            )));
        }
        let h = host::get()?;
        // SAFETY: the host table is valid; the id string outlives the call.
        let status = unsafe {
            (h.event_define)(
                h.host,
                h.mod_token,
                FishStr::from_static(self.id),
                self.combine.wire(),
            )
        };
        check("defining an event", status)?;
        self.defined.store(true, Ordering::Release);
        Ok(())
    }

    /// Registers a listener in the default phase.
    pub fn register<F>(&self, listener: F) -> Result<(), Error>
    where
        F: Fn(&mut P) -> R + Send + Sync + 'static,
    {
        self.register_phase(Phase::DEFAULT, listener)
    }

    /// Registers a listener in `phase`.
    pub fn register_phase<F>(&self, phase: Phase, listener: F) -> Result<(), Error>
    where
        F: Fn(&mut P) -> R + Send + Sync + 'static,
    {
        if !ident::is_valid_identifier(phase.0) {
            return Err(Error::InvalidArgument(format!(
                "invalid phase id {:?}",
                phase.0
            )));
        }
        self.define()?;
        let h = host::get()?;
        let user = Box::into_raw(Box::new(listener)).cast::<c_void>();
        // SAFETY: valid host table; strings outlive the call; ownership of `user` moves to the host,
        // which calls `drop_box::<F>` exactly once.
        let status = unsafe {
            (h.event_register)(
                h.host,
                h.mod_token,
                FishStr::from_static(self.id),
                FishStr::new(phase.0),
                trampoline::<P, R, F>,
                user,
                Some(drop_box::<F>),
            )
        };
        if !status.is_ok() {
            // The host did not take ownership.
            // SAFETY: ownership never left this function.
            unsafe { drop_box::<F>(user) };
        }
        check("registering an event listener", status)
    }

    /// Makes phase `first` run before phase `second` for this event.
    pub fn add_phase_ordering(&self, first: Phase, second: Phase) -> Result<(), Error> {
        self.define()?;
        let h = host::get()?;
        // SAFETY: valid host table; strings outlive the call.
        let status = unsafe {
            (h.event_phase_order)(
                h.host,
                h.mod_token,
                FishStr::from_static(self.id),
                FishStr::new(first.0),
                FishStr::new(second.0),
            )
        };
        check("ordering event phases", status)
    }

    /// Invokes the event (for events a mod defines itself). Returns the combined result.
    pub fn invoke(&self, payload: &mut P) -> Result<R, Error> {
        self.define()?;
        let h = host::get()?;
        let mut out = R::pass();
        let out_ptr: *mut c_void = if std::mem::size_of::<R>() > 0 {
            (&raw mut out).cast()
        } else {
            std::ptr::null_mut()
        };
        // SAFETY: valid host table; `payload` and `out` are exclusively borrowed for the call.
        let status = unsafe {
            (h.event_invoke)(
                h.host,
                h.mod_token,
                FishStr::from_static(self.id),
                (payload as *mut P).cast(),
                out_ptr,
            )
        };
        check("invoking an event", status)?;
        Ok(out)
    }
}

/// Runs `f` with panics converted to a log record; used by generated code for object entrypoints.
#[doc(hidden)]
pub fn __catch<T>(what: &str, f: impl FnOnce() -> T) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(v) => Some(v),
        Err(p) => {
            host::host_log(
                level::ERROR,
                "fish_api",
                &format!("panic in {what}: {}", host::panic_message(&*p)),
            );
            None
        }
    }
}
