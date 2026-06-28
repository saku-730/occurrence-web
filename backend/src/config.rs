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

        let app = AppConfig {
            // アプリ基本設定は開発環境で起動しやすいようにdefaultを持つ。
            host: get_env_or("APP_HOST", "127.0.0.1"),
            port: parse_u16_env_or("APP_PORT", 3000)?,
            app_base_url: get_env_or("APP_BASE_URL", "http://127.0.0.1:3000"),
            environment: get_env_or("APP_ENV", "development"),
            // COOKIE_SECUREは本番でtrueにする。未指定時falseなのはローカルHTTP開発を妨げないため。
            cookie_secure: parse_bool_env_or("COOKIE_SECURE", false)?,
        };

        validate_app_config(&app)?;

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

        let garage = GarageConfig {
            bucket: get_required_env("S3_BUCKET")?,
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

    struct EnvGuard {
        key: &'static str,
        old_value: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let old_value = env::var(key).ok();
            // Rust 2024では環境変数変更がunsafe。テストは --test-threads=1 で直列実行し、
            // Dropで必ず元の値へ戻すことでDB接続など後続テストへの影響を残さない。
            unsafe {
                env::set_var(key, value);
            }

            Self { key, old_value }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // unset/setどちらもプロセス全体へ影響するためunsafe。
            // guardの寿命をテスト関数内に閉じ、panic時も復元されるようDropに寄せる。
            unsafe {
                match &self.old_value {
                    Some(value) => env::set_var(self.key, value),
                    None => env::remove_var(self.key),
                }
            }
        }
    }

    fn set_required_config_env() -> Vec<EnvGuard> {
        vec![
            EnvGuard::set(
                "DATABASE_URL",
                "postgres://user:password@localhost:5432/test",
            ),
            EnvGuard::set("FUSEKI_BASE_URL", "http://127.0.0.1:3030/ds"),
            EnvGuard::set("FUSEKI_USER", "admin"),
            EnvGuard::set("FUSEKI_PASSWORD", "password"),
            EnvGuard::set("S3_BUCKET", "test-required-bucket"),
        ]
    }

    #[test]
    fn from_env_reads_s3_bucket() {
        let _guards = set_required_config_env();

        let config = Config::from_env().expect("config should load S3 bucket");

        assert_eq!(config.garage.bucket, "test-required-bucket");
    }

    #[test]
    fn from_env_rejects_insecure_cookie_in_production() {
        let mut guards = set_required_config_env();
        guards.push(EnvGuard::set("APP_ENV", "production"));
        guards.push(EnvGuard::set("COOKIE_SECURE", "false"));

        let result = Config::from_env();

        assert!(
            matches!(result, Err(ConfigError::InvalidCombination { .. })),
            "production should not start when COOKIE_SECURE is false: {:?}",
            result
        );
    }

    #[test]
    fn from_env_accepts_secure_cookie_in_production() {
        let mut guards = set_required_config_env();
        guards.push(EnvGuard::set("APP_ENV", "production"));
        guards.push(EnvGuard::set("COOKIE_SECURE", "true"));

        let config = Config::from_env().expect("production config should load with secure cookie");

        assert_eq!(config.app.environment, "production");
        assert!(config.app.cookie_secure);
    }
}
