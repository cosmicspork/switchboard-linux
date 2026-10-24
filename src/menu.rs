//! The menu as plain data, so its rules are testable without a tray host.
//! `tray.rs` only translates [`Entry`] into ksni items.

use crate::state::{Dot, LoginState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperView {
    pub id: String,
    pub name: String,
    pub dot: Dot,
    pub login: LoginState,
    pub has_url: bool,
    /// The last failed action or read, shown until the next one succeeds.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Start(String),
    Stop(String),
    Restart(String),
    SetLogin(String, bool),
    OpenUrl(String),
    ShowLogs(String),
    OpenConfigDir,
    ReloadConfig,
    Quit,
}

/// An item with no action is shown disabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Item {
        label: String,
        action: Option<Action>,
    },
    Check {
        label: String,
        checked: bool,
        action: Option<Action>,
    },
    Separator,
    Submenu {
        label: String,
        entries: Vec<Entry>,
    },
}

fn item(label: impl Into<String>, action: Option<Action>) -> Entry {
    Entry::Item {
        label: label.into(),
        action,
    }
}

const ERROR_MAX_CHARS: usize = 80;

fn shorten(msg: &str) -> String {
    let line = msg.lines().next().unwrap_or_default();
    if line.chars().count() <= ERROR_MAX_CHARS {
        line.to_owned()
    } else {
        let cut: String = line.chars().take(ERROR_MAX_CHARS - 1).collect();
        format!("{cut}…")
    }
}

pub fn helper_label(h: &HelperView) -> String {
    format!("{} {} ({})", h.dot.glyph(), h.name, h.dot.word())
}

fn helper_entries(h: &HelperView) -> Vec<Entry> {
    let id = || h.id.clone();
    let when = |ok: bool, action: Action| ok.then_some(action);
    let mut entries = Vec::new();

    if let Some(err) = &h.error {
        entries.push(item(format!("Error: {}", shorten(err)), None));
        entries.push(Entry::Separator);
    }

    entries.push(item("Start", when(h.dot.can_start(), Action::Start(id()))));
    entries.push(item("Stop", when(h.dot.can_stop(), Action::Stop(id()))));
    entries.push(item(
        "Restart",
        when(h.dot == Dot::Active, Action::Restart(id())),
    ));
    entries.push(Entry::Separator);

    let checked = h.login == LoginState::Enabled;
    entries.push(Entry::Check {
        label: "Start at login".into(),
        checked,
        action: when(
            h.login != LoginState::Fixed,
            Action::SetLogin(id(), !checked),
        ),
    });
    entries.push(Entry::Separator);

    if h.has_url {
        // A listening socket starts the app on the first request, so opening
        // the page is how a socket-activated helper is woken.
        let reachable = matches!(h.dot, Dot::Active | Dot::Listening);
        entries.push(item(
            "Open in browser",
            when(reachable, Action::OpenUrl(id())),
        ));
    }
    entries.push(item("Show logs", Some(Action::ShowLogs(id()))));
    entries
}

pub fn build(helpers: &[HelperView], config_error: Option<&str>) -> Vec<Entry> {
    let mut entries = Vec::new();

    if let Some(err) = config_error {
        entries.push(item(format!("Config error: {}", shorten(err)), None));
        entries.push(Entry::Separator);
    } else if helpers.is_empty() {
        entries.push(item("No helpers configured", None));
        entries.push(Entry::Separator);
    }

    for h in helpers {
        entries.push(Entry::Submenu {
            label: helper_label(h),
            entries: helper_entries(h),
        });
    }
    if !helpers.is_empty() {
        entries.push(Entry::Separator);
    }

    entries.push(item("Open config folder", Some(Action::OpenConfigDir)));
    entries.push(item("Reload config", Some(Action::ReloadConfig)));
    entries.push(Entry::Separator);
    entries.push(item("Quit", Some(Action::Quit)));
    entries
}

/// One line for the tray tooltip, e.g. "2 running, 1 failed".
pub fn summary(helpers: &[HelperView]) -> String {
    let count = |f: fn(Dot) -> bool| helpers.iter().filter(|h| f(h.dot)).count();
    let running = count(|d| d == Dot::Active);
    let listening = count(|d| d == Dot::Listening);
    let failed = count(|d| d == Dot::Failed);
    let mut parts = vec![format!("{running} running")];
    if listening > 0 {
        parts.push(format!("{listening} listening"));
    }
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(dot: Dot) -> HelperView {
        HelperView {
            id: "t".into(),
            name: "Tracon".into(),
            dot,
            login: LoginState::Enabled,
            has_url: true,
            error: None,
        }
    }

    fn submenu(entries: &[Entry]) -> &[Entry] {
        entries
            .iter()
            .find_map(|e| match e {
                Entry::Submenu { entries, .. } => Some(entries.as_slice()),
                _ => None,
            })
            .expect("a helper submenu")
    }

    fn action_of<'a>(entries: &'a [Entry], label: &str) -> Option<&'a Action> {
        entries
            .iter()
            .find_map(|e| match e {
                Entry::Item { label: l, action } if l == label => Some(action.as_ref()),
                Entry::Check {
                    label: l, action, ..
                } if l == label => Some(action.as_ref()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no item {label:?}"))
    }

    #[test]
    fn running_helper_offers_stop_restart_and_url() {
        let menu = build(&[view(Dot::Active)], None);
        let sub = submenu(&menu);
        assert_eq!(action_of(sub, "Start"), None);
        assert_eq!(action_of(sub, "Stop"), Some(&Action::Stop("t".into())));
        assert_eq!(
            action_of(sub, "Restart"),
            Some(&Action::Restart("t".into()))
        );
        assert_eq!(
            action_of(sub, "Open in browser"),
            Some(&Action::OpenUrl("t".into()))
        );
        assert_eq!(
            action_of(sub, "Start at login"),
            Some(&Action::SetLogin("t".into(), false))
        );
    }

    #[test]
    fn stopped_helper_offers_start_only() {
        let menu = build(&[view(Dot::Inactive)], None);
        let sub = submenu(&menu);
        assert_eq!(action_of(sub, "Start"), Some(&Action::Start("t".into())));
        assert_eq!(action_of(sub, "Stop"), None);
        assert_eq!(action_of(sub, "Restart"), None);
        assert_eq!(action_of(sub, "Open in browser"), None);
        assert_eq!(
            action_of(sub, "Show logs"),
            Some(&Action::ShowLogs("t".into()))
        );
    }

    #[test]
    fn listening_socket_can_be_opened_and_stopped() {
        let menu = build(&[view(Dot::Listening)], None);
        let sub = submenu(&menu);
        assert_eq!(action_of(sub, "Stop"), Some(&Action::Stop("t".into())));
        assert!(action_of(sub, "Open in browser").is_some());
    }

    #[test]
    fn fixed_login_state_is_disabled() {
        let mut h = view(Dot::Active);
        h.login = LoginState::Fixed;
        let menu = build(&[h], None);
        assert_eq!(action_of(submenu(&menu), "Start at login"), None);
    }

    #[test]
    fn disabled_login_toggles_on() {
        let mut h = view(Dot::Active);
        h.login = LoginState::Disabled;
        let menu = build(&[h], None);
        assert_eq!(
            action_of(submenu(&menu), "Start at login"),
            Some(&Action::SetLogin("t".into(), true))
        );
    }

    #[test]
    fn no_url_no_browser_item() {
        let mut h = view(Dot::Active);
        h.has_url = false;
        let menu = build(&[h], None);
        assert!(
            !submenu(&menu)
                .iter()
                .any(|e| matches!(e, Entry::Item { label, .. } if label == "Open in browser"))
        );
    }

    #[test]
    fn error_is_shown_first_and_shortened() {
        let mut h = view(Dot::Failed);
        h.error = Some("x".repeat(200));
        let menu = build(&[h], None);
        match &submenu(&menu)[0] {
            Entry::Item { label, action } => {
                assert!(label.starts_with("Error: "));
                assert!(label.chars().count() <= "Error: ".len() + ERROR_MAX_CHARS);
                assert_eq!(action, &None);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn label_carries_state() {
        assert_eq!(helper_label(&view(Dot::Failed)), "✕ Tracon (failed)");
    }

    #[test]
    fn empty_and_broken_configs_say_so() {
        let empty = build(&[], None);
        assert!(matches!(&empty[0], Entry::Item { label, .. } if label == "No helpers configured"));
        let broken = build(&[], Some("bad toml\nmore detail"));
        assert!(
            matches!(&broken[0], Entry::Item { label, .. } if label == "Config error: bad toml")
        );
        assert!(matches!(
            broken.last(),
            Some(Entry::Item {
                action: Some(Action::Quit),
                ..
            })
        ));
    }

    #[test]
    fn summary_counts() {
        let mut failed = view(Dot::Failed);
        failed.id = "f".into();
        assert_eq!(
            summary(&[view(Dot::Active), view(Dot::Listening), failed]),
            "1 running, 1 listening, 1 failed"
        );
        assert_eq!(summary(&[]), "0 running");
    }
}
