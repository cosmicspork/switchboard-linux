unit_dir := env_var('HOME') / ".config/systemd/user"

default:
    @just --list

# Run from source against the real user manager.
run *args:
    cargo run -- {{args}}

format:
    cargo fmt

format-check:
    cargo fmt --check

lint:
    cargo clippy --all-targets -- -D warnings

test:
    cargo test

check: format-check lint test

# Install to ~/.local/bin and start the tray with the graphical session.
install:
    cargo install --locked --path . --root ~/.local
    install -Dm644 contrib/systemd/switchboard.service {{unit_dir}}/switchboard.service
    systemctl --user daemon-reload
    systemctl --user enable --now switchboard.service

# Stops and removes the tray only; the units it switches are left as they are.
uninstall:
    -systemctl --user disable --now switchboard.service
    rm -f {{unit_dir}}/switchboard.service
    systemctl --user daemon-reload
    cargo uninstall --root ~/.local switchboard
