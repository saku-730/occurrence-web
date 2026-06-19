use std::env;
use std::fmt;

// アプリ起動時に環境変数を集約する設定。handlerやserviceが直接envを読まないようにする。
#[derive(Debug, Clone)]
pub struct Config {
    pub app: AppConfig,
    pub posgre: PosgreConfig,
    pub smtp: SmtpConfig,     //メール関係
    pub fuseki: FusekiConfig, //Fuseki関係
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub app_base_url: String,
    // 本番ではsession cookieをHTTPSに限定するためtrueにする。開発ではHTTPで動かすためfalseを許可する。
    pub cookie_secure: bool,
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

#[derive(Debug, Clone)]
pub struct FusekiConfig {
    pub base_url: String,
    pub user: String,
    pub password: String,
}

impl FusekiConfig {
    // Fusekiは用途ごとにendpointが分かれるため、URL組み立てをここに閉じ込める。
    pub fn data_url(&self) -> String {
        format!("{}/data", self.base_url.trim_end_matches('/'))
    }

    pub fn sparql_url(&self) -> String {
        format!("{}/sparql", self.base_url.trim_end_matches('/'))
    }

    pub fn update_url(&self) -> String {
        format!("{}/update", self.base_url.trim_end_matches('/'))
    }
}

#[derive(Debug)]
pub enum ConfigError {
    MissingVar(&'static str),
    InvalidVar { key: &'static str, value: String },
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

        let app = AppConfig {
            // アプリ基本設定は開発環境で起動しやすいようにdefaultを持つ。
            host: get_env_or("APP_HOST", "127.0.0.1"),
            port: parse_u16_env_or("APP_PORT", 3000)?,
            app_base_url: get_env_or("APP_BASE_URL", "http://127.0.0.1:3000"),
            // COOKIE_SECUREは本番でtrueにする。未指定時falseなのはローカルHTTP開発を妨げないため。
            cookie_secure: parse_bool_env_or("COOKIE_SECURE", false)?,
        };

        let posgre = PosgreConfig {
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

        let fuseki = FusekiConfig {
            base_url: get_required_env("FUSEKI_BASE_URL")?,
            user: get_required_env("FUSEKI_USER")?,
            password: get_required_env("FUSEKI_PASSWORD")?,
        };

        Ok(Self {
            app,
            posgre,
            smtp,
            fuseki,
        })
    }
}

fn get_env_or(key: &'static str, default: &str) -> String {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => default.to_string(),
    }
}

// 外部サービス接続に必須な値は、空文字defaultで起動して失敗するより起動時に明示的に落とす。
fn get_required_env(key: &'static str) -> Result<String, ConfigError> {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(ConfigError::MissingVar(key)),
    }
}

fn parse_u16_env_or(key: &'static str, default: u16) -> Result<u16, ConfigError> {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => value
            .parse::<u16>()
            .map_err(|_| ConfigError::InvalidVar { key, value }),
        _ => Ok(default),
    }
}

// DockerやPaaSの環境変数表現に合わせて、true/falseだけでなく1/0なども受け付ける。
fn parse_bool_env_or(key: &'static str, default: bool) -> Result<bool, ConfigError> {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => {
            let normalized = value.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "true" | "1" | "yes" | "on" => Ok(true),
                "false" | "0" | "no" | "off" => Ok(false),
                _ => Err(ConfigError::InvalidVar { key, value }),
            }
        }
        _ => Ok(default),
    }
}
