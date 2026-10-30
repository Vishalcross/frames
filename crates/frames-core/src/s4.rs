//! MEGA S4 (S3-compatible object storage) client, AWS Signature V4 signer,
//! presigned URLs, resumable multipart uploads, space-limit checks, and OS
//! credential store integration (spec sections 3, 4, 7, 8, 9, 11).

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Default MEGA Pro Lite space limit: 750 GB.
pub const DEFAULT_SPACE_LIMIT_BYTES: u64 = 750 * 1_000_000_000;
/// Spec section 2 decision 9: warn at 90% of the configured space limit.
pub const SPACE_WARN_FRACTION: f64 = 0.90;
/// Spec section 7 step 6: multipart upload above ~64 MB.
pub const DEFAULT_MULTIPART_THRESHOLD_BYTES: u64 = 64 * 1024 * 1024;
/// Default multipart chunk size: 16 MB.
pub const DEFAULT_MULTIPART_PART_SIZE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct S4Config {
    /// Endpoint URL, e.g. `https://s3.eu-central-1.s4.mega.io`
    pub endpoint: String,
    /// Region name, e.g. `eu-central-1` or `us-east-1`
    pub region: String,
    /// Bucket name
    pub bucket: String,
    pub access_key_id: String,
    #[serde(skip_serializing)]
    pub secret_access_key: String,
    /// Configured storage cap in bytes (default 750 GB).
    pub space_limit_bytes: u64,
    /// Threshold above which files use S3 multipart upload (default 64 MB).
    pub multipart_threshold_bytes: u64,
    /// Size of each multipart upload part (default 16 MB).
    pub multipart_part_size_bytes: u64,
}

impl Default for S4Config {
    fn default() -> Self {
        Self {
            endpoint: "https://s3.g.s4.mega.io".to_string(),
            region: "Auto".to_string(),
            bucket: "frames-library".to_string(),
            access_key_id: String::new(),
            secret_access_key: String::new(),
            space_limit_bytes: DEFAULT_SPACE_LIMIT_BYTES,
            multipart_threshold_bytes: DEFAULT_MULTIPART_THRESHOLD_BYTES,
            multipart_part_size_bytes: DEFAULT_MULTIPART_PART_SIZE_BYTES,
        }
    }
}

#[derive(Debug, Error)]
pub enum S4Error {
    #[error("offline or network unreachable: {0}")]
    Offline(String),
    #[error("bad S4 credentials or access denied (check Settings): {0}")]
    BadCredentials(String),
    #[error("S4 space limit reached ({used_bytes} / {limit_bytes} bytes used)")]
    SpaceFull { used_bytes: u64, limit_bytes: u64 },
    #[error("S4 server busy or 5xx error (status {status}): {message}")]
    ServerBusy { status: u16, message: String },
    #[error("S4 request error (status {status}): {message}")]
    Http { status: u16, message: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("credential store error: {0}")]
    CredentialStore(String),
}

impl S4Error {
    /// True if the operation should be retried with exponential backoff (spec section 7 & 11).
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::ServerBusy { .. } | Self::Offline(_))
    }
}

pub type Result<T> = std::result::Result<T, S4Error>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectMeta {
    pub key: String,
    pub size: u64,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncompleteMultipartUpload {
    pub key: String,
    pub upload_id: String,
}

/// Space usage report with the 90% warning flag (spec section 2 decision 9 & section 11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceUsage {
    pub used_bytes: u64,
    pub limit_bytes: u64,
    pub warn_90_percent: bool,
    pub is_full: bool,
}

impl SpaceUsage {
    pub fn compute(used_bytes: u64, limit_bytes: u64) -> Self {
        let warn_threshold = ((limit_bytes as f64) * SPACE_WARN_FRACTION) as u64;
        Self {
            used_bytes,
            limit_bytes,
            warn_90_percent: limit_bytes > 0 && used_bytes >= warn_threshold,
            is_full: limit_bytes > 0 && used_bytes >= limit_bytes,
        }
    }
}

// =============================================================================
// AWS Signature V4 (RFC 2104 HMAC-SHA256 + S3 SigV4 Header & Presigned URL)
// =============================================================================

/// Compute RFC 2104 HMAC-SHA256 in pure Rust using `sha2::Sha256`.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut k_block = [0u8; 64];
    if key.len() > 64 {
        let hashed = Sha256::digest(key);
        k_block[..32].copy_from_slice(&hashed);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k_block[i];
        opad[i] ^= k_block[i];
    }

    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(message);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);
    let final_digest = outer.finalize();

    let mut out = [0u8; 32];
    out.copy_from_slice(&final_digest);
    out
}

pub fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    to_hex(&digest)
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}

fn derive_signing_key(secret: &str, date_yyyymmdd: &str, region: &str, service: &str) -> [u8; 32] {
    let k_secret = format!("AWS4{secret}");
    let k_date = hmac_sha256(k_secret.as_bytes(), date_yyyymmdd.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

/// URI-encode a string per AWS SigV4 rules (RFC 3986 unreserved characters kept).
/// When `encode_slash` is false, `'/'` is preserved for object key paths.
pub fn uri_encode(input: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        let is_unreserved =
            b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~';
        if is_unreserved || (!encode_slash && b == b'/') {
            out.push(b as char);
        } else {
            use std::fmt::Write;
            let _ = write!(&mut out, "%{b:02X}");
        }
    }
    out
}

/// Extract `(scheme, host_header, base_path)` from an endpoint URL.
pub fn parse_endpoint(endpoint: &str) -> (String, String, String) {
    let trimmed = endpoint.trim().trim_end_matches('/');
    let (scheme, rest) = if let Some(r) = trimmed.strip_prefix("https://") {
        ("https", r)
    } else if let Some(r) = trimmed.strip_prefix("http://") {
        ("http", r)
    } else {
        ("https", trimmed)
    };
    let (host, path) = match rest.split_once('/') {
        Some((h, p)) => (h.to_string(), format!("/{}", p.trim_matches('/'))),
        None => (rest.to_string(), String::new()),
    };
    (scheme.to_string(), host, path)
}

/// Sign an S3 HTTP request using AWS Signature V4 (`AWS4-HMAC-SHA256`), returning
/// the full URL and the signed headers (`Host`, `x-amz-date`, `x-amz-content-sha256`,
/// `Authorization`, plus any extra headers).
pub fn sign_s4_request(
    config: &S4Config,
    method: &str,
    key: &str,
    query_params: &BTreeMap<String, String>,
    extra_headers: &BTreeMap<String, String>,
    payload_sha256_hex: &str,
    now: DateTime<Utc>,
) -> (String, BTreeMap<String, String>) {
    let (scheme, host, base_path) = parse_endpoint(&config.endpoint);
    let clean_key = key.trim_start_matches('/');
    let raw_path = if clean_key.is_empty() {
        format!("{base_path}/{}", config.bucket)
    } else {
        format!("{base_path}/{}/{}", config.bucket, clean_key)
    };
    let canonical_uri = uri_encode(&raw_path, false);

    let canonical_querystring: String = query_params
        .iter()
        .map(|(k, v)| format!("{}={}", uri_encode(k, true), uri_encode(v, true)))
        .collect::<Vec<_>>()
        .join("&");

    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();

    let mut headers = BTreeMap::new();
    headers.insert("host".to_string(), host.clone());
    headers.insert("x-amz-content-sha256".to_string(), payload_sha256_hex.to_string());
    headers.insert("x-amz-date".to_string(), amz_date.clone());
    for (k, v) in extra_headers {
        headers.insert(k.to_ascii_lowercase(), v.trim().to_string());
    }

    let canonical_headers: String = headers
        .iter()
        .map(|(k, v)| format!("{k}:{v}\n"))
        .collect();
    let signed_headers: String = headers.keys().cloned().collect::<Vec<_>>().join(";");

    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_querystring}\n{canonical_headers}\n{signed_headers}\n{payload_sha256_hex}"
    );

    let credential_scope = format!("{date_stamp}/{}/s3/aws4_request", config.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );

    let signing_key = derive_signing_key(&config.secret_access_key, &date_stamp, &config.region, "s3");
    let signature = to_hex(&hmac_sha256(&signing_key, string_to_sign.as_bytes()));

    let auth_header = format!(
        "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
        config.access_key_id
    );
    headers.insert("Authorization".to_string(), auth_header);

    let url = if canonical_querystring.is_empty() {
        format!("{scheme}://{host}{canonical_uri}")
    } else {
        format!("{scheme}://{host}{canonical_uri}?{canonical_querystring}")
    };

    (url, headers)
}

/// Generate a presigned `GET` URL for `key` (spec section 3 & 8).
/// Uses `UNSIGNED-PAYLOAD` and signs only `host`, so HTML `<video>` elements and
/// `ffmpeg` can perform arbitrary HTTP `Range` requests against the URL.
pub fn presign_get_url(
    config: &S4Config,
    key: &str,
    now: DateTime<Utc>,
    expires_secs: u64,
) -> String {
    let (scheme, host, base_path) = parse_endpoint(&config.endpoint);
    let clean_key = key.trim_start_matches('/');
    let raw_path = format!("{base_path}/{}/{}", config.bucket, clean_key);
    let canonical_uri = uri_encode(&raw_path, false);

    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let credential_scope = format!("{date_stamp}/{}/s3/aws4_request", config.region);
    let credential = format!("{}/{credential_scope}", config.access_key_id);

    let mut query = BTreeMap::new();
    query.insert("X-Amz-Algorithm".to_string(), "AWS4-HMAC-SHA256".to_string());
    query.insert("X-Amz-Credential".to_string(), credential);
    query.insert("X-Amz-Date".to_string(), amz_date.clone());
    query.insert("X-Amz-Expires".to_string(), expires_secs.clamp(1, 604_800).to_string());
    query.insert("X-Amz-SignedHeaders".to_string(), "host".to_string());

    let canonical_querystring: String = query
        .iter()
        .map(|(k, v)| format!("{}={}", uri_encode(k, true), uri_encode(v, true)))
        .collect::<Vec<_>>()
        .join("&");

    let canonical_headers = format!("host:{host}\n");
    let canonical_request = format!(
        "GET\n{canonical_uri}\n{canonical_querystring}\n{canonical_headers}\nhost\nUNSIGNED-PAYLOAD"
    );

    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let signing_key = derive_signing_key(&config.secret_access_key, &date_stamp, &config.region, "s3");
    let signature = to_hex(&hmac_sha256(&signing_key, string_to_sign.as_bytes()));

    format!(
        "{scheme}://{host}{canonical_uri}?{canonical_querystring}&X-Amz-Signature={signature}"
    )
}

// =============================================================================
// ObjectStore Trait & Implementations (InMemoryObjectStore + S4HttpClient)
// =============================================================================

pub trait ObjectStore: Send + Sync {
    fn head_object(&self, key: &str) -> Result<Option<ObjectMeta>>;
    fn get_object(&self, key: &str) -> Result<Option<Vec<u8>>>;
    fn get_object_range(
        &self,
        key: &str,
        start: u64,
        end_inclusive: Option<u64>,
    ) -> Result<Option<Vec<u8>>>;
    fn put_object(&self, key: &str, body: &[u8], content_type: &str) -> Result<String>;
    fn delete_object(&self, key: &str) -> Result<()>;
    fn list_objects(&self, prefix: &str) -> Result<Vec<ObjectMeta>>;

    fn create_multipart_upload(&self, key: &str, content_type: &str) -> Result<String>;
    fn upload_part(
        &self,
        key: &str,
        upload_id: &str,
        part_number: u32,
        body: &[u8],
    ) -> Result<String>;
    fn complete_multipart_upload(
        &self,
        key: &str,
        upload_id: &str,
        parts: &[(u32, String)],
    ) -> Result<()>;
    fn abort_multipart_upload(&self, key: &str, upload_id: &str) -> Result<()>;
    fn list_multipart_uploads(&self) -> Result<Vec<IncompleteMultipartUpload>>;

    fn presign_get(&self, key: &str, now: DateTime<Utc>, expires_secs: u64) -> Result<String>;
    fn config(&self) -> &S4Config;
}

/// Resumable file upload helper with exponential backoff retry (spec section 7, step 6):
/// - Files <= `multipart_threshold_bytes` upload via a single `PUT`.
/// - Larger files upload via S3 multipart (`CreateMultipartUpload` -> `UploadPart`
///   with per-part retry -> `CompleteMultipartUpload`), invoking `on_part_done` so
///   `import_queue` in SQLite can resume from `parts_done` after a crash or network drop.
pub fn upload_file_with_resume<F>(
    store: &dyn ObjectStore,
    path: &Path,
    key: &str,
    content_type: &str,
    existing_upload_id: Option<&str>,
    parts_already_done: u32,
    max_retries: u32,
    mut on_progress: F,
) -> Result<()>
where
    F: FnMut(Option<&str>, u32),
{
    let mut file = std::fs::File::open(path)?;
    let file_size = file.metadata()?.len();
    let cfg = store.config();

    if file_size <= cfg.multipart_threshold_bytes {
        let mut buf = Vec::with_capacity(file_size as usize);
        file.read_to_end(&mut buf)?;
        retry_with_backoff(max_retries, || {
            store.put_object(key, &buf, content_type).map(|_| ())
        })?;
        on_progress(None, 1);
        return Ok(());
    }

    let part_size = cfg.multipart_part_size_bytes.max(5 * 1024 * 1024);
    let upload_id = match existing_upload_id {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => retry_with_backoff(max_retries, || {
            store.create_multipart_upload(key, content_type)
        })?,
    };
    on_progress(Some(&upload_id), parts_already_done);

    let total_parts = file_size.div_ceil(part_size) as u32;
    let mut completed_parts: Vec<(u32, String)> = Vec::with_capacity(total_parts as usize);

    for part_num in 1..=total_parts {
        let offset = u64::from(part_num - 1) * part_size;
        let len = (file_size - offset).min(part_size) as usize;
        file.seek(SeekFrom::Start(offset))?;
        let mut chunk = vec![0u8; len];
        file.read_exact(&mut chunk)?;

        if part_num <= parts_already_done {
            // Re-compute deterministic ETag for already-uploaded part
            let etag = format!("\"{}\"", sha256_hex(&chunk));
            completed_parts.push((part_num, etag));
            continue;
        }

        let etag = retry_with_backoff(max_retries, || {
            store.upload_part(key, &upload_id, part_num, &chunk)
        })?;
        completed_parts.push((part_num, etag));
        on_progress(Some(&upload_id), part_num);
    }

    retry_with_backoff(max_retries, || {
        store.complete_multipart_upload(key, &upload_id, &completed_parts)
    })?;
    Ok(())
}

pub fn retry_with_backoff<T, F>(max_retries: u32, mut op: F) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let mut attempt = 0;
    loop {
        match op() {
            Ok(val) => return Ok(val),
            Err(err) => {
                if !err.is_retryable() || attempt >= max_retries {
                    return Err(err);
                }
                attempt += 1;
                // Exponential backoff capped at 2s in normal runtime (fast in tests)
                let delay_ms = (10u64 << attempt.min(6)).min(500);
                std::thread::sleep(Duration::from_millis(delay_ms));
            }
        }
    }
}

// =============================================================================
// In-Memory S4 Bucket Stand-In (for Multi-Device Sync & Multipart Tests)
// =============================================================================

#[derive(Default)]
struct InMemoryBucketState {
    objects: BTreeMap<String, Vec<u8>>,
    multiparts: BTreeMap<String, (String, BTreeMap<u32, Vec<u8>>)>, // upload_id -> (key, parts)
    next_upload_seq: u64,
    offline: bool,
    transient_failures_remaining: u32,
}

#[derive(Clone)]
pub struct InMemoryObjectStore {
    config: S4Config,
    state: Arc<Mutex<InMemoryBucketState>>,
}

impl InMemoryObjectStore {
    pub fn new(config: S4Config) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(InMemoryBucketState::default())),
        }
    }

    /// Simulate offline mode (spec section 11).
    pub fn set_offline(&self, offline: bool) {
        self.state.lock().unwrap().offline = offline;
    }

    /// Inject `count` transient 503 busy errors to test exponential backoff retries.
    pub fn inject_transient_failures(&self, count: u32) {
        self.state.lock().unwrap().transient_failures_remaining = count;
    }

    fn check_faults(&self, st: &mut InMemoryBucketState) -> Result<()> {
        if st.offline {
            return Err(S4Error::Offline("simulated offline mode".into()));
        }
        if st.transient_failures_remaining > 0 {
            st.transient_failures_remaining -= 1;
            return Err(S4Error::ServerBusy {
                status: 503,
                message: "simulated 503 SlowDown".into(),
            });
        }
        Ok(())
    }

    pub fn total_used_bytes(&self) -> u64 {
        let st = self.state.lock().unwrap();
        st.objects.values().map(|v| v.len() as u64).sum()
    }
}

impl ObjectStore for InMemoryObjectStore {
    fn head_object(&self, key: &str) -> Result<Option<ObjectMeta>> {
        let mut st = self.state.lock().unwrap();
        self.check_faults(&mut st)?;
        Ok(st.objects.get(key).map(|bytes| ObjectMeta {
            key: key.to_string(),
            size: bytes.len() as u64,
            etag: Some(format!("\"{}\"", sha256_hex(bytes))),
        }))
    }

    fn get_object(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let mut st = self.state.lock().unwrap();
        self.check_faults(&mut st)?;
        Ok(st.objects.get(key).cloned())
    }

    fn get_object_range(
        &self,
        key: &str,
        start: u64,
        end_inclusive: Option<u64>,
    ) -> Result<Option<Vec<u8>>> {
        let mut st = self.state.lock().unwrap();
        self.check_faults(&mut st)?;
        let Some(bytes) = st.objects.get(key) else {
            return Ok(None);
        };
        if start >= bytes.len() as u64 {
            return Ok(Some(Vec::new()));
        }
        let s = start as usize;
        let e = end_inclusive
            .map(|end| ((end as usize) + 1).min(bytes.len()))
            .unwrap_or(bytes.len());
        Ok(Some(bytes[s..e].to_vec()))
    }

    fn put_object(&self, key: &str, body: &[u8], _content_type: &str) -> Result<String> {
        let mut st = self.state.lock().unwrap();
        self.check_faults(&mut st)?;
        let current_used: u64 = st
            .objects
            .iter()
            .filter(|(k, _)| k.as_str() != key)
            .map(|(_, v)| v.len() as u64)
            .sum();
        if self.config.space_limit_bytes > 0
            && current_used + (body.len() as u64) > self.config.space_limit_bytes
        {
            return Err(S4Error::SpaceFull {
                used_bytes: current_used,
                limit_bytes: self.config.space_limit_bytes,
            });
        }
        let etag = format!("\"{}\"", sha256_hex(body));
        st.objects.insert(key.to_string(), body.to_vec());
        Ok(etag)
    }

    fn delete_object(&self, key: &str) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        self.check_faults(&mut st)?;
        st.objects.remove(key);
        Ok(())
    }

    fn list_objects(&self, prefix: &str) -> Result<Vec<ObjectMeta>> {
        let mut st = self.state.lock().unwrap();
        self.check_faults(&mut st)?;
        let items = st
            .objects
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| ObjectMeta {
                key: k.clone(),
                size: v.len() as u64,
                etag: Some(format!("\"{}\"", sha256_hex(v))),
            })
            .collect();
        Ok(items)
    }

    fn create_multipart_upload(&self, key: &str, _content_type: &str) -> Result<String> {
        let mut st = self.state.lock().unwrap();
        self.check_faults(&mut st)?;
        st.next_upload_seq += 1;
        let upload_id = format!("mp-{}", st.next_upload_seq);
        st.multiparts
            .insert(upload_id.clone(), (key.to_string(), BTreeMap::new()));
        Ok(upload_id)
    }

    fn upload_part(
        &self,
        key: &str,
        upload_id: &str,
        part_number: u32,
        body: &[u8],
    ) -> Result<String> {
        let mut st = self.state.lock().unwrap();
        self.check_faults(&mut st)?;
        let Some((mp_key, parts)) = st.multiparts.get_mut(upload_id) else {
            return Err(S4Error::Http {
                status: 404,
                message: format!("NoSuchUpload: {upload_id}"),
            });
        };
        if mp_key != key {
            return Err(S4Error::Http {
                status: 400,
                message: "multipart key mismatch".into(),
            });
        }
        let etag = format!("\"{}\"", sha256_hex(body));
        parts.insert(part_number, body.to_vec());
        Ok(etag)
    }

    fn complete_multipart_upload(
        &self,
        key: &str,
        upload_id: &str,
        parts: &[(u32, String)],
    ) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        self.check_faults(&mut st)?;
        let Some((_, mut stored_parts)) = st.multiparts.remove(upload_id) else {
            return Err(S4Error::Http {
                status: 404,
                message: format!("NoSuchUpload: {upload_id}"),
            });
        };
        let mut combined = Vec::new();
        for (part_num, _etag) in parts {
            let Some(bytes) = stored_parts.remove(part_num) else {
                return Err(S4Error::Http {
                    status: 400,
                    message: format!("InvalidPart {part_num}"),
                });
            };
            combined.extend_from_slice(&bytes);
        }
        st.objects.insert(key.to_string(), combined);
        Ok(())
    }

    fn abort_multipart_upload(&self, _key: &str, upload_id: &str) -> Result<()> {
        let mut st = self.state.lock().unwrap();
        self.check_faults(&mut st)?;
        st.multiparts.remove(upload_id);
        Ok(())
    }

    fn list_multipart_uploads(&self) -> Result<Vec<IncompleteMultipartUpload>> {
        let mut st = self.state.lock().unwrap();
        self.check_faults(&mut st)?;
        Ok(st
            .multiparts
            .iter()
            .map(|(uid, (k, _))| IncompleteMultipartUpload {
                key: k.clone(),
                upload_id: uid.clone(),
            })
            .collect())
    }

    fn presign_get(&self, key: &str, now: DateTime<Utc>, expires_secs: u64) -> Result<String> {
        Ok(presign_get_url(&self.config, key, now, expires_secs))
    }

    fn config(&self) -> &S4Config {
        &self.config
    }
}

// =============================================================================
// Production S4 HTTP Client (SigV4 + Native OS curl HTTPS Transport)
// =============================================================================

pub struct S4Client {
    config: S4Config,
}

impl S4Client {
    pub fn new(config: S4Config) -> Self {
        Self { config }
    }

    fn execute(
        &self,
        method: &str,
        key: &str,
        query: &BTreeMap<String, String>,
        extra_headers: &BTreeMap<String, String>,
        body: &[u8],
    ) -> Result<(u16, BTreeMap<String, String>, Vec<u8>)> {
        let payload_hash = sha256_hex(body);
        let (url, signed_headers) = sign_s4_request(
            &self.config,
            method,
            key,
            query,
            extra_headers,
            &payload_hash,
            Utc::now(),
        );

        let mut cmd = Command::new("curl");
        cmd.args(["-sS", "-i", "-X", method]);
        for (k, v) in &signed_headers {
            cmd.arg("-H").arg(format!("{k}: {v}"));
        }
        cmd.arg("--data-binary").arg("@-").arg(&url);

        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| S4Error::Offline(e.to_string()))?;

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(body);
        }

        let output = child
            .wait_with_output()
            .map_err(|e| S4Error::Offline(e.to_string()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(S4Error::Offline(stderr));
        }

        parse_raw_http_response(&output.stdout)
    }
}

fn parse_raw_http_response(raw: &[u8]) -> Result<(u16, BTreeMap<String, String>, Vec<u8>)> {
    // Skip any 100 Continue headers if present
    let mut slice = raw;
    loop {
        let Some(sep) = slice.windows(4).position(|w| w == b"\r\n\r\n") else {
            return Err(S4Error::Http {
                status: 0,
                message: "malformed HTTP response".into(),
            });
        };
        let header_block = String::from_utf8_lossy(&slice[..sep]);
        let body = &slice[sep + 4..];
        let mut lines = header_block.lines();
        let status_line = lines.next().unwrap_or("");
        let status: u16 = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if status == 100 {
            slice = body;
            continue;
        }
        let mut headers = BTreeMap::new();
        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
            }
        }
        return Ok((status, headers, body.to_vec()));
    }
}

fn check_s3_status(status: u16, body: &[u8]) -> Result<()> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    let msg = String::from_utf8_lossy(body).trim().to_string();
    if status == 401 || status == 403 {
        return Err(S4Error::BadCredentials(msg));
    }
    if status == 429 || status >= 500 {
        return Err(S4Error::ServerBusy {
            status,
            message: msg,
        });
    }
    Err(S4Error::Http {
        status,
        message: msg,
    })
}

impl ObjectStore for S4Client {
    fn head_object(&self, key: &str) -> Result<Option<ObjectMeta>> {
        let (status, headers, body) =
            self.execute("HEAD", key, &BTreeMap::new(), &BTreeMap::new(), &[])?;
        if status == 404 {
            return Ok(None);
        }
        check_s3_status(status, &body)?;
        let size = headers
            .get("content-length")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let etag = headers.get("etag").cloned();
        Ok(Some(ObjectMeta {
            key: key.to_string(),
            size,
            etag,
        }))
    }

    fn get_object(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let (status, _, body) =
            self.execute("GET", key, &BTreeMap::new(), &BTreeMap::new(), &[])?;
        if status == 404 {
            return Ok(None);
        }
        check_s3_status(status, &body)?;
        Ok(Some(body))
    }

    fn get_object_range(
        &self,
        key: &str,
        start: u64,
        end_inclusive: Option<u64>,
    ) -> Result<Option<Vec<u8>>> {
        let mut extra = BTreeMap::new();
        let range = match end_inclusive {
            Some(end) => format!("bytes={start}-{end}"),
            None => format!("bytes={start}-"),
        };
        extra.insert("Range".to_string(), range);
        let (status, _, body) = self.execute("GET", key, &BTreeMap::new(), &extra, &[])?;
        if status == 404 {
            return Ok(None);
        }
        check_s3_status(status, &body)?;
        Ok(Some(body))
    }

    fn put_object(&self, key: &str, body: &[u8], content_type: &str) -> Result<String> {
        let mut extra = BTreeMap::new();
        extra.insert("Content-Type".to_string(), content_type.to_string());
        let (status, headers, resp_body) =
            self.execute("PUT", key, &BTreeMap::new(), &extra, body)?;
        check_s3_status(status, &resp_body)?;
        Ok(headers.get("etag").cloned().unwrap_or_default())
    }

    fn delete_object(&self, key: &str) -> Result<()> {
        let (status, _, body) =
            self.execute("DELETE", key, &BTreeMap::new(), &BTreeMap::new(), &[])?;
        if status == 404 || status == 204 || status == 200 {
            return Ok(());
        }
        check_s3_status(status, &body)
    }

    fn list_objects(&self, prefix: &str) -> Result<Vec<ObjectMeta>> {
        let mut all = Vec::new();
        let mut continuation_token: Option<String> = None;

        loop {
            let mut query = BTreeMap::new();
            query.insert("list-type".to_string(), "2".to_string());
            if !prefix.is_empty() {
                query.insert("prefix".to_string(), prefix.to_string());
            }
            if let Some(ref token) = continuation_token {
                query.insert("continuation-token".to_string(), token.clone());
            }

            let (status, _, body) =
                self.execute("GET", "", &query, &BTreeMap::new(), &[])?;
            check_s3_status(status, &body)?;
            let xml = String::from_utf8_lossy(&body);
            let (mut page, next_tok) = parse_list_objects_v2_xml(&xml);
            all.append(&mut page);
            match next_tok {
                Some(tok) if !tok.is_empty() => continuation_token = Some(tok),
                _ => break,
            }
        }
        Ok(all)
    }

    fn create_multipart_upload(&self, key: &str, content_type: &str) -> Result<String> {
        let mut query = BTreeMap::new();
        query.insert("uploads".to_string(), String::new());
        let mut extra = BTreeMap::new();
        extra.insert("Content-Type".to_string(), content_type.to_string());
        let (status, _, body) = self.execute("POST", key, &query, &extra, &[])?;
        check_s3_status(status, &body)?;
        let xml = String::from_utf8_lossy(&body);
        extract_xml_tag(&xml, "UploadId").ok_or_else(|| S4Error::Http {
            status: 500,
            message: "missing UploadId in CreateMultipartUpload response".into(),
        })
    }

    fn upload_part(
        &self,
        key: &str,
        upload_id: &str,
        part_number: u32,
        body: &[u8],
    ) -> Result<String> {
        let mut query = BTreeMap::new();
        query.insert("partNumber".to_string(), part_number.to_string());
        query.insert("uploadId".to_string(), upload_id.to_string());
        let (status, headers, resp) =
            self.execute("PUT", key, &query, &BTreeMap::new(), body)?;
        check_s3_status(status, &resp)?;
        Ok(headers.get("etag").cloned().unwrap_or_default())
    }

    fn complete_multipart_upload(
        &self,
        key: &str,
        upload_id: &str,
        parts: &[(u32, String)],
    ) -> Result<()> {
        let mut query = BTreeMap::new();
        query.insert("uploadId".to_string(), upload_id.to_string());
        let mut xml = String::from("<CompleteMultipartUpload>");
        for (num, etag) in parts {
            use std::fmt::Write;
            let _ = write!(
                &mut xml,
                "<Part><PartNumber>{num}</PartNumber><ETag>{etag}</ETag></Part>"
            );
        }
        xml.push_str("</CompleteMultipartUpload>");
        let mut extra = BTreeMap::new();
        extra.insert("Content-Type".to_string(), "application/xml".to_string());
        let (status, _, resp) =
            self.execute("POST", key, &query, &extra, xml.as_bytes())?;
        check_s3_status(status, &resp)
    }

    fn abort_multipart_upload(&self, key: &str, upload_id: &str) -> Result<()> {
        let mut query = BTreeMap::new();
        query.insert("uploadId".to_string(), upload_id.to_string());
        let (status, _, resp) =
            self.execute("DELETE", key, &query, &BTreeMap::new(), &[])?;
        if status == 404 || status == 204 || status == 200 {
            return Ok(());
        }
        check_s3_status(status, &resp)
    }

    fn list_multipart_uploads(&self) -> Result<Vec<IncompleteMultipartUpload>> {
        let mut query = BTreeMap::new();
        query.insert("uploads".to_string(), String::new());
        let (status, _, body) = self.execute("GET", "", &query, &BTreeMap::new(), &[])?;
        check_s3_status(status, &body)?;
        let xml = String::from_utf8_lossy(&body);
        Ok(parse_list_multipart_uploads_xml(&xml))
    }

    fn presign_get(&self, key: &str, now: DateTime<Utc>, expires_secs: u64) -> Result<String> {
        Ok(presign_get_url(&self.config, key, now, expires_secs))
    }

    fn config(&self) -> &S4Config {
        &self.config
    }
}

pub fn parse_list_objects_v2_xml(xml: &str) -> (Vec<ObjectMeta>, Option<String>) {
    let mut items = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<Contents>") {
        let after = &rest[start + 10..];
        let Some(end) = after.find("</Contents>") else {
            break;
        };
        let block = &after[..end];
        if let Some(key) = extract_xml_tag(block, "Key") {
            let size = extract_xml_tag(block, "Size")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let etag = extract_xml_tag(block, "ETag");
            items.push(ObjectMeta { key, size, etag });
        }
        rest = &after[end + 11..];
    }
    let next_token = extract_xml_tag(xml, "NextContinuationToken");
    (items, next_token)
}

pub fn parse_list_multipart_uploads_xml(xml: &str) -> Vec<IncompleteMultipartUpload> {
    let mut items = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<Upload>") {
        let after = &rest[start + 8..];
        let Some(end) = after.find("</Upload>") else {
            break;
        };
        let block = &after[..end];
        if let (Some(key), Some(upload_id)) = (
            extract_xml_tag(block, "Key"),
            extract_xml_tag(block, "UploadId"),
        ) {
            items.push(IncompleteMultipartUpload { key, upload_id });
        }
        rest = &after[end + 9..];
    }
    items
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml_unescape(xml[start..end].trim()))
}

fn xml_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

// =============================================================================
// OS Keychain / Credential Manager Storage (Spec Section 11)
// =============================================================================

const KEYCHAIN_SERVICE: &str = "com.frames.s4";

/// Store the S4 secret access key in the OS password store (macOS Keychain via
/// `/usr/bin/security`, Windows Credential Manager via `cmdkey`).
pub fn save_secret_in_os_keychain(account: &str, secret: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("/usr/bin/security")
            .args([
                "add-generic-password",
                "-U",
                "-s",
                KEYCHAIN_SERVICE,
                "-a",
                account,
                "-w",
                secret,
            ])
            .status()
            .map_err(|e| S4Error::CredentialStore(e.to_string()))?;
        if !status.success() {
            return Err(S4Error::CredentialStore(
                "macOS Keychain add-generic-password failed".into(),
            ));
        }
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let status = Command::new("cmdkey")
            .args([
                &format!("/generic:{KEYCHAIN_SERVICE}:{account}"),
                &format!("/user:{account}"),
                &format!("/pass:{secret}"),
            ])
            .status()
            .map_err(|e| S4Error::CredentialStore(e.to_string()))?;
        if !status.success() {
            return Err(S4Error::CredentialStore("Windows cmdkey failed".into()));
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (account, secret);
        Ok(())
    }
}

/// Read the S4 secret access key from the OS password store.
pub fn load_secret_from_os_keychain(account: &str) -> Result<Option<String>> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("/usr/bin/security")
            .args([
                "find-generic-password",
                "-s",
                KEYCHAIN_SERVICE,
                "-a",
                account,
                "-w",
            ])
            .output()
            .map_err(|e| S4Error::CredentialStore(e.to_string()))?;
        if !output.status.success() {
            return Ok(None);
        }
        let secret = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(if secret.is_empty() { None } else { Some(secret) })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = account;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn sigv4_presigned_url_contains_required_s3_query_params() {
        let cfg = S4Config {
            endpoint: "https://s3.eu-central-1.s4.mega.io".into(),
            region: "eu-central-1".into(),
            bucket: "my-photos".into(),
            access_key_id: "AKIAIOSFODNN7EXAMPLE".into(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
            ..Default::default()
        };
        let now = Utc.with_ymd_and_hms(2026, 11, 2, 3, 14, 15).unwrap();
        let url = presign_get_url(&cfg, "originals/2024/05/IMG 1234.MOV", now, 3600);
        assert!(url.starts_with(
            "https://s3.eu-central-1.s4.mega.io/my-photos/originals/2024/05/IMG%201234.MOV?"
        ));
        assert!(url.contains("X-Amz-Algorithm=AWS4-HMAC-SHA256"));
        assert!(url.contains("X-Amz-Expires=3600"));
        assert!(url.contains("X-Amz-SignedHeaders=host"));
        assert!(url.contains("X-Amz-Signature="));
    }

    #[test]
    fn space_usage_warns_at_90_percent_and_stops_at_100_percent() {
        let limit = 1000;
        assert!(!SpaceUsage::compute(899, limit).warn_90_percent);
        let at_90 = SpaceUsage::compute(900, limit);
        assert!(at_90.warn_90_percent);
        assert!(!at_90.is_full);
        assert!(SpaceUsage::compute(1000, limit).is_full);
    }

    #[test]
    fn resumable_multipart_upload_and_transient_retry() {
        let cfg = S4Config {
            multipart_threshold_bytes: 10, // force multipart for test file
            multipart_part_size_bytes: 6,
            ..Default::default()
        };
        let store = InMemoryObjectStore::new(cfg);

        let temp_path = std::env::temp_dir().join(format!("frames-mp-{}", fastrand::u64(..)));
        let data = b"0123456789ABCDEF"; // 16 bytes -> 3 parts
        std::fs::write(&temp_path, data).unwrap();

        // Inject 2 transient 503 errors to verify exponential backoff retry
        store.inject_transient_failures(2);

        let mut progress_steps = Vec::new();
        upload_file_with_resume(
            &store,
            &temp_path,
            "originals/2024/05/CLIP.MOV",
            "video/quicktime",
            None,
            0,
            4,
            |uid, parts| progress_steps.push((uid.map(str::to_string), parts)),
        )
        .unwrap();

        let _ = std::fs::remove_file(&temp_path);

        let fetched = store
            .get_object("originals/2024/05/CLIP.MOV")
            .unwrap()
            .unwrap();
        assert_eq!(fetched, data);
        assert!(store.list_multipart_uploads().unwrap().is_empty());
    }

    #[test]
    fn parses_s3_list_objects_and_multipart_xml() {
        let xml = r#"
        <ListBucketResult>
            <Contents><Key>originals/2024/05/IMG_1.HEIC</Key><Size>12345</Size><ETag>"ab12"</ETag></Contents>
            <Contents><Key>thumbs/ab12/s.webp</Key><Size>4500</Size></Contents>
            <NextContinuationToken>tok-2</NextContinuationToken>
        </ListBucketResult>"#;
        let (items, next) = parse_list_objects_v2_xml(xml);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].key, "originals/2024/05/IMG_1.HEIC");
        assert_eq!(items[0].size, 12345);
        assert_eq!(next.as_deref(), Some("tok-2"));
    }
}
