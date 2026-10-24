use std::path::{Path, PathBuf};

use ksni::menu::{CheckmarkItem, StandardItem, SubMenu};
use ksni::{Category, MenuItem, ToolTip};
use tokio::sync::mpsc::UnboundedSender;

use crate::menu::{self, Action, Entry, HelperView};
use crate::state::Dot;

/// Installed into the hicolor theme by `just install`.
const ICON: &str = "switchboard-symbolic";
/// Breeze's horizontal sliders, for a run from source before the icon is
/// installed. Asking for a name no theme has leaves a blank tray slot.
const FALLBACK_ICON: &str = "adjustlevels-symbolic";

pub struct SwitchTray {
    pub helpers: Vec<HelperView>,
    pub config_error: Option<String>,
    icon: &'static str,
    actions: UnboundedSender<Action>,
}

impl SwitchTray {
    pub fn new(actions: UnboundedSender<Action>) -> Self {
        Self {
            helpers: Vec::new(),
            config_error: None,
            icon: icon_for(&data_dirs()),
            actions,
        }
    }
}

/// `$XDG_DATA_HOME` then `$XDG_DATA_DIRS`, with the spec's defaults.
fn data_dirs() -> Vec<PathBuf> {
    let var = |name| std::env::var_os(name).filter(|v| !v.is_empty());
    let home = var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(var("HOME").unwrap_or_default()).join(".local/share"));
    let system = var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    std::iter::once(home)
        .chain(std::env::split_paths(&system))
        .collect()
}

fn icon_for(data_dirs: &[PathBuf]) -> &'static str {
    let installed = |dir: &Path| {
        dir.join("icons/hicolor/scalable/apps")
            .join(format!("{ICON}.svg"))
            .is_file()
    };
    if data_dirs.iter().any(|d| installed(d)) {
        ICON
    } else {
        FALLBACK_ICON
    }
}

/// dbusmenu reads `_` as a mnemonic marker; double it to show it literally.
fn escape(label: &str) -> String {
    label.replace('_', "__")
}

fn activator(action: Option<Action>) -> Box<dyn Fn(&mut SwitchTray) + Send> {
    Box::new(move |tray: &mut SwitchTray| {
        if let Some(action) = &action {
            let _ = tray.actions.send(action.clone());
        }
    })
}

fn to_ksni(entries: &[Entry]) -> Vec<MenuItem<SwitchTray>> {
    entries
        .iter()
        .map(|entry| match entry {
            Entry::Item { label, action } => StandardItem {
                label: escape(label),
                enabled: action.is_some(),
                activate: activator(action.clone()),
                ..Default::default()
            }
            .into(),
            Entry::Check {
                label,
                checked,
                action,
            } => CheckmarkItem {
                label: escape(label),
                checked: *checked,
                enabled: action.is_some(),
                activate: activator(action.clone()),
                ..Default::default()
            }
            .into(),
            Entry::Separator => MenuItem::Separator,
            Entry::Submenu { label, entries } => SubMenu {
                label: escape(label),
                submenu: to_ksni(entries),
                ..Default::default()
            }
            .into(),
        })
        .collect()
}

impl ksni::Tray for SwitchTray {
    // Left click opens the menu too; there is no window to activate.
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "switchboard".into()
    }

    fn title(&self) -> String {
        "Switchboard".into()
    }

    fn category(&self) -> Category {
        Category::SystemServices
    }

    fn icon_name(&self) -> String {
        self.icon.into()
    }

    fn overlay_icon_name(&self) -> String {
        let failed = self.helpers.iter().any(|h| h.dot == Dot::Failed);
        if failed || self.config_error.is_some() {
            "emblem-error".into()
        } else {
            String::new()
        }
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "Switchboard".into(),
            description: menu::summary(&self.helpers),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        to_ksni(&menu::build(&self.helpers, self.config_error.as_deref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn underscores_are_literal() {
        assert_eq!(escape("llama_server"), "llama__server");
    }

    #[test]
    fn installed_icon_is_preferred() {
        let root = std::env::temp_dir().join(format!("switchboard-icon-{}", std::process::id()));
        let empty = root.join("empty");
        let data = root.join("data");
        let apps = data.join("icons/hicolor/scalable/apps");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::create_dir_all(&empty).unwrap();

        assert_eq!(icon_for(std::slice::from_ref(&empty)), FALLBACK_ICON);
        std::fs::write(apps.join("switchboard-symbolic.svg"), "<svg/>").unwrap();
        assert_eq!(icon_for(&[empty, data]), ICON);

        std::fs::remove_dir_all(&root).unwrap();
    }
}
