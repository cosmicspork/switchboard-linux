# Switchboard for Linux

A small tray app that switches systemd user units on and off. systemd
supervises them; Switchboard is just the switch, so quitting it leaves every
unit exactly as it was.

It is the Linux counterpart of [Switchboard](https://github.com/cosmicspork/switchboard)
for macOS, which does the same for launchd agents. The two share an idea and a
config shape, not code.

## What it does

Each configured helper gets a submenu in the tray:

- a state: running, listening (socket up, service not started yet), stopped,
  failed, starting, stopping, or not found
- Start, Stop and Restart
- Start at login, which enables or disables the unit
- Open in browser, for helpers with a `url`
- Show logs, which follows `journalctl --user` for the unit in your terminal

State is live: Switchboard subscribes to the systemd user manager over D-Bus,
so a unit started or stopped from a shell shows up in the menu right away. The
tray icon gets an error overlay when a helper has failed.

## Requirements

- Linux with a systemd user session.
- A tray that speaks StatusNotifierItem: KDE Plasma natively, GNOME with the
  AppIndicator extension.
- `xdg-open`, and `xdg-terminal-exec` for Show logs.
- A Rust toolchain (`stable`) to build it.

## Install

```sh
just install     # cargo install to ~/.local/bin, then enable the user unit
just uninstall   # stop and remove the tray; the units it switches stay as they are
```

`just install` installs [`contrib/systemd/switchboard.service`](contrib/systemd/switchboard.service),
which starts with `graphical-session.target` and restarts on failure, and the
tray icon, [`contrib/icons/switchboard-symbolic.svg`](contrib/icons/switchboard-symbolic.svg),
into `~/.local/share/icons/hicolor`. Running it again replaces both and restarts
the tray.

To try it from source without installing: `just run`, or
`just run --config path/to/helpers.toml`. Until the icon is installed, the tray uses
Breeze's `adjustlevels-symbolic` instead.

## Configuration

Switchboard reads `~/.config/switchboard/helpers.toml` (or
`$XDG_CONFIG_HOME/switchboard/helpers.toml`). A missing file is fine: the menu
says there are no helpers and offers to open the config folder. After editing,
use **Reload config** in the menu.

```toml
[[helper]]
id = "tracon"
name = "Tracon"
unit = "tracon.service"
url = "http://127.0.0.1:7420"
```

- `id`: unique within the file.
- `name`: what the menu shows.
- `unit`: a `.service`, or a `.socket` for a socket-activated helper.
- `url`: optional; adds Open in browser.

See [`helpers.example.toml`](helpers.example.toml). Helper *types* live in the
code; *which* helpers exist is configuration, so nothing personal is committed
here.

### Socket-activated helpers

Point `unit` at the `.socket`. Switchboard finds the service the socket
triggers and treats the two as one helper:

- Start starts the socket. The helper reads *listening* until the first
  connection starts the service, then *running*.
- Stop stops the socket first, then the service, so a connection in between
  can't start it again.
- Restart restarts the service; Start at login enables the socket.
- Open in browser is available while listening, since the request is what
  starts the app.

A tool documents how it runs in its own repository (`contrib/systemd/`), so
it doesn't depend on Switchboard.

## How it works

- `systemd.rs`: the `org.freedesktop.systemd1` session-bus API through
  `zbus`: load, start, stop, restart, enable, disable, and property-change
  subscriptions.
- `state.rs`: maps systemd's `ActiveState`/`LoadState`/`UnitFileState` (and a
  socket's triggered service) to what the menu shows.
- `menu.rs`: the menu as plain data, built from that state.
- `tray.rs`: translates the menu into a StatusNotifierItem through `ksni`.

Everything with a rule in it is a pure function with unit tests; the D-Bus and
tray layers only carry data.

## Development

```sh
just check   # cargo fmt --check, clippy -D warnings, cargo test
```

## Not in scope

Supervising processes, restart policy, and health checks belong to systemd.
Remote control belongs elsewhere.

## License

MIT. See [LICENSE](LICENSE).
