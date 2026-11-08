//! Unread badges for the sidebar and the Dock, derived from what pages already expose:
//! most web apps put an unread count in the document title ("(3) WhatsApp",
//! "Inbox (12) - Gmail"), and some mark unread-without-count with a leading "*" or "•"
//! (Mattermost). A notification that arrived while the app was in the background shows a
//! dot until the app is opened.

/// What the sidebar shows on an app's icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Badge {
    #[default]
    None,
    /// Something new, no count known.
    Dot,
    Count(u32),
}

impl Badge {
    /// Text for a count badge ("1".."99", then "99+"); `None` for a dot or no badge.
    pub fn label(self) -> Option<String> {
        match self {
            Badge::Count(n) if n > 99 => Some("99+".into()),
            Badge::Count(n) => Some(n.to_string()),
            _ => None,
        }
    }
}

/// The badge a page title implies.
pub fn from_title(title: &str) -> Badge {
    let t = title.trim();
    if let Some(n) = bracketed_count(t) {
        return if n > 0 { Badge::Count(n) } else { Badge::None };
    }
    if t.starts_with("* ") || t.starts_with('•') {
        return Badge::Dot;
    }
    Badge::None
}

/// First "(N)" or "[N]" (also "(99+)") anywhere in the title.
fn bracketed_count(t: &str) -> Option<u32> {
    let b = t.as_bytes();
    for (i, &open) in b.iter().enumerate() {
        let close = match open {
            b'(' => b')',
            b'[' => b']',
            _ => continue,
        };
        let rest = &t[i + 1..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || digits > 6 {
            continue;
        }
        let mut after = rest[digits..].bytes();
        let next = after.next();
        let ok = next == Some(close) || (next == Some(b'+') && after.next() == Some(close));
        if ok {
            return rest[..digits].parse().ok();
        }
    }
    None
}

/// The title's badge, or a dot for an unseen notification when the title has none.
pub fn combine(title: Badge, unseen_notification: bool) -> Badge {
    match title {
        Badge::None if unseen_notification => Badge::Dot,
        b => b,
    }
}

/// Dock / taskbar label for all apps: the summed counts ("99+" cap), "•" if there are only
/// dots, `None` when nothing is unread.
pub fn total_label(badges: impl IntoIterator<Item = Badge>) -> Option<String> {
    let (mut sum, mut dot) = (0u32, false);
    for b in badges {
        match b {
            Badge::Count(n) => sum = sum.saturating_add(n),
            Badge::Dot => dot = true,
            Badge::None => {}
        }
    }
    if sum > 0 {
        Badge::Count(sum).label()
    } else if dot {
        Some("•".into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_from_common_title_formats() {
        assert_eq!(from_title("(3) WhatsApp"), Badge::Count(3));
        assert_eq!(
            from_title("Inbox (12) - me@example.org - Gmail"),
            Badge::Count(12)
        );
        assert_eq!(from_title("(2) Town Square - Team"), Badge::Count(2));
        assert_eq!(from_title("[5] Chat"), Badge::Count(5));
        assert_eq!(from_title("(99+) Feed"), Badge::Count(99));
        assert_eq!(from_title("(0) Inbox"), Badge::None);
    }

    #[test]
    fn dots_and_plain_titles() {
        assert_eq!(from_title("* Town Square - Team"), Badge::Dot);
        assert_eq!(from_title("• Chat"), Badge::Dot);
        assert_eq!(from_title("WhatsApp"), Badge::None);
        assert_eq!(from_title(""), Badge::None);
    }

    #[test]
    fn ignores_brackets_that_are_not_counts() {
        assert_eq!(from_title("Release notes (beta) for v2"), Badge::None);
        assert_eq!(from_title("Meeting (10:30)"), Badge::None);
        assert_eq!(from_title("Year (2024 edition)"), Badge::None);
        assert_eq!(from_title("Ref (1234567)"), Badge::None);
        assert_eq!(from_title("Docs (v2) (4)"), Badge::Count(4));
    }

    #[test]
    fn labels_cap_at_99() {
        assert_eq!(Badge::Count(7).label().as_deref(), Some("7"));
        assert_eq!(Badge::Count(120).label().as_deref(), Some("99+"));
        assert_eq!(Badge::Dot.label(), None);
    }

    #[test]
    fn notification_dot_only_when_title_has_nothing() {
        assert_eq!(combine(Badge::None, true), Badge::Dot);
        assert_eq!(combine(Badge::Count(2), true), Badge::Count(2));
        assert_eq!(combine(Badge::None, false), Badge::None);
    }

    #[test]
    fn total_sums_counts_or_shows_a_dot() {
        assert_eq!(
            total_label([Badge::Count(3), Badge::Dot, Badge::Count(4)]).as_deref(),
            Some("7")
        );
        assert_eq!(total_label([Badge::Dot, Badge::None]).as_deref(), Some("•"));
        assert_eq!(total_label([Badge::None]), None);
        assert_eq!(
            total_label([Badge::Count(90), Badge::Count(20)]).as_deref(),
            Some("99+")
        );
    }
}
