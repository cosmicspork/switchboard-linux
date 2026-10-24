//! The two desktop actions that run a program: opening a URL and following a
//! helper's journal in a terminal.

use std::io::ErrorKind;

use anyhow::{Result, anyhow};

pub fn open_argv(target: &str) -> Vec<String> {
    vec!["xdg-open".into(), target.into()]
}

/// `xdg-terminal-exec` opens the user's preferred terminal (Konsole on
/// Plasma) and runs the command in it.
pub fn logs_argv(units: &[&str]) -> Vec<String> {
    let mut argv: Vec<String> = [
        "xdg-terminal-exec",
        "journalctl",
        "--user",
        "-n",
        "100",
        "-f",
    ]
    .map(String::from)
    .into();
    for unit in units {
        argv.push("-u".into());
        argv.push((*unit).into());
    }
    argv
}

/// Starts `argv` detached and reaps it in the background, so a launcher that
/// exits doesn't linger as a zombie.
pub fn spawn(argv: &[String]) -> Result<()> {
    let (program, args) = argv.split_first().ok_or_else(|| anyhow!("empty command"))?;
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => anyhow!("{program} is not installed"),
            _ => anyhow!("running {program}: {e}"),
        })?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_passes_target_as_one_argument() {
        assert_eq!(
            open_argv("http://127.0.0.1:7420/?a=1&b=2"),
            ["xdg-open", "http://127.0.0.1:7420/?a=1&b=2"]
        );
    }

    #[test]
    fn logs_follow_every_unit() {
        assert_eq!(
            logs_argv(&["hoja.socket", "hoja.service"]),
            [
                "xdg-terminal-exec",
                "journalctl",
                "--user",
                "-n",
                "100",
                "-f",
                "-u",
                "hoja.socket",
                "-u",
                "hoja.service"
            ]
        );
    }

    #[tokio::test]
    async fn missing_program_is_a_readable_error() {
        let err = spawn(&["switchboard-no-such-program".into()]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "switchboard-no-such-program is not installed"
        );
    }
}
