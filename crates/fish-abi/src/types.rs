//! Primitive ABI types.

use core::str::Utf8Error;

/// A borrowed UTF-8 string, valid for the duration of the call it is passed to.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FishStr {
    /// Pointer to the first byte (may be dangling when `len == 0`).
    pub ptr: *const u8,
    /// Length in bytes.
    pub len: usize,
}

// SAFETY: a `FishStr` is an immutable view; sending or sharing it does not touch the data. The
// creator guarantees the data outlives every use.
unsafe impl Send for FishStr {}
// SAFETY: see above.
unsafe impl Sync for FishStr {}

impl FishStr {
    /// The empty string.
    pub const EMPTY: FishStr = FishStr {
        ptr: core::ptr::NonNull::<u8>::dangling().as_ptr(),
        len: 0,
    };

    /// Views a `&'static str`; usable in `static` tables.
    pub const fn from_static(s: &'static str) -> FishStr {
        FishStr {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }

    /// Views `s` for the duration of a call. The caller must not let the `FishStr` outlive `s`.
    pub const fn new(s: &str) -> FishStr {
        FishStr {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }

    /// Reads the string back, validating UTF-8. A null pointer with length 0 is the empty string.
    ///
    /// # Safety
    /// `ptr` must point to `len` readable bytes that stay valid and unmodified for `'a`, unless
    /// `len == 0`.
    pub unsafe fn as_str<'a>(self) -> Result<&'a str, Utf8Error> {
        if self.len == 0 {
            return Ok("");
        }
        // SAFETY: guaranteed by the caller.
        let bytes = unsafe { core::slice::from_raw_parts(self.ptr, self.len) };
        core::str::from_utf8(bytes)
    }
}

/// A borrowed byte slice, valid for the duration of the call it is passed to.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FishBytes {
    /// Pointer to the first byte (may be dangling when `len == 0`).
    pub ptr: *const u8,
    /// Length in bytes.
    pub len: usize,
}

// SAFETY: same reasoning as `FishStr`.
unsafe impl Send for FishBytes {}
// SAFETY: same reasoning as `FishStr`.
unsafe impl Sync for FishBytes {}

impl FishBytes {
    /// Views `b` for the duration of a call.
    pub const fn new(b: &[u8]) -> FishBytes {
        FishBytes {
            ptr: b.as_ptr(),
            len: b.len(),
        }
    }

    /// Reads the bytes back. A zero length yields an empty slice regardless of the pointer.
    ///
    /// # Safety
    /// `ptr` must point to `len` readable bytes valid for `'a`, unless `len == 0`.
    pub unsafe fn as_slice<'a>(self) -> &'a [u8] {
        if self.len == 0 {
            return &[];
        }
        // SAFETY: guaranteed by the caller.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }
}

/// Result code of every fallible ABI function.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FishStatus(pub i32);

impl FishStatus {
    /// Success.
    pub const OK: FishStatus = FishStatus(0);
    /// An argument was null, out of range, or malformed.
    pub const INVALID_ARGUMENT: FishStatus = FishStatus(-1);
    /// The named thing does not exist.
    pub const NOT_FOUND: FishStatus = FishStatus(-2);
    /// The host does not support the request.
    pub const UNSUPPORTED: FishStatus = FishStatus(-3);
    /// Code on the other side of the boundary panicked.
    pub const PANICKED: FishStatus = FishStatus(-4);
    /// A [`FishHandle`] was used after its callback returned.
    pub const STALE_HANDLE: FishStatus = FishStatus(-5);
    /// The call is not allowed on this thread.
    pub const WRONG_THREAD: FishStatus = FishStatus(-6);
    /// The thing being created already exists.
    pub const ALREADY_EXISTS: FishStatus = FishStatus(-7);
    /// A type tag or payload type did not match.
    pub const TYPE_MISMATCH: FishStatus = FishStatus(-8);
    /// Host and mod ABI versions are incompatible.
    pub const VERSION_MISMATCH: FishStatus = FishStatus(-9);
    /// An internal error in the host.
    pub const INTERNAL: FishStatus = FishStatus(-100);

    /// True for [`FishStatus::OK`].
    pub const fn is_ok(self) -> bool {
        self.0 == 0
    }

    /// The symbolic name of a known code, or `"UNKNOWN"`.
    pub const fn name(self) -> &'static str {
        match self.0 {
            0 => "OK",
            -1 => "INVALID_ARGUMENT",
            -2 => "NOT_FOUND",
            -3 => "UNSUPPORTED",
            -4 => "PANICKED",
            -5 => "STALE_HANDLE",
            -6 => "WRONG_THREAD",
            -7 => "ALREADY_EXISTS",
            -8 => "TYPE_MISMATCH",
            -9 => "VERSION_MISMATCH",
            -100 => "INTERNAL",
            _ => "UNKNOWN",
        }
    }
}

/// A generation-checked reference to a host object, valid only during the callback it arrived in.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FishHandle {
    /// Slot in the host's handle table.
    pub index: u32,
    /// Generation of the slot; a mismatch means the handle is stale.
    pub generation: u32,
}

/// Results of event listeners, transported as a `u8` and validated, never transmuted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ActionResult {
    /// Not handled; later listeners run.
    Pass = 0,
    /// Handled successfully.
    Success = 1,
    /// Handled; the default behaviour is cancelled but the action counts as consumed.
    Consume = 2,
    /// Handled and failed.
    Fail = 3,
}

impl ActionResult {
    /// Decodes a wire value; `None` for anything outside `0..=3`.
    pub const fn from_u8(v: u8) -> Option<ActionResult> {
        match v {
            0 => Some(ActionResult::Pass),
            1 => Some(ActionResult::Success),
            2 => Some(ActionResult::Consume),
            3 => Some(ActionResult::Fail),
            _ => None,
        }
    }

    /// The wire value.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fish_str_roundtrip() {
        let s = "héllo";
        let f = FishStr::new(s);
        // SAFETY: `s` outlives `f`.
        assert_eq!(unsafe { f.as_str() }, Ok(s));
        // SAFETY: empty strings never dereference.
        assert_eq!(unsafe { FishStr::EMPTY.as_str() }, Ok(""));
        static S: FishStr = FishStr::from_static("static");
        // SAFETY: points at static data.
        assert_eq!(unsafe { S.as_str() }, Ok("static"));
    }

    #[test]
    fn fish_str_rejects_invalid_utf8() {
        let bad = [0xffu8, 0xfe];
        let f = FishStr {
            ptr: bad.as_ptr(),
            len: 2,
        };
        // SAFETY: `bad` outlives `f`.
        assert!(unsafe { f.as_str() }.is_err());
    }

    #[test]
    fn null_empty_string_is_fine() {
        let f = FishStr {
            ptr: core::ptr::null(),
            len: 0,
        };
        // SAFETY: length zero.
        assert_eq!(unsafe { f.as_str() }, Ok(""));
        let b = FishBytes {
            ptr: core::ptr::null(),
            len: 0,
        };
        // SAFETY: length zero.
        assert!(unsafe { b.as_slice() }.is_empty());
    }

    #[test]
    fn bytes_roundtrip() {
        let data = [1u8, 2, 3];
        // SAFETY: `data` outlives the view.
        assert_eq!(unsafe { FishBytes::new(&data).as_slice() }, &data);
    }

    #[test]
    fn status_codes_roundtrip() {
        let all = [
            (FishStatus::OK, 0, "OK"),
            (FishStatus::INVALID_ARGUMENT, -1, "INVALID_ARGUMENT"),
            (FishStatus::NOT_FOUND, -2, "NOT_FOUND"),
            (FishStatus::UNSUPPORTED, -3, "UNSUPPORTED"),
            (FishStatus::PANICKED, -4, "PANICKED"),
            (FishStatus::STALE_HANDLE, -5, "STALE_HANDLE"),
            (FishStatus::WRONG_THREAD, -6, "WRONG_THREAD"),
            (FishStatus::ALREADY_EXISTS, -7, "ALREADY_EXISTS"),
            (FishStatus::TYPE_MISMATCH, -8, "TYPE_MISMATCH"),
            (FishStatus::VERSION_MISMATCH, -9, "VERSION_MISMATCH"),
            (FishStatus::INTERNAL, -100, "INTERNAL"),
        ];
        for (s, raw, name) in all {
            assert_eq!(s.0, raw);
            assert_eq!(s.name(), name);
            assert_eq!(s.is_ok(), raw == 0);
        }
        assert_eq!(FishStatus(42).name(), "UNKNOWN");
    }

    #[test]
    fn action_results_are_validated() {
        for v in 0..=3u8 {
            assert_eq!(ActionResult::from_u8(v).unwrap().as_u8(), v);
        }
        assert_eq!(ActionResult::from_u8(4), None);
        assert_eq!(ActionResult::from_u8(255), None);
    }
}
