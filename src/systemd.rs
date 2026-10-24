//! The systemd user manager over the session bus. Switchboard never
//! supervises anything itself; every state change here is a request to
//! systemd.

use anyhow::{Context, Result};
use futures_util::StreamExt;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, proxy};

use crate::state::UnitProps;

#[proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait Manager {
    fn subscribe(&self) -> zbus::Result<()>;
    fn load_unit(&self, name: &str) -> zbus::Result<OwnedObjectPath>;
    fn start_unit(&self, name: &str, mode: &str) -> zbus::Result<OwnedObjectPath>;
    fn stop_unit(&self, name: &str, mode: &str) -> zbus::Result<OwnedObjectPath>;
    fn restart_unit(&self, name: &str, mode: &str) -> zbus::Result<OwnedObjectPath>;
    #[allow(clippy::type_complexity)]
    fn enable_unit_files(
        &self,
        files: &[&str],
        runtime: bool,
        force: bool,
    ) -> zbus::Result<(bool, Vec<(String, String, String)>)>;
    fn disable_unit_files(
        &self,
        files: &[&str],
        runtime: bool,
    ) -> zbus::Result<Vec<(String, String, String)>>;
    fn reload(&self) -> zbus::Result<()>;

    #[zbus(signal)]
    fn unit_files_changed(&self) -> zbus::Result<()>;
    #[zbus(signal)]
    fn reloading(&self, active: bool) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.systemd1.Unit",
    default_service = "org.freedesktop.systemd1"
)]
trait Unit {
    #[zbus(property)]
    fn load_state(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn active_state(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn sub_state(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn unit_file_state(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn triggers(&self) -> zbus::Result<Vec<String>>;
}

/// One unit's current properties and object path.
pub struct UnitInfo {
    pub path: OwnedObjectPath,
    pub props: UnitProps,
    pub triggers: Vec<String>,
}

#[derive(Clone)]
pub struct Systemd {
    conn: Connection,
    manager: ManagerProxy<'static>,
}

/// "replace" is what `systemctl start` uses.
const MODE: &str = "replace";

impl Systemd {
    pub async fn connect(conn: &Connection) -> Result<Self> {
        let manager = ManagerProxy::new(conn).await?;
        // Without a subscriber the manager doesn't emit unit signals at all.
        manager
            .subscribe()
            .await
            .context("subscribing to the systemd user manager")?;
        Ok(Self {
            conn: conn.clone(),
            manager,
        })
    }

    /// Loads the unit (so a missing file reads as `not-found` rather than
    /// failing) and reads its properties fresh, bypassing zbus's cache.
    pub async fn unit(&self, name: &str) -> Result<UnitInfo> {
        let path = self.manager.load_unit(name).await?;
        let unit = UnitProxy::builder(&self.conn)
            .path(path.clone())?
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        let props = UnitProps {
            load_state: unit.load_state().await?,
            active_state: unit.active_state().await?,
            sub_state: unit.sub_state().await?,
            unit_file_state: unit.unit_file_state().await?,
        };
        let triggers = unit.triggers().await.unwrap_or_default();
        Ok(UnitInfo {
            path,
            props,
            triggers,
        })
    }

    pub async fn start(&self, name: &str) -> Result<()> {
        self.manager.start_unit(name, MODE).await?;
        Ok(())
    }

    pub async fn stop(&self, name: &str) -> Result<()> {
        self.manager.stop_unit(name, MODE).await?;
        Ok(())
    }

    pub async fn restart(&self, name: &str) -> Result<()> {
        self.manager.restart_unit(name, MODE).await?;
        Ok(())
    }

    /// Same as `systemctl --user enable|disable`, including the reload.
    pub async fn set_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        if enabled {
            self.manager
                .enable_unit_files(&[name], false, false)
                .await?;
        } else {
            self.manager.disable_unit_files(&[name], false).await?;
        }
        self.manager.reload().await?;
        Ok(())
    }

    /// Sends `id` on every property change of the unit at `path`.
    pub fn watch_unit<T: Clone + Send + 'static>(
        &self,
        path: OwnedObjectPath,
        id: T,
        tx: UnboundedSender<T>,
    ) -> JoinHandle<()> {
        let conn = self.conn.clone();
        tokio::spawn(async move {
            let stream = async {
                let props = zbus::fdo::PropertiesProxy::builder(&conn)
                    .destination("org.freedesktop.systemd1")?
                    .path(path.clone())?
                    .build()
                    .await?;
                props.receive_properties_changed().await
            };
            let mut stream = match stream.await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("watching {}: {e}", path.as_str());
                    return;
                }
            };
            while stream.next().await.is_some() {
                if tx.send(id.clone()).is_err() {
                    return;
                }
            }
        })
    }

    /// Sends `msg` whenever enablement may have changed or the manager
    /// finished a daemon-reload, either of which can change any unit.
    pub fn watch_manager<T: Clone + Send + 'static>(
        &self,
        msg: T,
        tx: UnboundedSender<T>,
    ) -> JoinHandle<()> {
        let manager = self.manager.clone();
        tokio::spawn(async move {
            let streams = async {
                Ok::<_, zbus::Error>((
                    manager.receive_unit_files_changed().await?,
                    manager.receive_reloading().await?,
                ))
            };
            let (mut files, mut reloading) = match streams.await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("watching the systemd manager: {e}");
                    return;
                }
            };
            loop {
                let fire = tokio::select! {
                    s = files.next() => s.is_some(),
                    s = reloading.next() => match s {
                        // Refresh once the reload is done, not as it starts.
                        Some(sig) => sig.args().map(|a| !a.active).unwrap_or(true),
                        None => false,
                    },
                };
                if fire && tx.send(msg.clone()).is_err() {
                    return;
                }
            }
        })
    }
}
