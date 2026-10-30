//! Creative tabs (Fabric's `ItemGroup`): a mod adds tabs to the creative inventory and puts items
//! into its own tabs and into vanilla's.
//!
//! Register while `pre_launch` or `main` runs (the same freeze as [`registry`](crate::registry)):
//!
//! ```no_run
//! use fish_api::item_group::{ItemGroup, ItemGroups};
//!
//! # fn init() -> Result<(), fish_api::Error> {
//! ItemGroup::builder("mymod:ruby_tab")
//!     .title("Ruby")
//!     .icon("mymod:ruby")
//!     .entries(["mymod:ruby", "mymod:ruby_block"])
//!     .register()?;
//! ItemGroups::modify_entries("minecraft:ingredients", ["mymod:ruby"])?;
//! # Ok(()) }
//! ```
//!
//! The namespace of a tab's identifier must be the id of the registering mod. Entries are item
//! ids (`namespace:path`): items of Fish mods, blocks of Fish mods (their items) and vanilla items.
//! Tabs take the free places of the creative screen, and every entry also reaches the search tab.

use fish_abi::items::{ItemGroupServiceV1, SERVICE_ITEM_GROUP};
use fish_abi::{FishStatus, FishStr};

use crate::client::service;
use crate::error::{Error, check};
use crate::host;

/// Collects what a creative tab shows; finish with [`register`](Self::register).
#[derive(Debug, Clone, PartialEq)]
pub struct ItemGroupBuilder {
    id: String,
    title: String,
    icon: String,
    entries: Vec<String>,
}

impl ItemGroupBuilder {
    /// The text on the tab; when absent it is made from the id (`ruby_tab` becomes `Ruby Tab`).
    pub fn title(mut self, title: &str) -> Self {
        self.title = title.to_string();
        self
    }
    /// Adds one item id to the end of the tab.
    pub fn entry(mut self, entry: &str) -> Self {
        self.entries.push(entry.to_string());
        self
    }

    /// The item shown on the tab (required).
    pub fn icon(mut self, icon: &str) -> Self {
        self.icon = icon.to_string();
        self
    }

    /// Adds item ids to the end of the tab.
    pub fn entries<I>(mut self, entries: I) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        self.entries
            .extend(entries.into_iter().map(|e| e.as_ref().to_string()));
        self
    }

    /// Registers the tab.
    pub fn register(self) -> Result<ItemGroup, Error> {
        let (svc, host_ptr) = service::<ItemGroupServiceV1>(SERVICE_ITEM_GROUP)?;
        let table = host::get()?;
        let list = strs(&self.entries);
        // SAFETY: valid service table; the strings and the list outlive the call.
        let status = unsafe {
            (svc.register)(
                host_ptr,
                table.mod_token,
                FishStr::new(&self.id),
                FishStr::new(&self.title),
                FishStr::new(&self.icon),
                list.as_ptr(),
                list.len(),
            )
        };
        if status == FishStatus::INVALID_ARGUMENT && crate::registry::is_frozen()? {
            return Err(Error::InvalidArgument("registries are frozen".to_string()));
        }
        check("registering an item group", status)?;
        Ok(ItemGroup { id: self.id })
    }
}

fn strs(entries: &[String]) -> Vec<FishStr> {
    entries.iter().map(|e| FishStr::new(e)).collect()
}

/// A registered creative tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemGroup {
    id: String,
}

impl ItemGroup {
    /// Starts the tab `id` (`namespace:path`, the namespace is the mod's id).
    pub fn builder(id: &str) -> ItemGroupBuilder {
        ItemGroupBuilder {
            id: id.to_string(),
            title: String::new(),
            icon: String::new(),
            entries: Vec::new(),
        }
    }

    /// The tab's id (`mymod:ruby_tab`).
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Adds items to tabs.
pub struct ItemGroups;

impl ItemGroups {
    /// Adds the item ids `entries` to the end of the tab `group`: a vanilla one
    /// (`minecraft:ingredients`) or one a mod registered. A tab that does not exist is logged
    /// when the creative inventory is built.
    pub fn modify_entries<I>(group: &str, entries: I) -> Result<(), Error>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let (svc, host_ptr) = service::<ItemGroupServiceV1>(SERVICE_ITEM_GROUP)?;
        let table = host::get()?;
        let owned: Vec<String> = entries
            .into_iter()
            .map(|e| e.as_ref().to_string())
            .collect();
        let list = strs(&owned);
        // SAFETY: valid service table; the strings and the list outlive the call.
        let status = unsafe {
            (svc.modify_entries)(
                host_ptr,
                table.mod_token,
                FishStr::new(group),
                list.as_ptr(),
                list.len(),
            )
        };
        if status == FishStatus::INVALID_ARGUMENT && crate::registry::is_frozen()? {
            return Err(Error::InvalidArgument("registries are frozen".to_string()));
        }
        check("adding creative entries", status)
    }
}
