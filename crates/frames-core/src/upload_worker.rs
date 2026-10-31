use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::s4::ObjectStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadProgressPayload {
    pub job_id: String,
    pub state: String,
}

pub type UploadProgressCallback = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Enqueues an asset for background upload.
pub fn enqueue_asset_upload(conn: &Connection, asset_id: &str) -> rusqlite::Result<String> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM upload_jobs WHERE asset_id = ?1 AND state IN ('queued', 'running', 'done')",
            params![asset_id],
            |r| r.get(0),
        )
        .optional()?;

    if let Some(id) = existing {
        return Ok(id);
    }

    let job_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO upload_jobs (id, asset_id, state, error, retry_count, updated_at)
         VALUES (?1, ?2, 'queued', NULL, 0, ?3)",
        params![job_id, asset_id, now],
    )?;
    Ok(job_id)
}

/// Resumes pending and running uploads by marking running jobs as queued at app start.
pub fn resume_pending_uploads(conn: &Connection) -> rusqlite::Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE upload_jobs SET state = 'queued', updated_at = ?1 WHERE state = 'running'",
        params![now],
    )
}

/// Returns the count of currently active (queued or running) upload jobs.
pub fn get_active_upload_count(conn: &Connection) -> rusqlite::Result<usize> {
    conn.query_row(
        "SELECT count(*) FROM upload_jobs WHERE state IN ('queued', 'running')",
        [],
        |r| r.get(0),
    )
}

/// Finds the next queued job that is not currently in flight.
pub fn query_next_upload_job(
    conn: &Connection,
    in_flight: &HashSet<String>,
) -> Option<(String, String, u32)> {
    let mut stmt = conn
        .prepare(
            "SELECT id, asset_id, retry_count
             FROM upload_jobs
             WHERE state = 'queued'
             ORDER BY updated_at ASC
             LIMIT 20",
        )
        .ok()?;

    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u32>(2)?,
            ))
        })
        .ok()?;

    for item in rows.flatten() {
        if !in_flight.contains(&item.0) {
            return Some(item);
        }
    }
    None
}

pub struct UploadWorker {
    running: Arc<AtomicBool>,
    in_flight: Arc<Mutex<HashSet<String>>>,
}

impl UploadWorker {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            in_flight: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Starts the background upload worker with a concurrency limit of 3.
    /// It does not share a queue or lock with thumbnail work or remote downloads.
    pub fn start_worker_pool(
        self: Arc<Self>,
        db_path: PathBuf,
        s4: Arc<dyn ObjectStore>,
        on_progress: Option<UploadProgressCallback>,
    ) {
        self.running.store(true, Ordering::SeqCst);
        let sem = Arc::new(tokio::sync::Semaphore::new(3));
        let in_flight = Arc::clone(&self.in_flight);
        let running = Arc::clone(&self.running);

        tokio::spawn(async move {
            // Resume running jobs at start
            if let Ok(conn) = Connection::open(&db_path) {
                let _ = resume_pending_uploads(&conn);
            }

            while running.load(Ordering::SeqCst) {
                let job_opt = {
                    if let Ok(conn) = Connection::open(&db_path) {
                        let flight = in_flight.lock().unwrap();
                        query_next_upload_job(&conn, &flight)
                    } else {
                        None
                    }
                };

                let (job_id, asset_id, retry_count) = match job_opt {
                    Some(j) => j,
                    None => {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        continue;
                    }
                };

                let permit = match sem.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => break,
                };

                in_flight.lock().unwrap().insert(job_id.clone());

                // Update job state to running and notify
                if let Ok(conn) = Connection::open(&db_path) {
                    let now = chrono::Utc::now().to_rfc3339();
                    let _ = conn.execute(
                        "UPDATE upload_jobs SET state = 'running', updated_at = ?1 WHERE id = ?2",
                        params![now, job_id],
                    );
                }
                if let Some(ref cb) = on_progress {
                    cb(&job_id, "running");
                }

                let s4_clone = Arc::clone(&s4);
                let db_path_clone = db_path.clone();
                let on_progress_clone = on_progress.clone();
                let in_flight_clone = Arc::clone(&in_flight);

                tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    let result = perform_upload(&db_path_clone, s4_clone.as_ref(), &asset_id);

                    if let Ok(conn) = Connection::open(&db_path_clone) {
                        let now = chrono::Utc::now().to_rfc3339();
                        match result {
                            Ok(remote_key) => {
                                let _ = conn.execute(
                                    "UPDATE assets SET remote_key = ?1 WHERE id = ?2",
                                    params![remote_key, asset_id],
                                );
                                let _ = conn.execute(
                                    "UPDATE upload_jobs SET state = 'done', updated_at = ?1 WHERE id = ?2",
                                    params![now, job_id],
                                );
                                in_flight_clone.lock().unwrap().remove(&job_id);
                                if let Some(ref cb) = on_progress_clone {
                                    cb(&job_id, "done");
                                }
                            }
                            Err(err_msg) => {
                                if retry_count < 3 {
                                    let next_retry = retry_count + 1;
                                    let delay_secs = match next_retry {
                                        1 => 2,
                                        2 => 10,
                                        _ => 60,
                                    };
                                    let flight_delayed = Arc::clone(&in_flight_clone);
                                    let db_path_delayed = db_path_clone.clone();
                                    let job_id_delayed = job_id.clone();
                                    let progress_delayed = on_progress_clone.clone();
                                    tokio::spawn(async move {
                                        tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                                        if let Ok(conn) = Connection::open(&db_path_delayed) {
                                            let now = chrono::Utc::now().to_rfc3339();
                                            let _ = conn.execute(
                                                "UPDATE upload_jobs SET state = 'queued', retry_count = ?1, error = ?2, updated_at = ?3 WHERE id = ?4",
                                                params![next_retry, err_msg, now, job_id_delayed],
                                            );
                                        }
                                        flight_delayed.lock().unwrap().remove(&job_id_delayed);
                                        if let Some(ref cb) = progress_delayed {
                                            cb(&job_id_delayed, "queued");
                                        }
                                    });
                                } else {
                                    let _ = conn.execute(
                                        "UPDATE upload_jobs SET state = 'failed', error = ?1, updated_at = ?2 WHERE id = ?3",
                                        params![err_msg, now, job_id],
                                    );
                                    in_flight_clone.lock().unwrap().remove(&job_id);
                                    if let Some(ref cb) = on_progress_clone {
                                        cb(&job_id, "failed");
                                    }
                                }
                            }
                        }
                    }
                });
            }
        });
    }
}

pub fn perform_upload(
    db_path: &Path,
    s4: &dyn ObjectStore,
    asset_id: &str,
) -> std::result::Result<String, String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let (local_path, album_id, relative_path, filename): (
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    ) = conn
        .query_row(
            "SELECT local_path, album_id, relative_path, filename FROM assets WHERE id = ?1",
            params![asset_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|e| format!("asset not found: {e}"))?;

    let local_path_str = local_path.ok_or_else(|| "asset has no local_path".to_string())?;
    let path = Path::new(&local_path_str);
    if !path.exists() {
        return Err(format!("local file does not exist: {local_path_str}"));
    }

    let remote_key = if let (Some(aid), Some(rel)) = (album_id.as_deref(), relative_path.as_deref()) {
        let album_name: String = conn
            .query_row(
                "SELECT name FROM albums WHERE id = ?1",
                params![aid],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| aid.to_string());
        format!("albums/{album_name}/{rel}")
    } else {
        format!("originals/{filename}")
    };

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let mime = match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "heic" => "image/heic",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    };

    s4.put_object_from_file(&remote_key, path, mime)
        .map_err(|e| e.to_string())?;

    Ok(remote_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s4::{InMemoryObjectStore, S4Config};
    use crate::store::Store;

    #[tokio::test]
    async fn upload_worker_lifecycle_success_and_resumption() {
        let dir = std::env::temp_dir().join(format!("frames-upload-test-{}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).unwrap();

        let db_path = dir.join("library.sqlite");
        let store = Store::open(&db_path).unwrap();
        Store::run_migrations(store.connection()).unwrap();

        store.connection().execute(
            "INSERT INTO albums (id, name, name_at, created_at) VALUES ('alb-1', 'Summer', 'now', 'now')",
            [],
        ).unwrap();

        let local_file = dir.join("photo.jpg");
        std::fs::write(&local_file, b"sample image bytes").unwrap();

        let asset_id = crate::scan_local::upsert_local_asset(
            store.connection(),
            "alb-1",
            "photo.jpg",
            local_file.to_str().unwrap(),
            "photo.jpg",
            18,
            chrono::Utc::now(),
            "test-device",
        ).unwrap();

        let job_id = enqueue_asset_upload(store.connection(), &asset_id).unwrap();
        assert_eq!(get_active_upload_count(store.connection()).unwrap(), 1);

        let s4 = Arc::new(InMemoryObjectStore::new(S4Config::default()));
        let worker = Arc::new(UploadWorker::new());
        let progress_events = Arc::new(Mutex::new(Vec::new()));
        let pe_clone = Arc::clone(&progress_events);

        let on_prog = Arc::new(move |jid: &str, st: &str| {
            pe_clone.lock().unwrap().push((jid.to_string(), st.to_string()));
        });

        worker.clone().start_worker_pool(db_path.clone(), s4.clone(), Some(on_prog));

        // Wait for completion
        let mut done = false;
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let conn = Connection::open(&db_path).unwrap();
            let state: String = conn
                .query_row(
                    "SELECT state FROM upload_jobs WHERE id = ?1",
                    params![job_id],
                    |r| r.get(0),
                )
                .unwrap();
            if state == "done" {
                done = true;
                break;
            }
        }
        assert!(done, "upload job should have completed");
        worker.stop();

        // Verify remote_key was set on asset
        let conn = Connection::open(&db_path).unwrap();
        let remote_key: Option<String> = conn
            .query_row(
                "SELECT remote_key FROM assets WHERE id = ?1",
                params![asset_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(remote_key.as_deref(), Some("albums/Summer/photo.jpg"));

        // Verify S4 contains the uploaded object
        let s4_bytes = s4.get_object("albums/Summer/photo.jpg").unwrap();
        assert_eq!(s4_bytes, Some(b"sample image bytes".to_vec()));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
