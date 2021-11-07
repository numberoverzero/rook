use serde::{
    de::{self, Deserializer},
    Deserialize,
};
use std::{collections::HashMap, fmt::Display, fs};
use toml;

pub struct RouteConfig {
    pub port: u16,
    pub gh_hooks: HashMap<String, Vec<GithubHook>>,
    pub rook_hooks: HashMap<String, Vec<RookHook>>,
}

pub struct GithubHook {
    pub repo: String,
    pub command: String,
    pub secret: Vec<u8>,
}

pub struct RookHook {
    pub command: String,
    pub secret: Vec<u8>,
}

pub enum ConfigError {
    IoError(std::io::Error),
    DeError(toml::de::Error),
    BadConfig(String),
}

pub fn from_file(config_path: &str) -> Result<RouteConfig, ConfigError> {
    let cfg_str = fs::read_to_string(config_path)?;
    let cfg: _RookConfig = toml::from_str(&cfg_str)?;

    let mut routes = RouteConfig {
        port: cfg.port,
        gh_hooks: HashMap::new(),
        rook_hooks: HashMap::new(),
    };
    for hook in cfg.hooks {
        match hook {
            _HookConfig::_GithubHook {
                url,
                secret,
                command,
                repo,
            } => {
                if routes.rook_hooks.contains_key(&url) {
                    return Err(format!("hook path type conflict: '{}'", url).into());
                }
                routes
                    .gh_hooks
                    .entry(url.to_string())
                    .or_insert_with(|| Vec::new())
                    .push(GithubHook {
                        repo: repo.to_string(),
                        command: command.to_string(),
                        secret: secret.to_vec(),
                    });
            }
            _HookConfig::_RookHook {
                url,
                secret,
                command,
            } => {
                if routes.gh_hooks.contains_key(&url) {
                    return Err(format!("hook path type conflict: '{}'", url).into());
                }
                routes
                    .rook_hooks
                    .entry(url.to_string())
                    .or_insert_with(|| Vec::new())
                    .push(RookHook {
                        command: command.to_string(),
                        secret: secret.to_vec(),
                    });
            }
        };
    }
    Ok(routes)
}

fn deserialize_secret<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: &str = Deserialize::deserialize(deserializer)?;
    fs::read_to_string(s)
        .map_err(|_| de::Error::custom(format!("failed to read secret at '{}'", s)))
        .map(|x| x.trim().as_bytes().to_vec())
}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}
impl From<toml::de::Error> for ConfigError {
    fn from(e: toml::de::Error) -> Self {
        Self::DeError(e)
    }
}
impl From<String> for ConfigError {
    fn from(s: String) -> Self {
        Self::BadConfig(s)
    }
}

impl Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ConfigError::IoError(e) => e.to_string(),
            ConfigError::DeError(e) => e.to_string(),
            ConfigError::BadConfig(e) => e.to_string(),
        };
        f.write_str(&s)
    }
}

#[derive(Deserialize)]
struct _RookConfig {
    port: u16,
    hooks: Vec<_HookConfig>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum _HookConfig {
    #[serde(rename = "github")]
    _GithubHook {
        url: String,
        #[serde(rename = "secret_file")]
        #[serde(deserialize_with = "deserialize_secret")]
        secret: Vec<u8>,
        #[serde(rename = "command_path")]
        command: String,
        repo: String,
    },
    #[serde(rename = "rook")]
    _RookHook {
        url: String,
        #[serde(rename = "secret_file")]
        #[serde(deserialize_with = "deserialize_secret")]
        secret: Vec<u8>,
        #[serde(rename = "command_path")]
        command: String,
    },
}
