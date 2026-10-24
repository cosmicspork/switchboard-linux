use ksni::menu::{CheckmarkItem, StandardItem, SubMenu};
use ksni::{Category, MenuItem, ToolTip};
use tokio::sync::mpsc::UnboundedSender;

use crate::menu::{self, Action, Entry, HelperView};
use crate::state::Dot;

pub struct SwitchTray {
    pub helpers: Vec<HelperView>,
    pub config_error: Option<String>,
    actions: UnboundedSender<Action>,
}

impl SwitchTray {
    pub fn new(actions: UnboundedSender<Action>) -> Self {
        Self {
            helpers: Vec::new(),
            config_error: None,
            actions,
        }
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
        "network-connect-symbolic".into()
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
}
