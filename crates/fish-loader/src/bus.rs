//! The event bus: definitions, phase ordering, copy-on-write listener lists, dispatch.

use std::collections::{BTreeSet, HashMap};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use fish_abi::{
    COMBINE_FIRST_NON_PASS, COMBINE_NOTIFY, DropUserFn, FishListener, FishStatus, ident,
};

/// The default phase every listener lands in unless it names another.
pub const DEFAULT_PHASE: &str = "fish:default";

/// Raw user pointer of a listener, owned by the registering mod.
struct UserPtr(*mut c_void);

// SAFETY: the pointer is opaque to the host; the registering mod guarantees its listener is
// `Send + Sync` (the safe API requires `Fn + Send + Sync`).
unsafe impl Send for UserPtr {}
// SAFETY: see above.
unsafe impl Sync for UserPtr {}

/// One registered listener.
pub(crate) struct ListenerRec {
    pub(crate) owner: u64,
    phase: usize,
    seq: u64,
    func: FishListener,
    user: UserPtr,
    drop_user: Option<DropUserFn>,
    failed: Arc<AtomicBool>,
}

impl Drop for ListenerRec {
    fn drop(&mut self) {
        if let Some(d) = self.drop_user {
            // SAFETY: `drop_user` was supplied together with `user` at registration and is
            // called exactly once, here.
            unsafe { d(self.user.0) };
        }
    }
}

#[derive(Default)]
struct EventConfig {
    phases: Vec<String>,
    edges: Vec<(usize, usize)>,
    regs: Vec<Arc<ListenerRec>>,
    next_seq: u64,
    cycle_reported: bool,
}

struct EventState {
    id: String,
    combine: u8,
    listeners: RwLock<Arc<Vec<Arc<ListenerRec>>>>,
    count: AtomicUsize,
    config: Mutex<EventConfig>,
}

/// Callbacks from the bus into the loader.
pub(crate) struct BusHooks {
    /// A listener panicked: `(owner token, message)`.
    pub on_panic: Box<dyn Fn(u64, String) + Send + Sync>,
    /// A non-fatal finding (cycles, odd statuses).
    pub warn: Box<dyn Fn(String) + Send + Sync>,
}

/// A listener registration request.
pub(crate) struct Registration<'a> {
    pub id: &'a str,
    pub owner: u64,
    pub failed: Arc<AtomicBool>,
    pub phase: &'a str,
    pub func: FishListener,
    pub user: *mut c_void,
    pub drop_user: Option<DropUserFn>,
}

/// Index of an event, resolved once for cheap repeated dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventIndex(pub u32);

/// The event bus.
pub struct EventBus {
    events: RwLock<Vec<Arc<EventState>>>,
    by_id: RwLock<HashMap<String, u32>>,
    hooks: BusHooks,
}

/// Topological order of `n` phases (ties by first-seen index); `Err` lists nodes stuck in cycles.
pub(crate) fn order_phases(n: usize, edges: &[(usize, usize)]) -> Result<Vec<usize>, Vec<usize>> {
    let mut indegree = vec![0usize; n];
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut seen = BTreeSet::new();
    for &(a, b) in edges {
        if seen.insert((a, b)) {
            out[a].push(b);
            indegree[b] += 1;
        }
    }
    let mut ready: BTreeSet<usize> = (0..n).filter(|&i| indegree[i] == 0).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(i) = ready.pop_first() {
        order.push(i);
        for &j in &out[i] {
            indegree[j] -= 1;
            if indegree[j] == 0 {
                ready.insert(j);
            }
        }
    }
    if order.len() == n {
        Ok(order)
    } else {
        Err((0..n).filter(|i| !order.contains(i)).collect())
    }
}

impl EventBus {
    pub(crate) fn new(hooks: BusHooks) -> EventBus {
        EventBus {
            events: RwLock::default(),
            by_id: RwLock::default(),
            hooks,
        }
    }

    fn get(&self, index: u32) -> Option<Arc<EventState>> {
        self.events
            .read()
            .expect("bus lock")
            .get(index as usize)
            .cloned()
    }

    /// Defines an event; idempotent when the combine mode matches.
    pub fn define(&self, id: &str, combine: u8) -> FishStatus {
        if !ident::is_valid_identifier(id)
            || (combine != COMBINE_NOTIFY && combine != COMBINE_FIRST_NON_PASS)
        {
            return FishStatus::INVALID_ARGUMENT;
        }
        let mut by_id = self.by_id.write().expect("bus lock");
        if let Some(&i) = by_id.get(id) {
            let existing = self.get(i).expect("indexed event exists");
            return if existing.combine == combine {
                FishStatus::OK
            } else {
                FishStatus::ALREADY_EXISTS
            };
        }
        let mut events = self.events.write().expect("bus lock");
        let index = events.len() as u32;
        let cfg = EventConfig {
            phases: vec![DEFAULT_PHASE.to_string()],
            ..EventConfig::default()
        };
        events.push(Arc::new(EventState {
            id: id.to_string(),
            combine,
            listeners: RwLock::new(Arc::new(Vec::new())),
            count: AtomicUsize::new(0),
            config: Mutex::new(cfg),
        }));
        by_id.insert(id.to_string(), index);
        FishStatus::OK
    }

    /// Resolves an event id to an index.
    pub fn lookup(&self, id: &str) -> Option<EventIndex> {
        self.by_id
            .read()
            .expect("bus lock")
            .get(id)
            .copied()
            .map(EventIndex)
    }

    /// Number of listeners currently registered on `id` (failed mods included).
    pub fn listener_count(&self, id: &str) -> usize {
        self.lookup(id)
            .and_then(|i| self.get(i.0))
            .map_or(0, |e| e.count.load(Ordering::Acquire))
    }

    fn find_phase(cfg: &mut EventConfig, phase: &str) -> usize {
        if let Some(i) = cfg.phases.iter().position(|p| p == phase) {
            return i;
        }
        cfg.phases.push(phase.to_string());
        cfg.phases.len() - 1
    }

    /// Recomputes the sorted listener list after any change.
    fn rebuild(&self, ev: &EventState, cfg: &mut EventConfig) {
        let rank = match order_phases(cfg.phases.len(), &cfg.edges) {
            Ok(order) => {
                let mut rank = vec![0; order.len()];
                for (pos, phase) in order.into_iter().enumerate() {
                    rank[phase] = pos;
                }
                rank
            }
            Err(stuck) => {
                if !cfg.cycle_reported {
                    cfg.cycle_reported = true;
                    let names: Vec<&str> = stuck.iter().map(|&i| cfg.phases[i].as_str()).collect();
                    (self.hooks.warn)(format!(
                        "event {}: phase ordering has a cycle between {}; using registration order",
                        ev.id,
                        names.join(", ")
                    ));
                }
                (0..cfg.phases.len()).collect()
            }
        };
        let mut sorted = cfg.regs.clone();
        sorted.sort_by_key(|l| (rank[l.phase], l.seq));
        ev.count.store(sorted.len(), Ordering::Release);
        *ev.listeners.write().expect("listener lock") = Arc::new(sorted);
    }

    /// Registers a listener for the mod `owner`; `failed` is that mod's failure flag.
    pub(crate) fn register(&self, reg: Registration<'_>) -> FishStatus {
        let Registration {
            id,
            owner,
            failed,
            phase,
            func,
            user,
            drop_user,
        } = reg;
        if !ident::is_valid_identifier(phase) {
            return FishStatus::INVALID_ARGUMENT;
        }
        let Some(ev) = self.lookup(id).and_then(|i| self.get(i.0)) else {
            return FishStatus::NOT_FOUND;
        };
        let mut cfg = ev.config.lock().expect("event config");
        let phase = Self::find_phase(&mut cfg, phase);
        let seq = cfg.next_seq;
        cfg.next_seq += 1;
        cfg.regs.push(Arc::new(ListenerRec {
            owner,
            phase,
            seq,
            func,
            user: UserPtr(user),
            drop_user,
            failed,
        }));
        self.rebuild(&ev, &mut cfg);
        FishStatus::OK
    }

    /// Orders two phases: `first` runs before `second`.
    pub fn phase_order(&self, id: &str, first: &str, second: &str) -> FishStatus {
        if !ident::is_valid_identifier(first) || !ident::is_valid_identifier(second) {
            return FishStatus::INVALID_ARGUMENT;
        }
        let Some(ev) = self.lookup(id).and_then(|i| self.get(i.0)) else {
            return FishStatus::NOT_FOUND;
        };
        let mut cfg = ev.config.lock().expect("event config");
        let a = Self::find_phase(&mut cfg, first);
        let b = Self::find_phase(&mut cfg, second);
        cfg.edges.push((a, b));
        self.rebuild(&ev, &mut cfg);
        FishStatus::OK
    }

    /// Invokes an event by index.
    ///
    /// For `FirstNonPass` events `out` must point at a result whose first byte is the action
    /// (initialised to `Pass`); dispatch stops once it is not `0`.
    ///
    /// # Safety
    /// `payload` and `out` must be valid for the listeners of this event.
    pub unsafe fn invoke(
        &self,
        index: EventIndex,
        payload: *mut c_void,
        out: *mut c_void,
    ) -> FishStatus {
        let Some(ev) = self.get(index.0) else {
            return FishStatus::NOT_FOUND;
        };
        if ev.count.load(Ordering::Acquire) == 0 {
            return FishStatus::OK;
        }
        let snapshot = ev.listeners.read().expect("listener lock").clone();
        for l in snapshot.iter() {
            if l.failed.load(Ordering::Acquire) {
                continue;
            }
            // SAFETY: `func`/`user` came together from `register`; payload validity is the caller's.
            let st = unsafe { (l.func)(l.user.0, payload, out) };
            if st == FishStatus::PANICKED {
                (self.hooks.on_panic)(l.owner, format!("a listener of {} panicked", ev.id));
            } else if !st.is_ok() {
                (self.hooks.warn)(format!("a listener of {} returned {}", ev.id, st.name()));
            }
            if ev.combine == COMBINE_FIRST_NON_PASS
                && !out.is_null()
                // SAFETY: first-non-pass events pass an `out` whose first byte is the action.
                && unsafe { *out.cast::<u8>() } != 0
            {
                break;
            }
        }
        FishStatus::OK
    }

    /// Invokes an event by id.
    ///
    /// # Safety
    /// See [`EventBus::invoke`].
    pub unsafe fn invoke_id(&self, id: &str, payload: *mut c_void, out: *mut c_void) -> FishStatus {
        match self.lookup(id) {
            // SAFETY: forwarded to the caller.
            Some(i) => unsafe { self.invoke(i, payload, out) },
            None => FishStatus::NOT_FOUND,
        }
    }

    /// Drops every listener (called before mod libraries unload).
    pub(crate) fn clear(&self) {
        for ev in self.events.read().expect("bus lock").iter() {
            let mut cfg = ev.config.lock().expect("event config");
            cfg.regs.clear();
            *ev.listeners.write().expect("listener lock") = Arc::new(Vec::new());
            ev.count.store(0, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_order_is_stable_topological() {
        // 0 default, 1 a, 2 b, 3 c
        assert_eq!(order_phases(4, &[]).unwrap(), [0, 1, 2, 3]);
        assert_eq!(order_phases(4, &[(2, 0)]).unwrap(), [1, 2, 0, 3]);
        assert_eq!(order_phases(4, &[(3, 1), (1, 2)]).unwrap(), [0, 3, 1, 2]);
        assert_eq!(order_phases(3, &[(0, 1), (0, 1)]).unwrap(), [0, 1, 2]);
    }

    #[test]
    fn cycles_report_the_stuck_nodes() {
        assert_eq!(order_phases(3, &[(1, 2), (2, 1)]).unwrap_err(), [1, 2]);
        assert_eq!(order_phases(2, &[(0, 0)]).unwrap_err(), [0]);
    }
}
