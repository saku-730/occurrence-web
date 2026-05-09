use std::env;
use std::fmt;

#[derive(Debug, Clone)]
pub struct Config {
    pub app: AppConfig,
    pub posgre: PosgreConfig,
    pub smtp: SmtpConfig, //メール関係
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

#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub tls: String,
    pub from: String,
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

        let smtp = SmtpConfig {
            host: get_env_or("SMTP_HOST", "127.0.0.1"),
            port: parse_u16_env_or("SMTP_PORT", 1025)?,
            username: get_env_or("SMTP_USERNAME", ""),
            password: get_env_or("SMTP_PASSWORD", ""),
            tls: get_env_or("SMTP_TLS", "none"),
            from: get_env_or("MAIL_FROM", "no-reply@example.com"),
        };

        Ok(Self { app, posgre, smtp })
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
                "SMTP_HOST",
                "SMTP_PORT",
                "SMTP_USERNAME",
                "SMTP_PASSWORD",
                "SMTP_TLS",
                "MAIL_FROM",
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
            ("DATABASE_URL", "postgres://admin:occurrence_password@localhost:5432/occurrence_web"),
            ("SMTP_HOST", "smtp.gmail.com"),
            ("SMTP_PORT", "587"),
            ("SMTP_USERNAME", "test-user@gmail.com"),
            ("SMTP_PASSWORD", "test-app-password"),
            ("SMTP_TLS", "starttls"),
            ("MAIL_FROM", "test-user@gmail.com"),
        ]);
        let config = Config::from_env()
            .expect("Config::from_env should read SMTP environment variables");

        assert_eq!(config.smtp.host, "smtp.gmail.com");
        assert_eq!(config.smtp.port, 587);
        assert_eq!(config.smtp.username, "test-user@gmail.com");
        assert_eq!(config.smtp.password, "test-app-password");
        assert_eq!(config.smtp.tls, "starttls");
        assert_eq!(config.smtp.from, "test-user@gmail.com");

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