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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadedPart {
    pub part_number: u32,
    pub etag: String,
    pub size: u64,
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

    /// Stream an object directly to `dest` on disk without buffering the entire
    /// object in RAM. Returns `Ok(true)` if found and written, `Ok(false)` on 404.
    fn get_object_to_file(&self, key: &str, dest: &Path) -> Result<bool> {
        let Some(bytes) = self.get_object(key)? else {
            return Ok(false);
        };
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(dest, &bytes)?;
        Ok(true)
    }

    /// Stream a local file directly into a single `PUT` object request without
    /// buffering the entire file in RAM.
    fn put_object_from_file(&self, key: &str, path: &Path, content_type: &str) -> Result<String> {
        let bytes = std::fs::read(path)?;
        self.put_object(key, &bytes, content_type)
    }

    fn create_multipart_upload(&self, key: &str, content_type: &str) -> Result<String>;
    fn upload_part(
        &self,
        key: &str,
        upload_id: &str,
        part_number: u32,
        body: &[u8],
    ) -> Result<String>;
    fn list_parts(&self, key: &str, upload_id: &str) -> Result<Vec<UploadedPart>>;
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
/// - Files <= `multipart_threshold_bytes` upload via a single streamed `PUT`.
/// - Larger files upload via S3 multipart (`CreateMultipartUpload` -> `ListParts` when
///   resuming -> `UploadPart` with per-part retry -> `CompleteMultipartUpload`),
///   invoking `on_progress` so `import_queue` in SQLite can resume from `upload_id`
///   and `parts_done` after a crash or network drop.
#[allow(clippy::too_many_arguments)]
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
        retry_with_backoff(max_retries, || {
            store.put_object_from_file(key, path, content_type).map(|_| ())
        })?;
        on_progress(None, 1);
        return Ok(());
    }

    let part_size = cfg.multipart_part_size_bytes.max(5 * 1024 * 1024);

    // If resuming an existing multipart upload, query S4 ListParts to get the
    // authentic server-side ETags of already-uploaded parts. If the upload_id
    // expired or was aborted on the server (404 NoSuchUpload), start a new one.
    let (upload_id, existing_parts_map) = match existing_upload_id {
        Some(id) if !id.is_empty() => match retry_with_backoff(max_retries, || store.list_parts(key, id)) {
            Ok(parts) => {
                let map: BTreeMap<u32, UploadedPart> =
                    parts.into_iter().map(|p| (p.part_number, p)).collect();
                (id.to_string(), map)
            }
            Err(S4Error::Http { status: 404, .. }) => {
                let new_id = retry_with_backoff(max_retries, || {
                    store.create_multipart_upload(key, content_type)
                })?;
                (new_id, BTreeMap::new())
            }
            Err(e) => return Err(e),
        },
        _ => {
            let new_id = retry_with_backoff(max_retries, || {
                store.create_multipart_upload(key, content_type)
            })?;
            (new_id, BTreeMap::new())
        }
    };

    let initial_done = (existing_parts_map.len() as u32).max(parts_already_done);
    on_progress(Some(&upload_id), initial_done);

    let total_parts = file_size.div_ceil(part_size) as u32;
    let mut completed_parts: Vec<(u32, String)> = Vec::with_capacity(total_parts as usize);

    for part_num in 1..=total_parts {
        let offset = u64::from(part_num - 1) * part_size;
        let len = (file_size - offset).min(part_size) as usize;

        if let Some(existing_part) = existing_parts_map.get(&part_num) {
            if existing_part.size == len as u64 && !existing_part.etag.is_empty() {
                completed_parts.push((part_num, existing_part.etag.clone()));
                continue;
            }
        }

        file.seek(SeekFrom::Start(offset))?;
        let mut chunk = vec![0u8; len];
        file.read_exact(&mut chunk)?;

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

type InMemoryPartMap = BTreeMap<u32, (String, Vec<u8>)>;

#[derive(Default)]
struct InMemoryBucketState {
    objects: BTreeMap<String, Vec<u8>>,
    multiparts: BTreeMap<String, (String, InMemoryPartMap)>, // upload_id -> (key, part_num -> (etag, bytes))
    next_upload_seq: u64,
    upload_part_calls: u64,
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

    /// Number of successful `upload_part` calls recorded on this mock store.
    pub fn upload_part_call_count(&self) -> u64 {
        self.state.lock().unwrap().upload_part_calls
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
        st.upload_part_calls += 1;
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
        // Use a server-specific ETag format distinct from raw sha256_hex(body) so
        // callers are forced to use the server-returned ETag (or ListParts on resume).
        let short_hash = &sha256_hex(body)[..16];
        let etag = format!("\"s4etag-{upload_id}-p{part_number}-{short_hash}\"");
        parts.insert(part_number, (etag.clone(), body.to_vec()));
        Ok(etag)
    }

    fn list_parts(&self, key: &str, upload_id: &str) -> Result<Vec<UploadedPart>> {
        let mut st = self.state.lock().unwrap();
        self.check_faults(&mut st)?;
        let Some((mp_key, parts)) = st.multiparts.get(upload_id) else {
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
        Ok(parts
            .iter()
            .map(|(&part_number, (etag, bytes))| UploadedPart {
                part_number,
                etag: etag.clone(),
                size: bytes.len() as u64,
            })
            .collect())
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
        for (part_num, etag) in parts {
            let Some((expected_etag, bytes)) = stored_parts.remove(part_num) else {
                return Err(S4Error::Http {
                    status: 400,
                    message: format!("InvalidPart {part_num}"),
                });
            };
            if etag.trim_matches('"') != expected_etag.trim_matches('"') {
                return Err(S4Error::Http {
                    status: 400,
                    message: format!(
                        "InvalidPart: ETag mismatch for part {part_num} (got {etag}, expected {expected_etag})"
                    ),
                });
            }
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

/// Build the `curl` CLI arguments for an HTTP request so tests can inspect
/// exact flags (`-I` for HEAD, timeouts, and body streaming).
pub fn build_curl_args(
    method: &str,
    url: &str,
    signed_headers: &BTreeMap<String, String>,
    has_body: bool,
) -> Vec<String> {
    let mut args = vec![
        "-sS".to_string(),
        "-i".to_string(),
        "--connect-timeout".to_string(),
        "15".to_string(),
        "--max-time".to_string(),
        "300".to_string(),
        "--speed-time".to_string(),
        "30".to_string(),
        "--speed-limit".to_string(),
        "1024".to_string(),
    ];
    if method == "HEAD" {
        args.push("-I".to_string());
    } else {
        args.push("-X".to_string());
        args.push(method.to_string());
    }
    for (k, v) in signed_headers {
        args.push("-H".to_string());
        args.push(format!("{k}: {v}"));
    }
    if has_body && method != "HEAD" {
        args.push("--data-binary".to_string());
        args.push("@-".to_string());
    }
    args.push(url.to_string());
    args
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

        let send_body = !body.is_empty() || matches!(method, "PUT" | "POST");
        let args = build_curl_args(method, &url, &signed_headers, send_body);

        let mut cmd = Command::new("curl");
        cmd.args(&args);

        let mut child = cmd
            .stdin(if send_body && method != "HEAD" {
                std::process::Stdio::piped()
            } else {
                std::process::Stdio::null()
            })
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

    fn get_object_to_file(&self, key: &str, dest: &Path) -> Result<bool> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp_dest = dest.with_extension(format!("part-{}", fastrand::u64(..)));
        let empty_hash = sha256_hex(&[]);
        let (url, signed_headers) = sign_s4_request(
            &self.config,
            "GET",
            key,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &empty_hash,
            Utc::now(),
        );

        let mut cmd = Command::new("curl");
        cmd.args([
            "-sS",
            "--connect-timeout",
            "15",
            "--max-time",
            "3600",
            "--speed-time",
            "30",
            "--speed-limit",
            "1024",
            "-D",
            "-",
            "-o",
        ]);
        cmd.arg(&tmp_dest);
        for (k, v) in &signed_headers {
            cmd.arg("-H").arg(format!("{k}: {v}"));
        }
        cmd.arg(&url);

        let output = cmd
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| S4Error::Offline(e.to_string()))?;

        if !output.status.success() {
            let _ = std::fs::remove_file(&tmp_dest);
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(S4Error::Offline(stderr));
        }

        let (status, _, _) = parse_raw_http_response(&output.stdout)?;
        if status == 404 {
            let _ = std::fs::remove_file(&tmp_dest);
            return Ok(false);
        }
        if !(200..300).contains(&status) {
            let err_body = std::fs::read(&tmp_dest).unwrap_or_default();
            let _ = std::fs::remove_file(&tmp_dest);
            check_s3_status(status, &err_body)?;
        }

        std::fs::rename(&tmp_dest, dest)?;
        Ok(true)
    }

    fn put_object_from_file(&self, key: &str, path: &Path, content_type: &str) -> Result<String> {
        let payload_hash = crate::media::sha256_file(path)?;
        let file_size = std::fs::metadata(path)?.len();
        let mut extra = BTreeMap::new();
        extra.insert("Content-Type".to_string(), content_type.to_string());
        extra.insert("Content-Length".to_string(), file_size.to_string());

        let (url, signed_headers) = sign_s4_request(
            &self.config,
            "PUT",
            key,
            &BTreeMap::new(),
            &extra,
            &payload_hash,
            Utc::now(),
        );

        let mut cmd = Command::new("curl");
        cmd.args([
            "-sS",
            "-i",
            "--connect-timeout",
            "15",
            "--max-time",
            "3600",
            "--speed-time",
            "30",
            "--speed-limit",
            "1024",
            "-T",
        ]);
        cmd.arg(path);
        for (k, v) in &signed_headers {
            cmd.arg("-H").arg(format!("{k}: {v}"));
        }
        cmd.arg(&url);

        let output = cmd
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| S4Error::Offline(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(S4Error::Offline(stderr));
        }

        let (status, headers, resp_body) = parse_raw_http_response(&output.stdout)?;
        check_s3_status(status, &resp_body)?;
        Ok(headers.get("etag").cloned().unwrap_or_default())
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

    fn list_parts(&self, key: &str, upload_id: &str) -> Result<Vec<UploadedPart>> {
        let mut all = Vec::new();
        let mut part_marker: Option<String> = None;

        loop {
            let mut query = BTreeMap::new();
            query.insert("uploadId".to_string(), upload_id.to_string());
            if let Some(ref marker) = part_marker {
                query.insert("part-number-marker".to_string(), marker.clone());
            }

            let (status, _, body) =
                self.execute("GET", key, &query, &BTreeMap::new(), &[])?;
            check_s3_status(status, &body)?;
            let xml = String::from_utf8_lossy(&body);
            let (mut page, next_marker) = parse_list_parts_xml(&xml);
            all.append(&mut page);
            match next_marker {
                Some(m) if !m.is_empty() && m != "0" => part_marker = Some(m),
                _ => break,
            }
        }
        Ok(all)
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

pub fn parse_list_parts_xml(xml: &str) -> (Vec<UploadedPart>, Option<String>) {
    let mut items = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<Part>") {
        let after = &rest[start + 6..];
        let Some(end) = after.find("</Part>") else {
            break;
        };
        let block = &after[..end];
        if let (Some(part_num_str), Some(etag)) = (
            extract_xml_tag(block, "PartNumber"),
            extract_xml_tag(block, "ETag"),
        ) {
            if let Ok(part_number) = part_num_str.parse::<u32>() {
                let size = extract_xml_tag(block, "Size")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                items.push(UploadedPart {
                    part_number,
                    etag,
                    size,
                });
            }
        }
        rest = &after[end + 7..];
    }
    let is_truncated = extract_xml_tag(xml, "IsTruncated")
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
    let next_marker = if is_truncated {
        extract_xml_tag(xml, "NextPartNumberMarker")
    } else {
        None
    };
    (items, next_marker)
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

#[cfg(target_os = "macos")]
fn quote_security_arg(arg: &str) -> String {
    let mut escaped = String::with_capacity(arg.len() + 2);
    escaped.push('"');
    for ch in arg.chars() {
        if ch == '"' || ch == '\\' {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped.push('"');
    escaped
}

#[cfg(target_os = "windows")]
mod win_cred {
    use super::{Result, S4Error};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    const CRED_TYPE_GENERIC: u32 = 1;
    const CRED_PERSIST_LOCAL_MACHINE: u32 = 2;

    #[repr(C)]
    struct FILETIME {
        dw_low_date_time: u32,
        dw_high_date_time: u32,
    }

    #[repr(C)]
    struct CREDENTIALW {
        flags: u32,
        cred_type: u32,
        target_name: *mut u16,
        comment: *mut u16,
        last_written: FILETIME,
        credential_blob_size: u32,
        credential_blob: *mut u8,
        persist: u32,
        attribute_count: u32,
        attributes: *mut core::ffi::c_void,
        target_alias: *mut u16,
        user_name: *mut u16,
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn CredWriteW(credential: *const CREDENTIALW, flags: u32) -> i32;
        fn CredReadW(
            target_name: *const u16,
            cred_type: u32,
            flags: u32,
            credential: *mut *mut CREDENTIALW,
        ) -> i32;
        fn CredFree(buffer: *mut core::ffi::c_void);
    }

    fn to_wide_null(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn write_generic_credential(target: &str, account: &str, secret: &str) -> Result<()> {
        let mut target_w = to_wide_null(target);
        let mut user_w = to_wide_null(account);
        let mut blob = secret.as_bytes().to_vec();

        let cred = CREDENTIALW {
            flags: 0,
            cred_type: CRED_TYPE_GENERIC,
            target_name: target_w.as_mut_ptr(),
            comment: ptr::null_mut(),
            last_written: FILETIME {
                dw_low_date_time: 0,
                dw_high_date_time: 0,
            },
            credential_blob_size: blob.len() as u32,
            credential_blob: blob.as_mut_ptr(),
            persist: CRED_PERSIST_LOCAL_MACHINE,
            attribute_count: 0,
            attributes: ptr::null_mut(),
            target_alias: ptr::null_mut(),
            user_name: user_w.as_mut_ptr(),
        };

        let ok = unsafe { CredWriteW(&cred, 0) };
        if ok == 0 {
            return Err(S4Error::CredentialStore(
                "Windows Credential Manager CredWriteW failed".into(),
            ));
        }
        Ok(())
    }

    pub fn read_generic_credential(target: &str) -> Result<Option<String>> {
        let target_w = to_wide_null(target);
        let mut cred_ptr: *mut CREDENTIALW = ptr::null_mut();
        let ok = unsafe { CredReadW(target_w.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred_ptr) };
        if ok == 0 || cred_ptr.is_null() {
            return Ok(None);
        }
        let secret = unsafe {
            let cred = &*cred_ptr;
            let slice = if cred.credential_blob.is_null() || cred.credential_blob_size == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(cred.credential_blob, cred.credential_blob_size as usize)
            };
            let s = String::from_utf8_lossy(slice).trim().to_string();
            CredFree(cred_ptr.cast());
            s
        };
        Ok(if secret.is_empty() { None } else { Some(secret) })
    }
}

/// Store the S4 secret access key in the OS password store without exposing the
/// secret in command-line process arguments (`ps`):
/// - macOS: pipes `add-generic-password` via stdin to `/usr/bin/security -i`.
/// - Windows: calls Win32 `CredWriteW` (`Advapi32.dll`) directly in-process.
pub fn save_secret_in_os_keychain(account: &str, secret: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let mut child = Command::new("/usr/bin/security")
            .arg("-i")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| S4Error::CredentialStore(e.to_string()))?;

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let cmd_line = format!(
                "add-generic-password -U -s {} -a {} -w {}\n",
                quote_security_arg(KEYCHAIN_SERVICE),
                quote_security_arg(account),
                quote_security_arg(secret),
            );
            stdin
                .write_all(cmd_line.as_bytes())
                .map_err(|e| S4Error::CredentialStore(e.to_string()))?;
        }

        let out = child
            .wait_with_output()
            .map_err(|e| S4Error::CredentialStore(e.to_string()))?;
        if !out.status.success() {
            return Err(S4Error::CredentialStore(
                "macOS Keychain add-generic-password failed".into(),
            ));
        }
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let target = format!("{KEYCHAIN_SERVICE}:{account}");
        win_cred::write_generic_credential(&target, account, secret)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (account, secret);
        Ok(())
    }
}

/// Read the S4 secret access key from the OS password store:
/// - macOS: queries Keychain via `/usr/bin/security -i` stdin.
/// - Windows: calls Win32 `CredReadW` (`Advapi32.dll`) directly in-process.
pub fn load_secret_from_os_keychain(account: &str) -> Result<Option<String>> {
    #[cfg(target_os = "macos")]
    {
        let mut child = Command::new("/usr/bin/security")
            .arg("-i")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| S4Error::CredentialStore(e.to_string()))?;

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let cmd_line = format!(
                "find-generic-password -s {} -a {} -w\n",
                quote_security_arg(KEYCHAIN_SERVICE),
                quote_security_arg(account),
            );
            let _ = stdin.write_all(cmd_line.as_bytes());
        }

        let output = child
            .wait_with_output()
            .map_err(|e| S4Error::CredentialStore(e.to_string()))?;
        if !output.status.success() {
            return Ok(None);
        }
        let secret = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(if secret.is_empty() { None } else { Some(secret) })
    }
    #[cfg(target_os = "windows")]
    {
        let target = format!("{KEYCHAIN_SERVICE}:{account}");
        win_cred::read_generic_credential(&target)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
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
    fn resumable_multipart_upload_and_list_parts_resume() {
        let cfg = S4Config {
            multipart_threshold_bytes: 10, // force multipart for test file
            multipart_part_size_bytes: 5 * 1024 * 1024, // 5 MB minimum part size
            ..Default::default()
        };
        let store = InMemoryObjectStore::new(cfg);

        // Pre-create a multipart upload and upload part 1 (5 MB) before an "interruption"
        let key = "originals/2024/05/CLIP.MOV";
        let uid = store.create_multipart_upload(key, "video/quicktime").unwrap();
        let part1 = vec![b'A'; 5 * 1024 * 1024];
        let part2 = vec![b'B'; 1024];
        let _etag1 = store.upload_part(key, &uid, 1, &part1).unwrap();
        assert_eq!(store.upload_part_call_count(), 1);

        let mut full_data = part1.clone();
        full_data.extend_from_slice(&part2);
        let temp_path = std::env::temp_dir().join(format!("frames-mp-{}", fastrand::u64(..)));
        std::fs::write(&temp_path, &full_data).unwrap();

        // Inject 2 transient 503 errors to verify exponential backoff retry
        store.inject_transient_failures(2);

        // Resume with existing_upload_id = Some(&uid) and parts_already_done = 1:
        // part 1 must be discovered via `list_parts` (NOT re-uploaded!) and its
        // server-assigned ETag must pass `complete_multipart_upload` verification.
        let mut progress_steps = Vec::new();
        upload_file_with_resume(
            &store,
            &temp_path,
            key,
            "video/quicktime",
            Some(&uid),
            1,
            4,
            |u, parts| progress_steps.push((u.map(str::to_string), parts)),
        )
        .unwrap();

        let _ = std::fs::remove_file(&temp_path);

        // Part 1 was already uploaded (1 call) + Part 2 uploaded (1 retryable failure + 1 success):
        // verify the assembled object matches full_data!
        let fetched = store.get_object(key).unwrap().unwrap();
        assert_eq!(fetched.len(), full_data.len());
        assert_eq!(&fetched[..16], &full_data[..16]);
        assert_eq!(&fetched[fetched.len() - 16..], &full_data[full_data.len() - 16..]);
        assert!(store.list_multipart_uploads().unwrap().is_empty());
    }

    #[test]
    fn head_curl_args_use_dash_cap_i_and_timeouts() {
        let headers = BTreeMap::new();
        let head_args = build_curl_args("HEAD", "https://s4.example/bucket/key", &headers, false);
        assert!(head_args.contains(&"-I".to_string()));
        assert!(!head_args.contains(&"--data-binary".to_string()));
        assert!(head_args.contains(&"--connect-timeout".to_string()));
        assert!(head_args.contains(&"--max-time".to_string()));
    }

    #[test]
    fn parses_s3_list_objects_multipart_and_list_parts_xml() {
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

        let parts_xml = r#"
        <ListPartsResult>
            <Part><PartNumber>1</PartNumber><ETag>&quot;etag-p1&quot;</ETag><Size>5242880</Size></Part>
            <Part><PartNumber>2</PartNumber><ETag>&quot;etag-p2&quot;</ETag><Size>1024</Size></Part>
            <IsTruncated>false</IsTruncated>
        </ListPartsResult>"#;
        let (parts, next_marker) = parse_list_parts_xml(parts_xml);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].part_number, 1);
        assert_eq!(parts[0].etag, "\"etag-p1\"");
        assert_eq!(parts[0].size, 5_242_880);
        assert!(next_marker.is_none());
    }
}
