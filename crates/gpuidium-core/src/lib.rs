//! GUI-free domain logic for GPUIdium: typed app state, URL policy and storage.

pub mod model;
pub mod paths;
pub mod storage;
pub mod uri;

pub use model::{AppEntry, AppId, AppState, Command, Event, ProfileId, RemoveMode};
pub use uri::{NormalizedUrl, UriError, normalize_input};
