//! APNs provider API: token-based authentication and HTTP/2 requests.
//!
//! The signing key and team id come from a local file outside the repository
//! (`<data dir>/push/apns.json`). Nothing here logs the key, the provider
//! token, a device token or the team id.

use super::payload::Alert;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use bytes::Bytes;
use ring::{
    rand::SystemRandom,
    signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair},
};
use serde::Deserialize;
use serde_json::json;
use soyeht_platform::fs::file_is_owner_only;
use soyeht_protocol::PushEnvironment;
use std::{
    collections::HashMap,
    future::Future,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

pub const CONFIG_DIRECTORY: &str = "push";
pub const CONFIG_FILE: &str = "apns.json";
/// Apple asks for a new provider token at most every 20 and at least every
/// 60 minutes.
const TOKEN_LIFETIME_SEC: i64 = 50 * 60;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const RESPONSE_BYTES: usize = 4 * 1024;

pub fn host(environment: &PushEnvironment) -> &'static str {
    match environment {
        PushEnvironment::Sandbox => "api.sandbox.push.apple.com",
        PushEnvironment::Production => "api.push.apple.com",
    }
}

/// Why push stays off. Each value is safe to log: none carries a secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigProblem {
    Missing,
    Unreadable,
    Invalid,
    KeyUnreadable,
    KeyExposed,
    KeyInvalid,
}

impl ConfigProblem {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "config_missing",
            Self::Unreadable => "config_unreadable",
            Self::Invalid => "config_invalid",
            Self::KeyUnreadable => "key_unreadable",
            Self::KeyExposed => "key_readable_by_others",
            Self::KeyInvalid => "key_invalid",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ConfigFile {
    key_id: String,
    team_id: String,
    key_path: PathBuf,
    topic: String,
}

/// Credentials loaded from the local config file.
pub struct Credentials {
    pub key_id: String,
    pub team_id: String,
    /// The app's bundle id, sent as `apns-topic`.
    pub topic: String,
    pub key: EcdsaKeyPair,
}

fn apple_id(value: &str) -> bool {
    value.len() == 10 && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/// The DER bytes inside a PEM `PRIVATE KEY` block (the `.p8` format).
fn pem_private_key(text: &str) -> Option<Vec<u8>> {
    let body = text
        .trim()
        .strip_prefix("-----BEGIN PRIVATE KEY-----")?
        .strip_suffix("-----END PRIVATE KEY-----")?;
    let encoded: String = body.split_whitespace().collect();
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()
}

impl Credentials {
    pub fn load(data_directory: &Path) -> Result<Self, ConfigProblem> {
        let directory = data_directory.join(CONFIG_DIRECTORY);
        let raw = match std::fs::read(directory.join(CONFIG_FILE)) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(ConfigProblem::Missing);
            }
            // `Unreadable` is the reason `engine.push_disabled` logs.
            Err(_) => return Err(ConfigProblem::Unreadable),
        };
        let config: ConfigFile =
            serde_json::from_slice(&raw).map_err(|_| ConfigProblem::Invalid)?;
        if !apple_id(&config.key_id)
            || !apple_id(&config.team_id)
            || config.topic.is_empty()
            || config.topic.ends_with(".voip")
        {
            return Err(ConfigProblem::Invalid);
        }
        let key_path = directory.join(&config.key_path);
        let key_file = std::fs::File::open(&key_path).map_err(|_| ConfigProblem::KeyUnreadable)?;
        if !file_is_owner_only(&key_file).map_err(|_| ConfigProblem::KeyUnreadable)? {
            return Err(ConfigProblem::KeyExposed);
        }
        let text = std::fs::read_to_string(&key_path).map_err(|_| ConfigProblem::KeyUnreadable)?;
        let der = pem_private_key(&text).ok_or(ConfigProblem::KeyInvalid)?;
        let key =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &der, &SystemRandom::new())
                .map_err(|_| ConfigProblem::KeyInvalid)?;
        Ok(Self {
            key_id: config.key_id,
            team_id: config.team_id,
            topic: config.topic,
            key,
        })
    }
}

/// The ES256 JSON Web Token APNs expects in `authorization`, reused for 50
/// minutes and refreshed sooner only when APNs reports it expired.
pub struct ProviderToken {
    credentials: Credentials,
    current: Option<(String, i64)>,
}

impl ProviderToken {
    pub fn new(credentials: Credentials) -> Self {
        Self {
            credentials,
            current: None,
        }
    }

    pub fn topic(&self) -> &str {
        &self.credentials.topic
    }

    pub fn expire(&mut self) {
        self.current = None;
    }

    pub fn bearer(&mut self, now_unix: i64) -> io::Result<String> {
        if let Some((token, issued)) = &self.current
            && now_unix - issued < TOKEN_LIFETIME_SEC
            && now_unix >= *issued
        {
            return Ok(token.clone());
        }
        let header = json!({"alg":"ES256","kid":self.credentials.key_id});
        let claims = json!({"iss":self.credentials.team_id,"iat":now_unix});
        let signing_input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let signature = self
            .credentials
            .key
            .sign(&SystemRandom::new(), signing_input.as_bytes())
            .map_err(|_| io::Error::other("provider token signing failed"))?;
        let token = format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature.as_ref())
        );
        self.current = Some((token.clone(), now_unix));
        Ok(token)
    }
}

/// One POST to `/3/device/<token>`.
#[derive(Clone, Debug, PartialEq)]
pub struct ApnsRequest {
    pub environment: PushEnvironment,
    pub path: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl ApnsRequest {
    pub fn alert(
        alert: &Alert,
        device_token: &str,
        environment: PushEnvironment,
        topic: &str,
        bearer: &str,
    ) -> io::Result<Self> {
        Ok(Self {
            environment,
            path: format!("/3/device/{device_token}"),
            headers: vec![
                ("authorization", format!("bearer {bearer}")),
                ("apns-id", uuid::Uuid::new_v4().to_string()),
                ("apns-push-type", "alert".into()),
                ("apns-topic", topic.into()),
                ("apns-priority", "10".into()),
                ("apns-expiration", alert.expires_unix.max(0).to_string()),
                ("apns-collapse-id", alert.collapse_id.clone()),
            ],
            body: serde_json::to_vec(&alert.payload)?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApnsResponse {
    pub status: u16,
    /// The `reason` string APNs returns with an error.
    pub reason: Option<String>,
}

pub trait Transport: Send + 'static {
    fn send(
        &mut self,
        request: ApnsRequest,
    ) -> impl Future<Output = io::Result<ApnsResponse>> + Send;
}

/// One reused HTTP/2 connection per APNs environment, over TLS 1.2 or later
/// with the Mozilla root set.
pub struct Http2 {
    tls: tokio_rustls::TlsConnector,
    connections: HashMap<&'static str, h2::client::SendRequest<Bytes>>,
}

impl Default for Http2 {
    fn default() -> Self {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let mut config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        config.alpn_protocols = vec![b"h2".to_vec()];
        Self {
            tls: tokio_rustls::TlsConnector::from(Arc::new(config)),
            connections: HashMap::new(),
        }
    }
}

impl Http2 {
    async fn connection(
        &mut self,
        host: &'static str,
    ) -> io::Result<h2::client::SendRequest<Bytes>> {
        if let Some(open) = self.connections.remove(host)
            && let Ok(ready) = open.ready().await
        {
            self.connections.insert(host, ready.clone());
            return Ok(ready);
        }
        let tcp = tokio::net::TcpStream::connect((host, 443)).await?;
        let name = rustls::pki_types::ServerName::try_from(host)
            .map_err(|_| io::Error::other("invalid APNs host"))?;
        let tls = self.tls.connect(name, tcp).await?;
        let (send, connection) = h2::client::handshake(tls).await.map_err(io::Error::other)?;
        tokio::spawn(async move {
            // A broken connection fails `ready()` on the next exchange, which re-dials
            // and reports the error to the caller there.
            let _ = connection.await;
        });
        let ready = send.ready().await.map_err(io::Error::other)?;
        self.connections.insert(host, ready.clone());
        Ok(ready)
    }

    async fn exchange(&mut self, request: ApnsRequest) -> io::Result<ApnsResponse> {
        let host = host(&request.environment);
        let mut sender = self.connection(host).await?;
        let mut builder = http::Request::post(format!("https://{host}{}", request.path));
        for (name, value) in &request.headers {
            let mut value = http::HeaderValue::from_str(value).map_err(io::Error::other)?;
            // Never index credentials in the HPACK table.
            value.set_sensitive(*name == "authorization");
            builder = builder.header(*name, value);
        }
        let outgoing = builder.body(()).map_err(io::Error::other)?;
        let (response, mut stream) = sender
            .send_request(outgoing, false)
            .map_err(io::Error::other)?;
        stream
            .send_data(Bytes::from(request.body), true)
            .map_err(io::Error::other)?;
        let response = response.await.map_err(io::Error::other)?;
        let status = response.status().as_u16();
        let mut body = response.into_body();
        let mut bytes = Vec::new();
        while let Some(chunk) = body.data().await {
            let chunk = chunk.map_err(io::Error::other)?;
            // Only fails once the stream is reset, which the next `data()` reports.
            let _ = body.flow_control().release_capacity(chunk.len());
            bytes.extend_from_slice(&chunk);
            if bytes.len() > RESPONSE_BYTES {
                return Err(io::Error::other("APNs response exceeds bound"));
            }
        }
        let reason = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|value| value["reason"].as_str().map(str::to_owned));
        Ok(ApnsResponse { status, reason })
    }
}

impl Transport for Http2 {
    async fn send(&mut self, request: ApnsRequest) -> io::Result<ApnsResponse> {
        let host = host(&request.environment);
        match tokio::time::timeout(REQUEST_TIMEOUT, self.exchange(request)).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(error)) => {
                self.connections.remove(host);
                Err(error)
            }
            // The deadline passed: the connection is dropped and the caller sees `TimedOut`.
            Err(_) => {
                self.connections.remove(host);
                Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "APNs request timed out",
                ))
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ring::signature::{ECDSA_P256_SHA256_FIXED, KeyPair, UnparsedPublicKey};

    fn directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!("sn-apns-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(path.join(CONFIG_DIRECTORY)).unwrap();
        path
    }

    /// Writes a fresh P-256 key in the `.p8` PEM format and a config naming it. The key is
    /// readable by everyone when `exposed`, and by its owner only otherwise.
    pub(crate) fn install(data: &Path, exposed: bool) {
        let der =
            EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
                .unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(der.as_ref());
        let pem = format!("-----BEGIN PRIVATE KEY-----\n{encoded}\n-----END PRIVATE KEY-----\n");
        let push = data.join(CONFIG_DIRECTORY);
        std::fs::write(push.join("AuthKey_TEST.p8"), pem).unwrap();
        soyeht_platform::fs::set_private_file(&push.join("AuthKey_TEST.p8")).unwrap();
        if exposed {
            soyeht_platform::testing::expose_to_everyone(&push.join("AuthKey_TEST.p8")).unwrap();
        }
        std::fs::write(
            push.join(CONFIG_FILE),
            json!({"keyId":"KEY1234567","teamId":"TEAM123456","keyPath":"AuthKey_TEST.p8","topic":"com.example.app"})
                .to_string(),
        )
        .unwrap();
    }

    fn segment(token: &str, index: usize) -> serde_json::Value {
        let part = token.split('.').nth(index).unwrap();
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
    }

    #[test]
    fn absent_or_exposed_credentials_keep_push_off() {
        let data = directory();
        assert_eq!(Credentials::load(&data).err(), Some(ConfigProblem::Missing));
        install(&data, true);
        assert_eq!(
            Credentials::load(&data).err(),
            Some(ConfigProblem::KeyExposed)
        );
        std::fs::write(data.join("push/apns.json"), "{\"keyId\":\"short\"}").unwrap();
        assert_eq!(Credentials::load(&data).err(), Some(ConfigProblem::Invalid));
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn provider_token_is_an_es256_jwt_reused_until_it_ages() {
        let data = directory();
        install(&data, false);
        let credentials = Credentials::load(&data).unwrap();
        let public = credentials.key.public_key().as_ref().to_vec();
        let mut provider = ProviderToken::new(credentials);
        let token = provider.bearer(1_000).unwrap();
        assert_eq!(
            segment(&token, 0),
            json!({"alg":"ES256","kid":"KEY1234567"})
        );
        assert_eq!(segment(&token, 1), json!({"iss":"TEAM123456","iat":1_000}));
        let (input, signature) = token.rsplit_once('.').unwrap();
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &public)
            .verify(
                input.as_bytes(),
                &URL_SAFE_NO_PAD.decode(signature).unwrap(),
            )
            .unwrap();
        assert_eq!(provider.bearer(1_000 + 20 * 60).unwrap(), token);
        let renewed = provider.bearer(1_000 + TOKEN_LIFETIME_SEC).unwrap();
        assert_eq!(segment(&renewed, 1)["iat"], 1_000 + TOKEN_LIFETIME_SEC);
        provider.expire();
        assert_ne!(
            provider.bearer(1_000 + TOKEN_LIFETIME_SEC + 1).unwrap(),
            renewed
        );
        std::fs::remove_dir_all(data).unwrap();
    }
}
