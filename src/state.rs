/// The properties read from one systemd unit. Strings are systemd's own
/// values (`ActiveState`, `SubState`, `LoadState`, `UnitFileState`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnitProps {
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
    pub unit_file_state: String,
}

impl UnitProps {
    fn is_loaded(&self) -> bool {
        self.load_state == "loaded"
    }
}

/// What a helper's menu row shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dot {
    Active,
    /// A socket helper whose socket is listening and whose service is not
    /// running yet.
    Listening,
    Inactive,
    Failed,
    Starting,
    Stopping,
    /// The unit file is missing, masked, or failed to parse.
    NotFound,
}

impl Dot {
    pub fn glyph(self) -> &'static str {
        match self {
            Dot::Active => "●",
            Dot::Listening => "◉",
            Dot::Inactive => "○",
            Dot::Failed => "✕",
            Dot::Starting | Dot::Stopping => "◌",
            Dot::NotFound => "?",
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Dot::Active => "running",
            Dot::Listening => "listening",
            Dot::Inactive => "stopped",
            Dot::Failed => "failed",
            Dot::Starting => "starting",
            Dot::Stopping => "stopping",
            Dot::NotFound => "not found",
        }
    }

    /// Whether Start applies. A failed unit can be started again.
    pub fn can_start(self) -> bool {
        matches!(self, Dot::Inactive | Dot::Failed)
    }

    /// Whether Stop applies. A listening socket counts as running: stopping
    /// it is how a socket-activated helper is switched off.
    pub fn can_stop(self) -> bool {
        matches!(
            self,
            Dot::Active | Dot::Listening | Dot::Starting | Dot::Failed
        )
    }
}

fn dot_for(unit: &UnitProps) -> Dot {
    if !unit.is_loaded() {
        return Dot::NotFound;
    }
    match unit.active_state.as_str() {
        "active" | "reloading" | "refreshing" => Dot::Active,
        "failed" => Dot::Failed,
        "activating" => Dot::Starting,
        "deactivating" => Dot::Stopping,
        _ => Dot::Inactive,
    }
}

/// `service` is the unit a `.socket` triggers; it is `None` for a plain
/// service helper.
pub fn dot(unit: &UnitProps, service: Option<&UnitProps>) -> Dot {
    let own = dot_for(unit);
    let Some(service) = service else {
        return own;
    };
    if own == Dot::NotFound {
        return own;
    }
    match (own, dot_for(service)) {
        (Dot::Failed, _) | (_, Dot::Failed) => Dot::Failed,
        (_, Dot::Active) => Dot::Active,
        (_, Dot::Starting) => Dot::Starting,
        (_, Dot::Stopping) => Dot::Stopping,
        (Dot::Active, _) => Dot::Listening,
        (other, _) => other,
    }
}

/// How the "Start at login" item looks for a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginState {
    Enabled,
    Disabled,
    /// `static`, `generated`, `transient`, `masked` and so on: enabling is
    /// not something a switch can change.
    Fixed,
}

pub fn login_state(unit: &UnitProps) -> LoginState {
    match unit.unit_file_state.as_str() {
        "enabled" | "enabled-runtime" => LoginState::Enabled,
        "disabled" | "linked" | "linked-runtime" => LoginState::Disabled,
        _ => LoginState::Fixed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(active: &str) -> UnitProps {
        UnitProps {
            load_state: "loaded".into(),
            active_state: active.into(),
            sub_state: String::new(),
            unit_file_state: "enabled".into(),
        }
    }

    #[test]
    fn service_states() {
        assert_eq!(dot(&unit("active"), None), Dot::Active);
        assert_eq!(dot(&unit("reloading"), None), Dot::Active);
        assert_eq!(dot(&unit("inactive"), None), Dot::Inactive);
        assert_eq!(dot(&unit("failed"), None), Dot::Failed);
        assert_eq!(dot(&unit("activating"), None), Dot::Starting);
        assert_eq!(dot(&unit("deactivating"), None), Dot::Stopping);
    }

    #[test]
    fn missing_unit_is_not_found() {
        let missing = UnitProps {
            load_state: "not-found".into(),
            active_state: "inactive".into(),
            ..UnitProps::default()
        };
        assert_eq!(dot(&missing, None), Dot::NotFound);
        assert_eq!(dot(&missing, Some(&unit("inactive"))), Dot::NotFound);
    }

    #[test]
    fn socket_listening_until_service_runs() {
        assert_eq!(
            dot(&unit("active"), Some(&unit("inactive"))),
            Dot::Listening
        );
        assert_eq!(dot(&unit("active"), Some(&unit("active"))), Dot::Active);
        assert_eq!(
            dot(&unit("active"), Some(&unit("activating"))),
            Dot::Starting
        );
        assert_eq!(
            dot(&unit("inactive"), Some(&unit("inactive"))),
            Dot::Inactive
        );
    }

    #[test]
    fn socket_failure_from_either_side() {
        assert_eq!(dot(&unit("failed"), Some(&unit("inactive"))), Dot::Failed);
        assert_eq!(dot(&unit("active"), Some(&unit("failed"))), Dot::Failed);
    }

    #[test]
    fn service_without_unit_file_yet_still_shows_socket() {
        // The triggered service is loaded on demand; before that it reads as
        // not-found, which must not hide a listening socket.
        let unloaded = UnitProps {
            load_state: "not-found".into(),
            ..UnitProps::default()
        };
        assert_eq!(dot(&unit("active"), Some(&unloaded)), Dot::Listening);
    }

    #[test]
    fn login_states() {
        let with = |s: &str| UnitProps {
            unit_file_state: s.into(),
            ..unit("active")
        };
        assert_eq!(login_state(&with("enabled")), LoginState::Enabled);
        assert_eq!(login_state(&with("disabled")), LoginState::Disabled);
        assert_eq!(login_state(&with("static")), LoginState::Fixed);
        assert_eq!(login_state(&with("generated")), LoginState::Fixed);
        assert_eq!(login_state(&with("")), LoginState::Fixed);
    }

    #[test]
    fn actions_by_state() {
        assert!(Dot::Inactive.can_start() && !Dot::Inactive.can_stop());
        assert!(Dot::Failed.can_start() && Dot::Failed.can_stop());
        assert!(!Dot::Listening.can_start() && Dot::Listening.can_stop());
        assert!(!Dot::NotFound.can_start() && !Dot::NotFound.can_stop());
    }
}
