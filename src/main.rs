mod actions;
mod config;
mod menu;
mod state;
mod systemd;
mod tray;

use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use ksni::TrayMethods;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::task::JoinHandle;

use crate::config::Helper;
use crate::menu::{Action, HelperView};
use crate::state::{Dot, LoginState};
use crate::systemd::Systemd;
use crate::tray::SwitchTray;

/// A tray switch for systemd user units. systemd supervises; quitting
/// Switchboard leaves every unit as it is.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Config file [default: $XDG_CONFIG_HOME/switchboard/helpers.toml]
    #[arg(long)]
    config: Option<PathBuf>,
}

/// What changed on the systemd side.
#[derive(Debug, Clone)]
enum Change {
    Unit(String),
    All,
}

#[derive(Default)]
struct HelperState {
    dot: Option<Dot>,
    login: Option<LoginState>,
    /// For a socket helper, the service the socket triggers.
    service: Option<String>,
    read_error: Option<String>,
    action_error: Option<String>,
}

struct App {
    sd: Systemd,
    config_path: PathBuf,
    helpers: Vec<Helper>,
    config_error: Option<String>,
    states: HashMap<String, HelperState>,
    watchers: Vec<JoinHandle<()>>,
    changes: UnboundedSender<Change>,
}

fn error_text(e: &anyhow::Error) -> String {
    format!("{e:#}")
}

impl App {
    fn helper(&self, id: &str) -> Option<&Helper> {
        self.helpers.iter().find(|h| h.id == id)
    }

    async fn load_config(&mut self) {
        for w in self.watchers.drain(..) {
            w.abort();
        }
        self.states.clear();
        match config::load(&self.config_path) {
            Ok(config) => {
                self.helpers = config.helpers;
                self.config_error = None;
            }
            Err(e) => {
                tracing::error!(path = %self.config_path.display(), "{}", error_text(&e));
                self.helpers.clear();
                self.config_error = Some(error_text(&e));
            }
        }
        for h in self.helpers.clone() {
            self.watch(&h).await;
            self.refresh(&h.id).await;
        }
    }

    /// Subscribes to the helper's unit, and for a socket to the service it
    /// triggers as well.
    async fn watch(&mut self, h: &Helper) {
        let mut units = vec![h.unit.clone()];
        let state = self.states.entry(h.id.clone()).or_default();
        if h.is_socket() {
            match self.sd.unit(&h.unit).await {
                Ok(info) => {
                    state.service = info.triggers.into_iter().find(|t| t.ends_with(".service"));
                    units.extend(state.service.clone());
                }
                Err(e) => state.read_error = Some(error_text(&e)),
            }
        }
        for unit in units {
            match self.sd.unit(&unit).await {
                Ok(info) => self.watchers.push(self.sd.watch_unit(
                    info.path,
                    Change::Unit(h.id.clone()),
                    self.changes.clone(),
                )),
                Err(e) => tracing::warn!("watching {unit}: {}", error_text(&e)),
            }
        }
    }

    async fn refresh(&mut self, id: &str) {
        let Some(h) = self.helper(id).cloned() else {
            return;
        };
        let service = self.states.get(id).and_then(|s| s.service.clone());
        let read = async {
            let unit = self.sd.unit(&h.unit).await?;
            let service = match &service {
                Some(name) => Some(self.sd.unit(name).await?.props),
                None => None,
            };
            Ok::<_, anyhow::Error>((
                state::dot(&unit.props, service.as_ref()),
                state::login_state(&unit.props),
            ))
        };
        let result = read.await;
        let state = self.states.entry(h.id.clone()).or_default();
        match result {
            Ok((dot, login)) => {
                state.dot = Some(dot);
                state.login = Some(login);
                state.read_error = None;
            }
            Err(e) => {
                state.dot = Some(Dot::NotFound);
                state.read_error = Some(error_text(&e));
            }
        }
    }

    async fn refresh_all(&mut self) {
        let ids: Vec<String> = self.helpers.iter().map(|h| h.id.clone()).collect();
        for id in ids {
            self.refresh(&id).await;
        }
    }

    async fn run(&mut self, action: &Action) -> Result<()> {
        let id = match action {
            Action::OpenConfigDir => {
                let dir = config::config_dir();
                std::fs::create_dir_all(&dir)
                    .with_context(|| format!("creating {}", dir.display()))?;
                return actions::spawn(&actions::open_argv(&dir.to_string_lossy()));
            }
            Action::ReloadConfig => {
                self.load_config().await;
                return Ok(());
            }
            Action::Quit => return Ok(()),
            Action::Start(id)
            | Action::Stop(id)
            | Action::Restart(id)
            | Action::SetLogin(id, _)
            | Action::OpenUrl(id)
            | Action::ShowLogs(id) => id,
        };
        // The menu can be a step behind a config reload.
        let Some(h) = self.helper(id).cloned() else {
            return Ok(());
        };
        let service = self.states.get(id).and_then(|s| s.service.clone());

        let result = match action {
            Action::Start(_) => self.sd.start(&h.unit).await,
            Action::Stop(_) => {
                async {
                    // Socket first, so a connection can't start the service again
                    // between the two stops.
                    self.sd.stop(&h.unit).await?;
                    if let Some(service) = &service {
                        self.sd.stop(service).await?;
                    }
                    Ok(())
                }
                .await
            }
            Action::Restart(_) => self.sd.restart(service.as_deref().unwrap_or(&h.unit)).await,
            Action::SetLogin(_, on) => self.sd.set_enabled(&h.unit, *on).await,
            Action::OpenUrl(_) => match &h.url {
                Some(url) => actions::spawn(&actions::open_argv(url)),
                None => Ok(()),
            },
            Action::ShowLogs(_) => {
                let mut units = vec![h.unit.as_str()];
                units.extend(service.as_deref());
                actions::spawn(&actions::logs_argv(&units))
            }
            Action::OpenConfigDir | Action::ReloadConfig | Action::Quit => unreachable!(),
        };

        let state = self.states.entry(h.id.clone()).or_default();
        match &result {
            Ok(()) => state.action_error = None,
            Err(e) => {
                tracing::warn!("{}: {}", h.id, error_text(e));
                state.action_error = Some(error_text(e));
            }
        }
        self.refresh(&h.id).await;
        Ok(())
    }

    fn views(&self) -> Vec<HelperView> {
        self.helpers
            .iter()
            .map(|h| {
                let state = self.states.get(&h.id);
                HelperView {
                    id: h.id.clone(),
                    name: h.name.clone(),
                    dot: state.and_then(|s| s.dot).unwrap_or(Dot::NotFound),
                    login: state.and_then(|s| s.login).unwrap_or(LoginState::Fixed),
                    has_url: h.url.is_some(),
                    error: state.and_then(|s| s.read_error.clone().or(s.action_error.clone())),
                }
            })
            .collect()
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "switchboard=info".into()),
        )
        .without_time()
        .with_ansi(std::io::stderr().is_terminal())
        .init();

    let conn = zbus::Connection::session()
        .await
        .context("connecting to the session bus")?;
    let sd = Systemd::connect(&conn).await?;

    let (action_tx, mut action_rx) = mpsc::unbounded_channel();
    let (change_tx, mut change_rx) = mpsc::unbounded_channel();
    let _manager_watch = sd.watch_manager(Change::All, change_tx.clone());

    let mut app = App {
        sd,
        config_path: cli.config.unwrap_or_else(config::default_path),
        helpers: Vec::new(),
        config_error: None,
        states: HashMap::new(),
        watchers: Vec::new(),
        changes: change_tx,
    };
    app.load_config().await;

    let handle = SwitchTray::new(action_tx)
        .spawn()
        .await
        .context("registering the tray icon")?;

    loop {
        let (views, config_error) = (app.views(), app.config_error.clone());
        handle
            .update(move |tray| {
                tray.helpers = views;
                tray.config_error = config_error;
            })
            .await;

        tokio::select! {
            Some(action) = action_rx.recv() => {
                if action == Action::Quit {
                    break;
                }
                if let Err(e) = app.run(&action).await {
                    tracing::warn!("{}", error_text(&e));
                }
            }
            Some(change) = change_rx.recv() => match change {
                Change::Unit(id) => app.refresh(&id).await,
                Change::All => app.refresh_all().await,
            },
        }
    }

    handle.shutdown().await;
    Ok(())
}
