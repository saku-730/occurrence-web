use std::{env, fmt};

use chrono::Utc;
use futures_util::StreamExt;
use reqwest::{
    Client, Url,
    header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HOST},
};
use sha2::{Digest, Sha256};
use tokio_util::io::ReaderStream;

use crate::features::media::service::{
    DeleteMediaObjectInput, GetMediaObjectInput, MediaObjectByteStream, MediaObjectStore,
    MediaServiceError, PutMediaObjectInput,
};

// GarageはS3互換APIを提供するため、backendではS3のpath-style PUTだけを利用する。
// SDK全体を導入せず、添付ファイル保存に必要な最小APIをここへ閉じ込めている。
#[derive(Clone)]
pub struct GarageMediaObjectStore {
    http: Client,
    endpoint: String,
    region: String,
    bucket: String,
    access_key: String,
    secret_key: String,
}

#[derive(Debug)]
pub struct GarageClientError {
    message: String,
}

impl GarageClientError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for GarageClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for GarageClientError {}

impl GarageMediaObjectStore {
    pub fn from_env() -> Result<Self, GarageClientError> {
        let endpoint = required_env("S3_ENDPOINT")?;
        let region = required_env("S3_REGION")?;
        let bucket = required_env("S3_BUCKET")?;
        let access_key = required_env("S3_ACCESS_KEY")?;
        let secret_key = required_env("S3_SECRET_KEY")?;
        let force_path_style = parse_bool_env("S3_FORCE_PATH_STYLE", true)?;

        // Garageの開発・本番構成はpath-styleを前提にしている。
        // virtual-host styleを暗黙に組み立てるとendpointやTLS証明書の扱いが変わるため、設定ミスとして起動時に止める。
        if !force_path_style {
            return Err(GarageClientError::new(
                "S3_FORCE_PATH_STYLE must be true for Garage",
            ));
        }

        Url::parse(endpoint.trim_end_matches('/'))
            .map_err(|error| GarageClientError::new(format!("invalid S3_ENDPOINT: {error}")))?;

        Ok(Self {
            http: Client::new(),
            endpoint: endpoint.trim_end_matches('/').to_string(),
            region,
            bucket,
            access_key,
            secret_key,
        })
    }

    async fn put_object_to_garage(
        &self,
        input: PutMediaObjectInput,
    ) -> Result<(), GarageClientError> {
        if input.bucket != self.bucket {
            return Err(GarageClientError::new(format!(
                "requested bucket {} does not match configured bucket {}",
                input.bucket, self.bucket
            )));
        }

        let object_url = format!(
            "{}/{}/{}",
            self.endpoint,
            encode_uri_path_segment(&input.bucket),
            encode_object_key(&input.object_key)
        );
        let url = Url::parse(&object_url)
            .map_err(|error| GarageClientError::new(format!("invalid object URL: {error}")))?;
        let host = host_header_value(&url)?;
        let metadata = tokio::fs::metadata(&input.file_path)
            .await
            .map_err(|error| {
                GarageClientError::new(format!("temporary media file metadata failed: {error}"))
            })?;
        if metadata.len() != input.size_bytes {
            return Err(GarageClientError::new(
                "temporary media file size changed before Garage PUT",
            ));
        }
        let payload_hash = input.payload_sha256;

        let now = Utc::now();
        let date = now.format("%Y%m%d").to_string();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let content_type = input.content_type.trim();

        // AWS Signature V4では、署名対象headerと実際に送るheaderが完全に一致する必要がある。
        // object keyはbackend発行値だが、URL pathは念のためsegment単位でpercent encodeする。
        let canonical_headers = format!(
            "content-type:{content_type}\nhost:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n"
        );
        let signed_headers = "content-type;host;x-amz-content-sha256;x-amz-date";
        let canonical_request = format!(
            "PUT\n{}\n\n{}\n{}\n{}",
            url.path(),
            canonical_headers,
            signed_headers,
            payload_hash
        );
        let credential_scope = format!("{date}/{}/s3/aws4_request", self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let signing_key = signing_key(&self.secret_key, &date, &self.region);
        let signature = hex::encode(hmac_sha256(&signing_key, string_to_sign.as_bytes()));
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key
        );

        let file = tokio::fs::File::open(&input.file_path)
            .await
            .map_err(|error| {
                GarageClientError::new(format!("temporary media file open failed: {error}"))
            })?;
        let body = reqwest::Body::wrap_stream(ReaderStream::new(file));

        let response = self
            .http
            .put(url)
            .header(HOST, host)
            .header(CONTENT_TYPE, content_type)
            .header(CONTENT_LENGTH, input.size_bytes)
            .header("x-amz-content-sha256", payload_hash)
            .header("x-amz-date", amz_date)
            .header(AUTHORIZATION, authorization)
            .body(body)
            .send()
            .await
            .map_err(|error| GarageClientError::new(format!("Garage PUT failed: {error}")))?;

        if !response.status().is_success() {
            return Err(GarageClientError::new(format!(
                "Garage PUT returned unexpected status {}",
                response.status()
            )));
        }

        Ok(())
    }

    async fn get_object_from_garage(
        &self,
        input: GetMediaObjectInput,
    ) -> Result<reqwest::Response, GarageClientError> {
        if input.bucket != self.bucket {
            return Err(GarageClientError::new(format!(
                "requested bucket {} does not match configured bucket {}",
                input.bucket, self.bucket
            )));
        }

        let object_url = format!(
            "{}/{}/{}",
            self.endpoint,
            encode_uri_path_segment(&input.bucket),
            encode_object_key(&input.object_key)
        );
        let url = Url::parse(&object_url)
            .map_err(|error| GarageClientError::new(format!("invalid object URL: {error}")))?;
        let host = host_header_value(&url)?;
        let payload_hash = sha256_hex(&[]);
        let now = Utc::now();
        let date = now.format("%Y%m%d").to_string();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let canonical_headers =
            format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";
        let canonical_request = format!(
            "GET\n{}\n\n{}\n{}\n{}",
            url.path(),
            canonical_headers,
            signed_headers,
            payload_hash
        );
        let credential_scope = format!("{date}/{}/s3/aws4_request", self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let signing_key = signing_key(&self.secret_key, &date, &self.region);
        let signature = hex::encode(hmac_sha256(&signing_key, string_to_sign.as_bytes()));
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key
        );

        let response = self
            .http
            .get(url)
            .header(HOST, host)
            .header("x-amz-content-sha256", payload_hash)
            .header("x-amz-date", amz_date)
            .header(AUTHORIZATION, authorization)
            .send()
            .await
            .map_err(|error| GarageClientError::new(format!("Garage GET failed: {error}")))?;

        if !response.status().is_success() {
            return Err(GarageClientError::new(format!(
                "Garage GET returned unexpected status {}",
                response.status()
            )));
        }

        Ok(response)
    }

    async fn delete_object_from_garage(
        &self,
        input: DeleteMediaObjectInput,
    ) -> Result<(), GarageClientError> {
        if input.bucket != self.bucket {
            return Err(GarageClientError::new(format!(
                "requested bucket {} does not match configured bucket {}",
                input.bucket, self.bucket
            )));
        }

        let object_url = format!(
            "{}/{}/{}",
            self.endpoint,
            encode_uri_path_segment(&input.bucket),
            encode_object_key(&input.object_key)
        );
        let url = Url::parse(&object_url)
            .map_err(|error| GarageClientError::new(format!("invalid object URL: {error}")))?;
        let host = host_header_value(&url)?;
        let payload_hash = sha256_hex(&[]);
        let now = Utc::now();
        let date = now.format("%Y%m%d").to_string();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();

        // DELETEにはbodyがないため、hostとAWS署名用headerだけをcanonical requestへ含める。
        let canonical_headers =
            format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";
        let canonical_request = format!(
            "DELETE\n{}\n\n{}\n{}\n{}",
            url.path(),
            canonical_headers,
            signed_headers,
            payload_hash
        );
        let credential_scope = format!("{date}/{}/s3/aws4_request", self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let signing_key = signing_key(&self.secret_key, &date, &self.region);
        let signature = hex::encode(hmac_sha256(&signing_key, string_to_sign.as_bytes()));
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key
        );

        let response = self
            .http
            .delete(url)
            .header(HOST, host)
            .header("x-amz-content-sha256", payload_hash)
            .header("x-amz-date", amz_date)
            .header(AUTHORIZATION, authorization)
            .send()
            .await
            .map_err(|error| GarageClientError::new(format!("Garage DELETE failed: {error}")))?;

        if !response.status().is_success() {
            return Err(GarageClientError::new(format!(
                "Garage DELETE returned unexpected status {}",
                response.status()
            )));
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl MediaObjectStore for GarageMediaObjectStore {
    async fn put_object(&self, input: PutMediaObjectInput) -> Result<(), MediaServiceError> {
        // infrastructure固有の認証・HTTPエラーはservice境界では保存失敗へ畳み込む。
        // handlerはこれを502へ変換し、Garage内部情報をレスポンスへ露出しない。
        self.put_object_to_garage(input)
            .await
            .map_err(|_| MediaServiceError::ObjectStoreFailed)
    }

    async fn get_object(
        &self,
        input: GetMediaObjectInput,
    ) -> Result<MediaObjectByteStream, MediaServiceError> {
        let response = self
            .get_object_from_garage(input)
            .await
            .map_err(|_| MediaServiceError::ObjectStoreFailed)?;
        let stream = response
            .bytes_stream()
            .map(|chunk| chunk.map_err(|_| MediaServiceError::ObjectStoreFailed));
        Ok(Box::pin(stream))
    }

    async fn delete_object(&self, input: DeleteMediaObjectInput) -> Result<(), MediaServiceError> {
        self.delete_object_from_garage(input)
            .await
            .map_err(|_| MediaServiceError::ObjectStoreFailed)
    }
}

fn required_env(key: &'static str) -> Result<String, GarageClientError> {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(GarageClientError::new(format!(
            "{key} must be set for Garage object storage"
        ))),
    }
}

fn parse_bool_env(key: &'static str, default: bool) -> Result<bool, GarageClientError> {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Ok(true),
            "false" | "0" | "no" | "off" => Ok(false),
            _ => Err(GarageClientError::new(format!(
                "{key} must be a boolean value"
            ))),
        },
        _ => Ok(default),
    }
}

fn host_header_value(url: &Url) -> Result<String, GarageClientError> {
    let host = url
        .host_str()
        .ok_or_else(|| GarageClientError::new("S3 endpoint must contain a host"))?;

    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

fn encode_object_key(object_key: &str) -> String {
    object_key
        .split('/')
        .map(encode_uri_path_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn encode_uri_path_segment(value: &str) -> String {
    let mut encoded = String::new();

    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }

    encoded
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

// HMAC-SHA256はSignature V4の鍵導出だけに利用する小さな実装。
// SHA-256のblock size 64 bytesに従い、長い鍵をhashしてからinner/outer padを適用する。
fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK_SIZE: usize = 64;
    let mut normalized_key = [0_u8; BLOCK_SIZE];

    if key.len() > BLOCK_SIZE {
        normalized_key[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        normalized_key[..key.len()].copy_from_slice(key);
    }

    let mut inner_pad = [0x36_u8; BLOCK_SIZE];
    let mut outer_pad = [0x5c_u8; BLOCK_SIZE];
    for index in 0..BLOCK_SIZE {
        inner_pad[index] ^= normalized_key[index];
        outer_pad[index] ^= normalized_key[index];
    }

    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(data);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_hash);
    outer.finalize().into()
}

fn signing_key(secret_key: &str, date: &str, region: &str) -> [u8; 32] {
    let date_key = hmac_sha256(format!("AWS4{secret_key}").as_bytes(), date.as_bytes());
    let region_key = hmac_sha256(&date_key, region.as_bytes());
    let service_key = hmac_sha256(&region_key, b"s3");
    hmac_sha256(&service_key, b"aws4_request")
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, process::Command};

    fn required_env(key: &str) -> String {
        std::env::var(key).unwrap_or_else(|_| panic!("{} must be set for real Garage test", key))
    }

    fn run_aws_s3_command(args: &[String]) {
        let endpoint = required_env("S3_ENDPOINT");
        let region = required_env("S3_REGION");
        let access_key = required_env("S3_ACCESS_KEY");
        let secret_key = required_env("S3_SECRET_KEY");

        let output = Command::new("aws")
            .env("AWS_ACCESS_KEY_ID", access_key)
            .env("AWS_SECRET_ACCESS_KEY", secret_key)
            .env("AWS_DEFAULT_REGION", region)
            .arg("--endpoint-url")
            .arg(endpoint)
            .args(args)
            .output()
            .expect("aws CLI should be installed for real Garage test");

        assert!(
            output.status.success(),
            "aws command failed: status={:?}, stdout={}, stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "requires a running Garage server and backend/.env S3_* credentials"]
    fn garage_client_puts_lists_and_deletes_object_from_real_garage() {
        dotenvy::dotenv().ok();

        let bucket = required_env("S3_BUCKET");
        let object_key = format!("connectivity-test/backend-{}.txt", uuid::Uuid::new_v4());
        let object_uri = format!("s3://{}/{}", bucket, object_key);

        let mut temp_path: PathBuf = std::env::temp_dir();
        temp_path.push(format!(
            "occurrence-web-garage-test-{}.txt",
            uuid::Uuid::new_v4()
        ));

        fs::write(&temp_path, b"garage real connectivity test\n")
            .expect("temporary upload file should be writable");

        // 実GarageのS3互換APIに対して、最小のwrite/read-metadata/delete権限を確認する。
        run_aws_s3_command(&vec![
            "s3".to_string(),
            "cp".to_string(),
            temp_path.display().to_string(),
            object_uri.clone(),
        ]);

        run_aws_s3_command(&vec![
            "s3".to_string(),
            "ls".to_string(),
            object_uri.clone(),
        ]);

        run_aws_s3_command(&vec!["s3".to_string(), "rm".to_string(), object_uri]);

        fs::remove_file(&temp_path).expect("temporary upload file should be removed");
    }
}
