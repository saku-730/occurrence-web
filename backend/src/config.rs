use std::env;
use std::fmt;

#[derive(Debug, Clone)]
pub struct Config {
    pub app: AppConfig,
    pub posgre: PosgreConfig,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub app_base_url: String,
}

impl AppConfig {
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Clone)]
pub struct PosgreConfig {
    pub url: String,
}

#[derive(Debug)]
pub enum ConfigError {
    MissingVar(&'static str),
    InvalidVar {
        key: &'static str,
        value: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::MissingVar(key) => {
                write!(f, "missing environment variable: {}", key)
            }
            ConfigError::InvalidVar { key, value } => {
                write!(f, "invalid environment variable: {}={}", key, value)
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let _ = dotenvy::dotenv(); //.envから環境変数へ

        let app = AppConfig { //アプリの基本情報
            host: get_env_or("APP_HOST", "127.0.0.1"),
            port: parse_u16_env_or("APP_PORT", 3000)?,
            app_base_url: get_env_or("APP_BASE_URL", "http://127.0.0.1:3000"),
        };

        let posgre = PosgreConfig{
            url: get_required_env("DATABASE_URL")?,
        };

        Ok(Self { app , posgre})
    }
}

fn get_env_or(key: &'static str, default: &str) -> String {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => default.to_string(),
    }
}

fn get_required_env(key: &'static str) -> Result<String, ConfigError> {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(ConfigError::MissingVar(key)),
    }
}


fn parse_u16_env_or(key: &'static str, default: u16) -> Result<u16, ConfigError> {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => {
            value.parse::<u16>().map_err(|_| ConfigError::InvalidVar {
                key,
                value,
            })
        }
        _ => Ok(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env,
        sync::{Mutex, OnceLock},
    };

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvGuard {
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn new(vars: &[(&'static str, &'static str)]) -> Self {
            let keys = [
                "APP_HOST",
                "APP_PORT",
                "APP_BASE_URL",
                "DATABASE_URL",
            ];

            let saved = keys
                .iter()
                .map(|&key| (key, env::var(key).ok()))
                .collect();

            unsafe {
                for key in keys {
                    env::remove_var(key);
                }

                for &(key, value) in vars {
                    env::set_var(key, value);
                }
            }

            Self { saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                for (key, value) in &self.saved {
                    match value {
                        Some(value) => env::set_var(key, value),
                        None => env::remove_var(key),
                    }
                }
            }
        }
    }

    #[test]
    fn from_env_reads_app_host_port_base_url_and_database_url() {
        let _lock = env_lock().lock().unwrap();

        let _env = EnvGuard::new(&[
            ("APP_HOST", "127.0.0.1"),
            ("APP_PORT", "3000"),
            ("APP_BASE_URL", "http://127.0.0.1:3000"),
            (
                "DATABASE_URL",
                "postgres://admin:occurrence_password@localhost:5432/occurrence_web",
            ),
        ]);

        let config = Config::from_env()
            .expect("Config::from_env should read required environment variables");

        assert_eq!(config.app.host, "127.0.0.1");
        assert_eq!(config.app.port, 3000);
        assert_eq!(config.app.app_base_url, "http://127.0.0.1:3000");
        assert_eq!(
            config.posgre.url,
            "postgres://admin:occurrence_password@localhost:5432/occurrence_web"
        );
    }
}