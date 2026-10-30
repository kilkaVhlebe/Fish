//! Generation-checked handles to host views that live only for one callback.

use std::sync::Mutex;
use std::thread::ThreadId;

use fish_abi::{FishHandle, FishStatus};
use fish_hooks::{ClientView, HudView};

#[derive(Clone, Copy)]
enum Target {
    Client(*mut (dyn ClientView + 'static)),
    Hud(*mut (dyn HudView + 'static)),
}

struct Slot {
    generation: u32,
    live: Option<(Target, ThreadId)>,
}

/// Table of live view handles.
#[derive(Default)]
pub(crate) struct Handles {
    slots: Mutex<Vec<Slot>>,
}

// SAFETY: the raw pointers in `Slot` are only dereferenced on the thread that registered them
// (checked via `ThreadId`), while the registering scope is still on the stack.
unsafe impl Send for Handles {}
// SAFETY: see above.
unsafe impl Sync for Handles {}

struct Release<'a> {
    handles: &'a Handles,
    index: u32,
}

impl Drop for Release<'_> {
    fn drop(&mut self) {
        let mut slots = self.handles.slots.lock().unwrap_or_else(|e| e.into_inner());
        let slot = &mut slots[self.index as usize];
        slot.live = None;
        slot.generation = slot.generation.wrapping_add(1);
    }
}

impl Handles {
    fn register(&self, target: Target) -> FishHandle {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        let live = Some((target, std::thread::current().id()));
        if let Some(i) = slots.iter().position(|s| s.live.is_none()) {
            slots[i].live = live;
            return FishHandle {
                index: i as u32,
                generation: slots[i].generation,
            };
        }
        slots.push(Slot {
            generation: 1,
            live,
        });
        FishHandle {
            index: slots.len() as u32 - 1,
            generation: 1,
        }
    }

    /// Runs `f` with a handle to `view`; the handle dies when `f` returns.
    pub(crate) fn client_scope<R>(
        &self,
        view: &mut dyn ClientView,
        f: impl FnOnce(FishHandle) -> R,
    ) -> R {
        // Game rules read and write the world of the game whose event runs.
        let _world = crate::gamerules::enter(view.world_id());
        // SAFETY: the erased lifetime is never observed after this function returns: the guard
        // below invalidates the handle (and its pointer) when `f` ends, even on unwind.
        let ptr: *mut (dyn ClientView + 'static) = unsafe { std::mem::transmute(view) };
        let handle = self.register(Target::Client(ptr));
        let _release = Release {
            handles: self,
            index: handle.index,
        };
        f(handle)
    }

    /// Runs `f` with a handle to the HUD draw target `view`.
    pub(crate) fn hud_scope<R>(
        &self,
        view: &mut dyn HudView,
        f: impl FnOnce(FishHandle) -> R,
    ) -> R {
        // SAFETY: as in `client_scope`.
        let ptr: *mut (dyn HudView + 'static) = unsafe { std::mem::transmute(view) };
        let handle = self.register(Target::Hud(ptr));
        let _release = Release {
            handles: self,
            index: handle.index,
        };
        f(handle)
    }

    fn resolve(&self, h: FishHandle) -> Result<Target, FishStatus> {
        let slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        let slot = slots
            .get(h.index as usize)
            .ok_or(FishStatus::STALE_HANDLE)?;
        match slot.live {
            Some((target, thread)) if slot.generation == h.generation => {
                if thread == std::thread::current().id() {
                    Ok(target)
                } else {
                    Err(FishStatus::WRONG_THREAD)
                }
            }
            _ => Err(FishStatus::STALE_HANDLE),
        }
    }

    /// Resolves a game handle and runs `f` on it.
    pub(crate) fn client<R>(
        &self,
        h: FishHandle,
        f: impl FnOnce(&mut dyn ClientView) -> R,
    ) -> Result<R, FishStatus> {
        match self.resolve(h)? {
            // SAFETY: the pointer is live (checked above) and used on its own thread.
            Target::Client(p) => Ok(f(unsafe { &mut *p })),
            Target::Hud(_) => Err(FishStatus::TYPE_MISMATCH),
        }
    }

    /// Resolves a HUD handle and runs `f` on it.
    pub(crate) fn hud<R>(
        &self,
        h: FishHandle,
        f: impl FnOnce(&mut dyn HudView) -> R,
    ) -> Result<R, FishStatus> {
        match self.resolve(h)? {
            // SAFETY: as in `client`.
            Target::Hud(p) => Ok(f(unsafe { &mut *p })),
            Target::Client(_) => Err(FishStatus::TYPE_MISMATCH),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fish_hooks::{PlayerSnapshot, Role};

    use super::*;

    struct V(u32);
    impl ClientView for V {
        fn role(&self) -> Role {
            Role::Window
        }
        fn player(&self) -> Option<PlayerSnapshot> {
            None
        }
        fn show_chat(&mut self, _: &str) {
            self.0 += 1;
        }
        fn game_dir(&self) -> PathBuf {
            PathBuf::new()
        }
    }

    struct H;
    impl HudView for H {
        fn fill(&mut self, _: [f32; 4], _: u32) {}
        fn text(&mut self, _: &str, _: f32, _: f32, _: u32, _: bool) {}
        fn text_width(&self, _: &str) -> f32 {
            1.0
        }
        fn gui_size(&self) -> [f32; 2] {
            [2.0, 3.0]
        }
    }

    #[test]
    fn handles_work_inside_their_scope_only() {
        let handles = Handles::default();
        let mut v = V(0);
        let mut kept = None;
        handles.client_scope(&mut v, |h| {
            kept = Some(h);
            handles.client(h, |c| c.show_chat("x")).unwrap();
            assert_eq!(
                handles.hud(h, |_| ()).unwrap_err(),
                FishStatus::TYPE_MISMATCH
            );
        });
        assert_eq!(v.0, 1);
        assert_eq!(
            handles.client(kept.unwrap(), |_| ()).unwrap_err(),
            FishStatus::STALE_HANDLE
        );
    }

    #[test]
    fn slots_are_reused_with_a_new_generation() {
        let handles = Handles::default();
        let mut h1 = H;
        let first = handles.hud_scope(&mut h1, |h| h);
        let second = handles.hud_scope(&mut h1, |h| h);
        assert_eq!(first.index, second.index);
        assert_ne!(first.generation, second.generation);
        assert_eq!(
            handles.hud(first, |_| ()).unwrap_err(),
            FishStatus::STALE_HANDLE
        );
        assert_eq!(
            handles
                .hud(
                    FishHandle {
                        index: 99,
                        generation: 1
                    },
                    |_| ()
                )
                .unwrap_err(),
            FishStatus::STALE_HANDLE
        );
    }

    #[test]
    fn nested_scopes_get_distinct_handles() {
        let handles = Handles::default();
        let (mut v, mut h) = (V(0), H);
        handles.client_scope(&mut v, |c| {
            handles.hud_scope(&mut h, |d| {
                assert_ne!(c.index, d.index);
                assert_eq!(handles.hud(d, |x| x.gui_size()).unwrap(), [2.0, 3.0]);
                assert_eq!(handles.hud(d, |x| x.text_width("a")).unwrap(), 1.0);
            });
            // the client handle survives the nested scope
            assert!(handles.client(c, |_| ()).is_ok());
        });
    }

    #[test]
    fn other_threads_are_refused() {
        let handles = Handles::default();
        let mut v = V(0);
        handles.client_scope(&mut v, |h| {
            std::thread::scope(|s| {
                let r = s
                    .spawn(|| handles.client(h, |_| ()).unwrap_err())
                    .join()
                    .unwrap();
                assert_eq!(r, FishStatus::WRONG_THREAD);
            });
        });
    }

    #[test]
    fn a_panic_in_the_scope_still_releases_the_handle() {
        let handles = Handles::default();
        let mut v = V(0);
        let mut kept = None;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handles.client_scope(&mut v, |h| {
                kept = Some(h);
                panic!("boom");
            });
        }));
        assert!(r.is_err());
        assert_eq!(
            handles.client(kept.unwrap(), |_| ()).unwrap_err(),
            FishStatus::STALE_HANDLE
        );
    }
}
