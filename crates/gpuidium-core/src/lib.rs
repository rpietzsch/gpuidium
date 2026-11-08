//! GUI-free domain logic for GPUIdium: typed app state, URL policy and storage.

pub mod badge;
pub mod diagnostics;
pub mod exchange;
pub mod instance;
pub mod lifecycle;
pub mod model;
pub mod paths;
pub mod policy;
pub mod profiles;
pub mod spell;
pub mod storage;
pub mod uri;
pub mod zoom;

pub use model::{AppEntry, AppId, AppState, Command, Event, ProfileId, RemoveMode};
pub use uri::{NormalizedUrl, UriError, normalize_input};
