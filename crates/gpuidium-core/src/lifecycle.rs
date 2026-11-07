//! Pure rules for unloading inactive apps (LIFE-010).

use std::time::Duration;

use crate::model::AppId;

/// Apps whose native view may be closed: not the active one, and idle for longer than
/// `limit`. `idle` pairs each loaded app with how long ago it was last active
/// (`None` = unknown, never unloaded).
pub fn due_for_unload(
    idle: impl IntoIterator<Item = (AppId, Option<Duration>)>,
    active: Option<AppId>,
    limit: Duration,
) -> Vec<AppId> {
    idle.into_iter()
        .filter(|(id, d)| Some(*id) != active && d.is_some_and(|d| d > limit))
        .map(|(id, _)| id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_unloads_active_unknown_or_recent() {
        let (a, b, c, d) = (AppId::new(), AppId::new(), AppId::new(), AppId::new());
        let limit = Duration::from_secs(600);
        let idle = vec![
            (a, Some(Duration::from_secs(9999))), // active: kept
            (b, Some(Duration::from_secs(9999))), // idle: unloaded
            (c, Some(Duration::from_secs(10))),   // recent: kept
            (d, None),                            // unknown: kept
        ];
        assert_eq!(due_for_unload(idle, Some(a), limit), vec![b]);
    }

    #[test]
    fn exactly_at_limit_is_kept() {
        let a = AppId::new();
        let l = Duration::from_secs(60);
        assert!(due_for_unload([(a, Some(l))], None, l).is_empty());
    }
}
