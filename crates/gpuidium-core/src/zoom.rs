//! Per-app page zoom (Ctrl/Cmd + `+` / `-` / `0`), kept in whole percent so repeated
//! steps never drift.

/// What a zoom shortcut asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomChange {
    In,
    Out,
    Reset,
}

pub const DEFAULT: u16 = 100;
pub const STEP: u16 = 10;
pub const MIN: u16 = 30;
pub const MAX: u16 = 300;

/// Clamp to `MIN..=MAX` and snap to the step grid (also cleans hand-edited config values).
pub fn sanitize(percent: u16) -> u16 {
    let snapped = percent.saturating_add(STEP / 2) / STEP * STEP;
    snapped.clamp(MIN, MAX)
}

/// The zoom after applying `change` to `current`, in 10 % steps within `MIN..=MAX`.
pub fn apply(current: u16, change: ZoomChange) -> u16 {
    let cur = sanitize(current);
    match change {
        ZoomChange::In => (cur + STEP).min(MAX),
        ZoomChange::Out => cur.saturating_sub(STEP).max(MIN),
        ZoomChange::Reset => DEFAULT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_by_ten_percent() {
        assert_eq!(apply(100, ZoomChange::In), 110);
        assert_eq!(apply(110, ZoomChange::In), 120);
        assert_eq!(apply(100, ZoomChange::Out), 90);
        assert_eq!(apply(90, ZoomChange::Out), 80);
        assert_eq!(apply(170, ZoomChange::Reset), 100);
    }

    #[test]
    fn many_steps_return_exactly() {
        let mut z = DEFAULT;
        for _ in 0..7 {
            z = apply(z, ZoomChange::In);
        }
        for _ in 0..7 {
            z = apply(z, ZoomChange::Out);
        }
        assert_eq!(z, DEFAULT);
    }

    #[test]
    fn stays_within_bounds() {
        assert_eq!(apply(MAX, ZoomChange::In), MAX);
        assert_eq!(apply(MIN, ZoomChange::Out), MIN);
        assert_eq!(apply(0, ZoomChange::Out), MIN);
        assert_eq!(apply(u16::MAX, ZoomChange::In), MAX);
    }

    #[test]
    fn sanitize_snaps_off_grid_values() {
        assert_eq!(sanitize(104), 100);
        assert_eq!(sanitize(105), 110);
        assert_eq!(sanitize(5), MIN);
        assert_eq!(sanitize(1000), MAX);
    }
}
