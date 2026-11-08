//! Typed application state, commands and events (CORE-001).
//!
//! Pure data and transitions: no WebView or GPUI types live here.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::uri::{UriError, normalize_input};

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);
        impl $name {
            pub fn new() -> Self { Self(Uuid::new_v4()) }
            pub fn as_uuid(&self) -> Uuid { self.0 }
            pub fn parse(s: &str) -> Option<Self> { Uuid::parse_str(s.trim()).ok().map(Self) }
        }
        impl Default for $name { fn default() -> Self { Self::new() } }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) }
        }
    };
}

id_type!(
    /// Stable identity of one app entry.
    AppId
);
id_type!(
    /// Opaque identity of the browser profile owned by exactly one app entry.
    /// Never derived from URL, host or name.
    ProfileId
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppEntry {
    pub id: AppId,
    pub name: String,
    pub start_url: String,
    pub profile_id: ProfileId,
    pub order: u32,
}

/// What to do with browsing data when an app is removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RemoveMode {
    /// Default: keep the profile data on disk (orphaned, purgeable later).
    #[default]
    RetainData,
    /// Explicitly delete only this app's profile data.
    DeleteData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Add { name: String, input: String },
    Remove { id: AppId, mode: RemoveMode },
    Select(AppId),
    Rename { id: AppId, name: String },
    SetUrl { id: AppId, input: String },
    MoveUp(AppId),
    MoveDown(AppId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Added(AppId),
    Removed {
        id: AppId,
        profile_id: ProfileId,
        mode: RemoveMode,
    },
    Selected(Option<AppId>),
    Renamed(AppId),
    UrlChanged(AppId),
    Reordered,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StateError {
    #[error("unknown app {0}")]
    UnknownApp(AppId),
    #[error(transparent)]
    Uri(#[from] UriError),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppState {
    apps: Vec<AppEntry>,
    active: Option<AppId>,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from persisted data; callers must have validated it (see storage).
    pub fn from_parts(mut apps: Vec<AppEntry>, active: Option<AppId>) -> Self {
        apps.sort_by_key(|a| a.order);
        let active = active.filter(|id| apps.iter().any(|a| a.id == *id));
        let mut s = Self { apps, active };
        s.renumber();
        s
    }

    pub fn apps(&self) -> &[AppEntry] {
        &self.apps
    }

    pub fn active(&self) -> Option<AppId> {
        self.active
    }

    pub fn get(&self, id: AppId) -> Option<&AppEntry> {
        self.apps.iter().find(|a| a.id == id)
    }

    fn index(&self, id: AppId) -> Result<usize, StateError> {
        self.apps
            .iter()
            .position(|a| a.id == id)
            .ok_or(StateError::UnknownApp(id))
    }

    fn renumber(&mut self) {
        for (i, a) in self.apps.iter_mut().enumerate() {
            a.order = i as u32;
        }
    }

    pub fn apply(&mut self, cmd: Command) -> Result<Vec<Event>, StateError> {
        match cmd {
            Command::Add { name, input } => {
                // Duplicate URLs are allowed; each entry gets a fresh profile.
                let url = normalize_input(&input)?;
                let name = if name.trim().is_empty() {
                    url.url().host_str().unwrap_or("App").to_owned()
                } else {
                    name.trim().to_owned()
                };
                let id = AppId::new();
                self.apps.push(AppEntry {
                    id,
                    name,
                    start_url: url.as_str().to_owned(),
                    profile_id: ProfileId::new(),
                    order: self.apps.len() as u32,
                });
                let mut ev = vec![Event::Added(id)];
                if self.active.is_none() {
                    self.active = Some(id);
                    ev.push(Event::Selected(Some(id)));
                }
                Ok(ev)
            }
            Command::Remove { id, mode } => {
                let i = self.index(id)?;
                let removed = self.apps.remove(i);
                self.renumber();
                let mut ev = vec![Event::Removed {
                    id,
                    profile_id: removed.profile_id,
                    mode,
                }];
                if self.active == Some(id) {
                    // Prefer the entry that took its slot, else the previous one.
                    self.active = self.apps.get(i).or_else(|| self.apps.last()).map(|a| a.id);
                    ev.push(Event::Selected(self.active));
                }
                Ok(ev)
            }
            Command::Select(id) => {
                self.index(id)?;
                self.active = Some(id);
                Ok(vec![Event::Selected(Some(id))])
            }
            Command::Rename { id, name } => {
                let i = self.index(id)?;
                self.apps[i].name = name.trim().to_owned();
                Ok(vec![Event::Renamed(id)])
            }
            Command::SetUrl { id, input } => {
                let i = self.index(id)?;
                self.apps[i].start_url = normalize_input(&input)?.as_str().to_owned();
                Ok(vec![Event::UrlChanged(id)])
            }
            Command::MoveUp(id) => {
                let i = self.index(id)?;
                if i > 0 {
                    self.apps.swap(i, i - 1);
                    self.renumber();
                }
                Ok(vec![Event::Reordered])
            }
            Command::MoveDown(id) => {
                let i = self.index(id)?;
                if i + 1 < self.apps.len() {
                    self.apps.swap(i, i + 1);
                    self.renumber();
                }
                Ok(vec![Event::Reordered])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn add(s: &mut AppState, name: &str, url: &str) -> AppId {
        match s
            .apply(Command::Add {
                name: name.into(),
                input: url.into(),
            })
            .unwrap()[0]
        {
            Event::Added(id) => id,
            _ => unreachable!(),
        }
    }

    #[test]
    fn first_add_becomes_active() {
        let mut s = AppState::new();
        let id = add(&mut s, "A", "example.com");
        assert_eq!(s.active(), Some(id));
        let ev = s
            .apply(Command::Add {
                name: "B".into(),
                input: "example.org".into(),
            })
            .unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(s.active(), Some(id));
    }

    #[test]
    fn duplicate_urls_get_distinct_ids_and_profiles() {
        let mut s = AppState::new();
        let a = add(&mut s, "A", "https://chat.example.org");
        let b = add(&mut s, "B", "https://chat.example.org");
        assert_ne!(a, b);
        let (pa, pb) = (s.get(a).unwrap().profile_id, s.get(b).unwrap().profile_id);
        assert_ne!(pa, pb);
        assert_eq!(s.get(a).unwrap().start_url, s.get(b).unwrap().start_url);
    }

    #[test]
    fn empty_name_defaults_to_host() {
        let mut s = AppState::new();
        let id = add(&mut s, "  ", "https://chat.example.org/x");
        assert_eq!(s.get(id).unwrap().name, "chat.example.org");
    }

    #[test]
    fn invalid_url_rejected_without_change() {
        let mut s = AppState::new();
        let err = s.apply(Command::Add {
            name: "x".into(),
            input: "javascript:1".into(),
        });
        assert!(matches!(err, Err(StateError::Uri(_))));
        assert!(s.apps().is_empty());
    }

    #[test]
    fn remove_reports_profile_and_mode_and_moves_selection() {
        let mut s = AppState::new();
        let a = add(&mut s, "A", "a.example");
        let b = add(&mut s, "B", "b.example");
        let pa = s.get(a).unwrap().profile_id;
        let ev = s
            .apply(Command::Remove {
                id: a,
                mode: RemoveMode::RetainData,
            })
            .unwrap();
        assert_eq!(
            ev[0],
            Event::Removed {
                id: a,
                profile_id: pa,
                mode: RemoveMode::RetainData
            }
        );
        assert_eq!(s.active(), Some(b));
        assert_eq!(s.get(b).unwrap().order, 0);
        // Re-adding allocates a fresh profile; old one never reattached.
        let a2 = add(&mut s, "A", "a.example");
        assert_ne!(s.get(a2).unwrap().profile_id, pa);
    }

    #[test]
    fn remove_last_clears_selection() {
        let mut s = AppState::new();
        let a = add(&mut s, "A", "a.example");
        s.apply(Command::Remove {
            id: a,
            mode: RemoveMode::DeleteData,
        })
        .unwrap();
        assert_eq!(s.active(), None);
    }

    #[test]
    fn reorder_keeps_dense_stable_order() {
        let mut s = AppState::new();
        let a = add(&mut s, "A", "a.example");
        let b = add(&mut s, "B", "b.example");
        let c = add(&mut s, "C", "c.example");
        s.apply(Command::MoveUp(c)).unwrap();
        s.apply(Command::MoveUp(a)).unwrap(); // already first: no-op
        let ids: Vec<_> = s.apps().iter().map(|x| x.id).collect();
        assert_eq!(ids, vec![a, c, b]);
        let orders: Vec<_> = s.apps().iter().map(|x| x.order).collect();
        assert_eq!(orders, vec![0, 1, 2]);
        s.apply(Command::MoveDown(b)).unwrap(); // already last: no-op
        assert_eq!(s.apps().last().unwrap().id, b);
    }

    #[test]
    fn unknown_ids_error() {
        let mut s = AppState::new();
        let ghost = AppId::new();
        assert_eq!(
            s.apply(Command::Select(ghost)),
            Err(StateError::UnknownApp(ghost))
        );
    }

    #[test]
    fn profile_ids_unique_across_many_adds() {
        let mut s = AppState::new();
        for i in 0..50 {
            add(&mut s, "x", &format!("h{i}.example"));
        }
        let set: HashSet<_> = s.apps().iter().map(|a| a.profile_id).collect();
        assert_eq!(set.len(), 50);
    }
}
