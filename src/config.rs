use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default, rename = "helper")]
    pub helpers: Vec<Helper>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Helper {
    pub id: String,
    pub name: String,
    /// A `.service`, or a `.socket` for a socket-activated helper.
    pub unit: String,
    pub url: Option<String>,
}

impl Helper {
    pub fn is_socket(&self) -> bool {
        self.unit.ends_with(".socket")
    }
}

/// `$XDG_CONFIG_HOME/switchboard`, falling back to `~/.config/switchboard`.
pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(".config")
        });
    base.join("switchboard")
}

pub fn default_path() -> PathBuf {
    config_dir().join("helpers.toml")
}

/// A missing file is an empty config, not an error: the tray still runs and
/// offers to open the config folder.
pub fn load(path: &Path) -> Result<Config> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    // The file name, not the whole path, so the message fits a menu line.
    let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy();
    parse(&text).with_context(|| name.into_owned())
}

pub fn parse(text: &str) -> Result<Config> {
    // toml's own Display spans several lines with a source excerpt; keep the
    // line number and the message.
    let config: Config = toml::from_str(text).map_err(|e| {
        let line = e
            .span()
            .map(|span| text[..span.start].matches('\n').count() + 1);
        match line {
            Some(line) => anyhow!("line {line}: {}", e.message()),
            None => anyhow!("{}", e.message()),
        }
    })?;
    validate(&config)?;
    Ok(config)
}

fn validate(config: &Config) -> Result<()> {
    let mut ids = HashSet::new();
    for h in &config.helpers {
        if h.id.trim().is_empty() {
            bail!("a helper has an empty id");
        }
        if !ids.insert(h.id.as_str()) {
            bail!("helper id {:?} is used more than once", h.id);
        }
        if !(h.unit.ends_with(".service") || h.unit.ends_with(".socket")) {
            bail!(
                "helper {:?}: unit {:?} must be a .service or .socket",
                h.id,
                h.unit
            );
        }
        if let Some(url) = &h.url
            && !(url.starts_with("http://") || url.starts_with("https://"))
        {
            bail!("helper {:?}: url {:?} must be http(s)", h.id, url);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_helpers() {
        let config = parse(
            r#"
            [[helper]]
            id = "tracon"
            name = "Tracon"
            unit = "tracon.service"
            url = "http://127.0.0.1:7420"

            [[helper]]
            id = "hoja"
            name = "hoja"
            unit = "hoja.socket"
            "#,
        )
        .unwrap();
        assert_eq!(config.helpers.len(), 2);
        assert_eq!(
            config.helpers[0].url.as_deref(),
            Some("http://127.0.0.1:7420")
        );
        assert!(!config.helpers[0].is_socket());
        assert!(config.helpers[1].is_socket());
        assert_eq!(config.helpers[1].url, None);
    }

    #[test]
    fn empty_file_is_empty_config() {
        assert_eq!(parse("").unwrap(), Config::default());
    }

    #[test]
    fn missing_file_is_empty_config() {
        let config = load(Path::new("/nonexistent/switchboard/helpers.toml")).unwrap();
        assert!(config.helpers.is_empty());
    }

    #[test]
    fn rejects_duplicate_ids() {
        let err = parse(
            r#"
            [[helper]]
            id = "a"
            name = "A"
            unit = "a.service"
            [[helper]]
            id = "a"
            name = "A again"
            unit = "b.service"
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("more than once"), "{err}");
    }

    #[test]
    fn rejects_other_unit_types() {
        let err = parse(
            r#"
            [[helper]]
            id = "t"
            name = "T"
            unit = "t.timer"
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains(".service or .socket"), "{err}");
    }

    #[test]
    fn rejects_non_http_url() {
        let err = parse(
            r#"
            [[helper]]
            id = "t"
            name = "T"
            unit = "t.service"
            url = "file:///etc/passwd"
            "#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("http(s)"), "{err}");
    }

    #[test]
    fn toml_errors_are_one_line_with_a_line_number() {
        let err = parse("[[helper]]\nid = \"t\"\nname = \"T\"\nunit = \"t.service\"\noops = 1\n")
            .unwrap_err()
            .to_string();
        assert!(err.starts_with("line 5: unknown field `oops`"), "{err}");
        assert!(!err.contains('\n'), "{err}");
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(
            parse(
                r#"
                [[helper]]
                id = "t"
                name = "T"
                unit = "t.service"
                lable = "typo"
                "#,
            )
            .is_err()
        );
    }
}
