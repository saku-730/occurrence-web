use std::env;
use std::fmt;

#[derive(Debug, Clone)]
pub struct Config {
    pub app: AppConfig,
    pub posgre: Posgre,
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
pub struct Posgre {
    pub host: String,
    pub port: u16,
    pub base_url: String,
    pub databse_url: String,
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

        let database_url = std::env::var("DATABASE_URL").map_err(|_| ConfigError::MissinEnv("DATABASE_URL".to_string()))?;

        Ok(Self { app })
    }
}

fn get_env_or(key: &'static str, default: &str) -> String {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => default.to_string(),
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