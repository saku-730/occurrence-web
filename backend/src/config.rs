use std::env;
use std::fmt;

// アプリ起動時に環境変数を集約する設定。handlerやserviceが直接envを読まないようにする。
#[derive(Debug, Clone)]
pub struct Config {
    pub app: AppConfig,
    pub posgre: PosgreConfig,
    pub smtp: SmtpConfig,     //メール関係
    pub fuseki: FusekiConfig, //Fuseki関係
    pub garage: GarageConfig, //Garage/S3互換object storage
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub app_base_url: String,
    // productionではセキュリティ要件を強制する。文字列にしているのは.envやPaaSの値をそのまま扱いやすくするため。
    pub environment: String,
    // 本番ではsession cookieをHTTPSに限定するためtrueにする。開発ではHTTPで動かすためfalseを許可する。
    pub cookie_secure: bool,
    // ユーザー名だけで本人扱いするため、デモ専用環境で明示的に有効化する。
    pub demo_auth_enabled: bool,
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
pub struct GarageConfig {
    // bucket名は環境ごとに異なり得るため、handlerへ固定値を持ち込まない。
    pub bucket: String,
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
    InvalidCombination { message: String },
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
            ConfigError::InvalidCombination { message } => write!(f, "invalid config: {}", message),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let _ = dotenvy::dotenv(); //.envから環境変数へ

        Self::from_lookup(|key| env::var(key).ok())
    }

    // Inject the source, not process-global variables, when testing configuration parsing.
    fn from_lookup(lookup: impl Fn(&'static str) -> Option<String>) -> Result<Self, ConfigError> {
        let app = AppConfig {
            // アプリ基本設定は開発環境で起動しやすいようにdefaultを持つ。
            host: get_env_or(&lookup, "APP_HOST", "127.0.0.1"),
            port: parse_u16_env_or(&lookup, "APP_PORT", 3000)?,
            app_base_url: get_env_or(&lookup, "APP_BASE_URL", "http://127.0.0.1:3000"),
            environment: get_env_or(&lookup, "APP_ENV", "development"),
            // COOKIE_SECUREは本番でtrueにする。未指定時falseなのはローカルHTTP開発を妨げないため。
            cookie_secure: parse_bool_env_or(&lookup, "COOKIE_SECURE", false)?,
            demo_auth_enabled: parse_bool_env_or(&lookup, "DEMO_AUTH_ENABLED", false)?,
        };

        validate_app_config(&app)?;

        let posgre = PosgreConfig {
            url: get_required_env(&lookup, "DATABASE_URL")?,
        };

        let smtp = SmtpConfig {
            host: get_env_or(&lookup, "SMTP_HOST", "127.0.0.1"),
            port: parse_u16_env_or(&lookup, "SMTP_PORT", 1025)?,
            username: get_env_or(&lookup, "SMTP_USERNAME", ""),
            password: get_env_or(&lookup, "SMTP_PASSWORD", ""),
            tls: get_env_or(&lookup, "SMTP_TLS", "none"),
            from: get_env_or(&lookup, "MAIL_FROM", "no-reply@example.com"),
        };

        let fuseki = FusekiConfig {
            base_url: get_required_env(&lookup, "FUSEKI_BASE_URL")?,
            user: get_required_env(&lookup, "FUSEKI_USER")?,
            password: get_required_env(&lookup, "FUSEKI_PASSWORD")?,
        };

        let garage = GarageConfig {
            bucket: get_required_env(&lookup, "S3_BUCKET")?,
        };

        Ok(Self {
            app,
            posgre,
            smtp,
            fuseki,
            garage,
        })
    }
}

fn get_env_or(
    lookup: &impl Fn(&'static str) -> Option<String>,
    key: &'static str,
    default: &str,
) -> String {
    match lookup(key) {
        Some(value) if !value.trim().is_empty() => value,
        _ => default.to_string(),
    }
}

// 外部サービス接続に必須な値は、空文字defaultで起動して失敗するより起動時に明示的に落とす。
fn get_required_env(
    lookup: &impl Fn(&'static str) -> Option<String>,
    key: &'static str,
) -> Result<String, ConfigError> {
    match lookup(key) {
        Some(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(ConfigError::MissingVar(key)),
    }
}

fn parse_u16_env_or(
    lookup: &impl Fn(&'static str) -> Option<String>,
    key: &'static str,
    default: u16,
) -> Result<u16, ConfigError> {
    match lookup(key) {
        Some(value) if !value.trim().is_empty() => value
            .parse::<u16>()
            .map_err(|_| ConfigError::InvalidVar { key, value }),
        _ => Ok(default),
    }
}

// DockerやPaaSの環境変数表現に合わせて、true/falseだけでなく1/0なども受け付ける。
fn parse_bool_env_or(
    lookup: &impl Fn(&'static str) -> Option<String>,
    key: &'static str,
    default: bool,
) -> Result<bool, ConfigError> {
    match lookup(key) {
        Some(value) if !value.trim().is_empty() => {
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

// 環境変数同士の組み合わせで決まる制約は、読み込み直後にまとめて検証する。
// 本番でSecureなしCookieを許すとsession cookieが平文HTTPへ流れるため、アプリを起動させない。
fn validate_app_config(app: &AppConfig) -> Result<(), ConfigError> {
    if app.environment.trim().eq_ignore_ascii_case("production") && !app.cookie_secure {
        return Err(ConfigError::InvalidCombination {
            message: "COOKIE_SECURE must be true when APP_ENV=production".to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn required_config() -> std::collections::HashMap<&'static str, String> {
        [
            (
                "DATABASE_URL",
                "postgres://user:password@localhost:5432/test",
            ),
            ("FUSEKI_BASE_URL", "http://127.0.0.1:3030/ds"),
            ("FUSEKI_USER", "admin"),
            ("FUSEKI_PASSWORD", "password"),
            ("S3_BUCKET", "test-required-bucket"),
        ]
        .into_iter()
        .map(|(key, value)| (key, value.to_string()))
        .collect()
    }

    #[test]
    fn from_env_reads_s3_bucket() {
        let values = required_config();

        let config = Config::from_lookup(|key| values.get(key).cloned())
            .expect("config should load S3 bucket");

        assert_eq!(config.garage.bucket, "test-required-bucket");
    }

    #[test]
    fn from_env_rejects_insecure_cookie_in_production() {
        let mut values = required_config();
        values.insert("APP_ENV", "production".to_string());
        values.insert("COOKIE_SECURE", "false".to_string());

        let result = Config::from_lookup(|key| values.get(key).cloned());

        assert!(
            matches!(result, Err(ConfigError::InvalidCombination { .. })),
            "production should not start when COOKIE_SECURE is false: {:?}",
            result
        );
    }

    #[test]
    fn from_env_accepts_secure_cookie_in_production() {
        let mut values = required_config();
        values.insert("APP_ENV", "production".to_string());
        values.insert("COOKIE_SECURE", "true".to_string());

        let config = Config::from_lookup(|key| values.get(key).cloned())
            .expect("production config should load with secure cookie");

        assert_eq!(config.app.environment, "production");
        assert!(config.app.cookie_secure);
    }
}
