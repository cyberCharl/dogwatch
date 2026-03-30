use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: Config,
    pub paths: RuntimePaths,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub drydock: DrydockConfig,
    pub notifications: NotificationsConfig,
    pub limits: LimitsConfig,
    pub paths: PathsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            drydock: DrydockConfig::default(),
            notifications: NotificationsConfig::default(),
            limits: LimitsConfig::default(),
            paths: PathsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DrydockConfig {
    pub api_url: String,
}

impl Default for DrydockConfig {
    fn default() -> Self {
        Self {
            api_url: "http://localhost:3000".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct NotificationsConfig {
    pub telegram_target: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LimitsConfig {
    pub max_retries: u32,
    pub stale_cycles: u32,
    pub log_max_rows: u32,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_retries: 6,
            stale_cycles: 3,
            log_max_rows: 5000,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PathsConfig {
    pub acpx_sessions: PathBuf,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            acpx_sessions: default_acpx_sessions(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimePaths {
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub database_file: PathBuf,
    pub pause_file: PathBuf,
}

impl LoadedConfig {
    pub fn load() -> Result<Self> {
        let paths = RuntimePaths::discover()?;
        Self::load_from_paths(paths)
    }

    pub fn load_from_paths(paths: RuntimePaths) -> Result<Self> {
        fs::create_dir_all(&paths.data_dir)
            .with_context(|| format!("creating data dir {}", paths.data_dir.display()))?;

        let config = if paths.config_file.exists() {
            let raw = fs::read_to_string(&paths.config_file)
                .with_context(|| format!("reading {}", paths.config_file.display()))?;
            toml::from_str::<Config>(&raw)
                .with_context(|| format!("parsing {}", paths.config_file.display()))?
        } else {
            Config::default()
        };

        Ok(Self { config, paths })
    }

    #[cfg(test)]
    pub fn load_from_str(raw: &str, paths: RuntimePaths) -> Result<Self> {
        fs::create_dir_all(&paths.data_dir)
            .with_context(|| format!("creating data dir {}", paths.data_dir.display()))?;
        let config = toml::from_str::<Config>(raw).context("parsing config string")?;
        Ok(Self { config, paths })
    }
}

impl RuntimePaths {
    pub fn discover() -> Result<Self> {
        let home_dir = env::var("HOME")
            .map(PathBuf::from)
            .context("HOME is not set; cannot resolve XDG paths")?;
        let config_home = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir.join(".config"));
        let data_home = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir.join(".local/share"));

        Ok(Self::from_homes(home_dir, config_home, data_home))
    }

    pub fn from_homes(_home_dir: PathBuf, config_home: PathBuf, data_home: PathBuf) -> Self {
        let config_dir = config_home.join("dogwatch");
        let data_dir = data_home.join("dogwatch");

        Self {
            config_file: config_dir.join("config.toml"),
            database_file: data_dir.join("dogwatch.db"),
            pause_file: data_dir.join("paused"),
            data_dir,
        }
    }
}

fn default_acpx_sessions() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".acpx")
        .join("sessions")
}

pub fn local_pause_enabled(paths: &RuntimePaths) -> bool {
    paths.pause_file.exists()
}

pub fn set_local_pause(paths: &RuntimePaths, paused: bool) -> Result<()> {
    if paused {
        fs::write(&paths.pause_file, b"paused\n")
            .with_context(|| format!("writing {}", paths.pause_file.display()))?;
    } else if paths.pause_file.exists() {
        fs::remove_file(&paths.pause_file)
            .with_context(|| format!("removing {}", paths.pause_file.display()))?;
    }

    Ok(())
}

pub fn config_display_path(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{LoadedConfig, RuntimePaths};

    #[test]
    fn parses_config_from_toml_with_defaults() {
        let root = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_homes(
            PathBuf::from("/tmp/home"),
            root.path().join("config"),
            root.path().join("data"),
        );

        let loaded = LoadedConfig::load_from_str(
            r#"
                [notifications]
                telegram_target = "charl"

                [limits]
                max_retries = 9
            "#,
            paths.clone(),
        )
        .unwrap();

        assert_eq!(
            loaded.config.notifications.telegram_target.as_deref(),
            Some("charl")
        );
        assert_eq!(loaded.config.limits.max_retries, 9);
        assert_eq!(loaded.config.limits.stale_cycles, 3);
        assert_eq!(loaded.config.drydock.api_url, "http://localhost:3000");
        assert!(loaded.paths.data_dir.exists());
    }

    #[test]
    fn loads_defaults_when_config_file_missing() {
        let root = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_homes(
            PathBuf::from("/tmp/home"),
            root.path().join("config"),
            root.path().join("data"),
        );

        let loaded = LoadedConfig::load_from_paths(paths.clone()).unwrap();

        assert_eq!(loaded.config.limits.log_max_rows, 5000);
        assert_eq!(loaded.paths.config_file, paths.config_file);
        assert!(fs::metadata(paths.data_dir).is_ok());
    }
}
