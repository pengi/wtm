//! Configuration: where the pool lives.
//!
//! Each setting comes from, in order of precedence: a command line argument,
//! an environment variable, the config file, or a built-in default.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};

pub const ENV_CONFIG: &str = "WTM_CONFIG";
pub const ENV_REPOS_DIR: &str = "WTM_REPOS_DIR";
pub const ENV_CHECKOUTS_DIR: &str = "WTM_CHECKOUTS_DIR";

/// Where a setting's value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Default,
    File(PathBuf),
    Env(&'static str),
    Argument,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Default => f.write_str("default"),
            Source::File(path) => write!(f, "config file {}", path.display()),
            Source::Env(var) => write!(f, "environment {var}"),
            Source::Argument => f.write_str("argument"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting<T> {
    pub value: T,
    pub source: Source,
}

/// Values given on the command line.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub config_file: Option<PathBuf>,
    pub repos_dir: Option<PathBuf>,
    pub checkouts_dir: Option<PathBuf>,
}

/// The system locations the defaults are based on.
#[derive(Debug, Clone)]
pub struct BaseDirs {
    pub home: PathBuf,
    /// `$XDG_CONFIG_HOME`, usually `~/.config`.
    pub config: PathBuf,
    /// `$XDG_DATA_HOME`, usually `~/.local/share`.
    pub data: PathBuf,
}

impl BaseDirs {
    pub fn from_system() -> Result<Self> {
        use etcetera::BaseStrategy;
        let strategy = etcetera::choose_base_strategy()
            .map_err(|e| Error::InvalidPath(format!("cannot find home directory: {e}")))?;
        Ok(BaseDirs {
            home: strategy.home_dir().to_path_buf(),
            config: strategy.config_dir(),
            data: strategy.data_dir(),
        })
    }
}

/// The contents of `config.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    repos_dir: Option<String>,
    checkouts_dir: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// The config file that applies, and whether it exists.
    pub file: Setting<PathBuf>,
    pub file_exists: bool,
    /// Directory of the bare repositories.
    pub repos_dir: Setting<PathBuf>,
    /// Directory of the worktrees.
    pub checkouts_dir: Setting<PathBuf>,
}

impl Config {
    /// Load the configuration from the real environment.
    pub fn load(overrides: &Overrides) -> Result<Self> {
        let base = BaseDirs::from_system()?;
        let cwd = std::env::current_dir().map_err(Error::io("."))?;
        Self::resolve(overrides, |var| std::env::var_os(var), &base, &cwd)
    }

    /// Resolve the configuration from explicit inputs.
    pub fn resolve(
        overrides: &Overrides,
        env: impl Fn(&str) -> Option<OsString>,
        base: &BaseDirs,
        cwd: &Path,
    ) -> Result<Self> {
        let given = |arg: &Option<PathBuf>, var: &'static str| -> Option<Setting<PathBuf>> {
            if let Some(path) = arg {
                return Some(Setting {
                    value: absolute(path, base, cwd),
                    source: Source::Argument,
                });
            }
            env(var).filter(|v| !v.is_empty()).map(|v| Setting {
                value: absolute(Path::new(&v), base, cwd),
                source: Source::Env(var),
            })
        };

        let explicit_file = given(&overrides.config_file, ENV_CONFIG);
        let file = explicit_file.clone().unwrap_or_else(|| Setting {
            value: base.config.join("wtm").join("config.toml"),
            source: Source::Default,
        });
        let content = match std::fs::read_to_string(&file.value) {
            Ok(content) => Some(content),
            // A missing default config file is fine; a missing explicit one
            // is a mistake.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && explicit_file.is_none() => None,
            Err(e) => return Err(Error::io(&file.value)(e)),
        };
        let file_exists = content.is_some();
        let parsed: FileConfig = match &content {
            Some(content) => toml::from_str(content).map_err(|e| Error::Config {
                path: file.value.clone(),
                message: e.to_string(),
            })?,
            None => FileConfig::default(),
        };

        let from_file = |value: &Option<String>, key: &str| -> Result<Option<Setting<PathBuf>>> {
            let Some(value) = value else { return Ok(None) };
            let path = expand_home(Path::new(value), base);
            if path.is_relative() {
                return Err(Error::Config {
                    path: file.value.clone(),
                    message: format!("{key} must be an absolute path or start with ~/"),
                });
            }
            Ok(Some(Setting {
                value: path,
                source: Source::File(file.value.clone()),
            }))
        };
        let default = |value: PathBuf| Setting {
            value,
            source: Source::Default,
        };

        let repos_dir = match given(&overrides.repos_dir, ENV_REPOS_DIR) {
            Some(s) => s,
            None => from_file(&parsed.repos_dir, "repos_dir")?
                .unwrap_or_else(|| default(base.data.join("wtm").join("repos"))),
        };
        let checkouts_dir = match given(&overrides.checkouts_dir, ENV_CHECKOUTS_DIR) {
            Some(s) => s,
            None => from_file(&parsed.checkouts_dir, "checkouts_dir")?
                .unwrap_or_else(|| default(base.home.join("wt"))),
        };

        Ok(Config {
            file,
            file_exists,
            repos_dir,
            checkouts_dir,
        })
    }
}

fn expand_home(path: &Path, base: &BaseDirs) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => base.home.join(rest),
        Err(_) => path.to_path_buf(),
    }
}

fn absolute(path: &Path, base: &BaseDirs, cwd: &Path) -> PathBuf {
    let path = expand_home(path, base);
    if path.is_relative() {
        cwd.join(path)
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(root: &Path) -> BaseDirs {
        BaseDirs {
            home: root.join("home"),
            config: root.join("home/.config"),
            data: root.join("home/.local/share"),
        }
    }

    fn no_env(_: &str) -> Option<OsString> {
        None
    }

    #[test]
    fn defaults_without_config_file() {
        let tmp = tempfile::tempdir().unwrap();
        let base = base(tmp.path());
        let config = Config::resolve(&Overrides::default(), no_env, &base, tmp.path()).unwrap();
        assert!(!config.file_exists);
        assert_eq!(config.repos_dir.value, base.data.join("wtm/repos"));
        assert_eq!(config.repos_dir.source, Source::Default);
        assert_eq!(config.checkouts_dir.value, base.home.join("wt"));
    }

    #[test]
    fn precedence_argument_env_file() {
        let tmp = tempfile::tempdir().unwrap();
        let base = base(tmp.path());
        let file = base.config.join("wtm/config.toml");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "repos_dir = \"~/r\"\ncheckouts_dir = \"/c\"\n").unwrap();

        let config = Config::resolve(&Overrides::default(), no_env, &base, tmp.path()).unwrap();
        assert_eq!(config.repos_dir.value, base.home.join("r"));
        assert_eq!(config.repos_dir.source, Source::File(file.clone()));
        assert_eq!(config.checkouts_dir.value, PathBuf::from("/c"));

        let env = |var: &str| (var == ENV_REPOS_DIR).then(|| OsString::from("/env"));
        let config = Config::resolve(&Overrides::default(), env, &base, tmp.path()).unwrap();
        assert_eq!(config.repos_dir.value, PathBuf::from("/env"));
        assert_eq!(config.repos_dir.source, Source::Env(ENV_REPOS_DIR));

        let overrides = Overrides {
            repos_dir: Some("rel".into()),
            ..Default::default()
        };
        let config = Config::resolve(&overrides, env, &base, Path::new("/cwd")).unwrap();
        assert_eq!(config.repos_dir.value, PathBuf::from("/cwd/rel"));
        assert_eq!(config.repos_dir.source, Source::Argument);
    }

    #[test]
    fn explicit_missing_config_file_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let overrides = Overrides {
            config_file: Some(tmp.path().join("nope.toml")),
            ..Default::default()
        };
        assert!(Config::resolve(&overrides, no_env, &base(tmp.path()), tmp.path()).is_err());
    }

    #[test]
    fn rejects_unknown_keys_and_relative_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("c.toml");
        let overrides = Overrides {
            config_file: Some(file.clone()),
            ..Default::default()
        };
        for content in ["repo_dir = \"/x\"", "repos_dir = \"x\""] {
            std::fs::write(&file, content).unwrap();
            assert!(
                Config::resolve(&overrides, no_env, &base(tmp.path()), tmp.path()).is_err(),
                "{content}"
            );
        }
    }
}
