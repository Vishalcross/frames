use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::thumbs::{generate_image_thumb_with_conn, generate_video_thumb_with_conn, ThumbSize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThumbReadyPayload {
    pub asset_id: String,
}

#[derive(Debug, Clone)]
pub struct AssetThumbTask {
    pub id: String,
    pub kind: String,
    pub local_path: String,
}

pub type ThumbReadyCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// Returns the count of assets still pending thumbnail generation.
pub fn get_pending_thumb_count(conn: &Connection) -> rusqlite::Result<usize> {
    conn.query_row(
        "SELECT count(*) FROM assets WHERE local_path IS NOT NULL AND thumb_state IN ('none', 'failed')",
        [],
        |r| r.get(0),
    )
}

pub struct ThumbWorker {
    priority_ids: Arc<Mutex<VecDeque<String>>>,
    active_in_flight: Arc<Mutex<HashSet<String>>>,
    running: Arc<AtomicBool>,
}

impl ThumbWorker {
    pub fn new() -> Self {
        Self {
            priority_ids: Arc::new(Mutex::new(VecDeque::new())),
            active_in_flight: Arc::new(Mutex::new(HashSet::new())),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Exposes prioritize(asset_ids) so UI can bump what is on screen to visible-first.
    pub fn prioritize(&self, asset_ids: Vec<String>) {
        let mut q = self.priority_ids.lock().unwrap();
        // Add to front in reverse order so the first item in asset_ids is at the very front
        for id in asset_ids.into_iter().rev() {
            // Remove existing occurrence if any to prevent duplicates
            q.retain(|existing| existing != &id);
            q.push_front(id);
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Queries the next pending assets from the database.
    pub fn query_pending_assets(conn: &Connection) -> rusqlite::Result<Vec<AssetThumbTask>> {
        let mut stmt = conn.prepare(
            "SELECT a.id, a.kind, a.local_path
             FROM assets a
             LEFT JOIN albums al ON al.id = a.album_id
             WHERE a.local_path IS NOT NULL
               AND a.thumb_state IN ('none', 'failed')
             ORDER BY al.created_at DESC, a.taken_at DESC
             LIMIT 100",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(AssetThumbTask {
                id: r.get(0)?,
                kind: r.get(1)?,
                local_path: r.get(2)?,
            })
        })?;
        let mut tasks = Vec::new();
        for r in rows {
            if let Ok(t) = r {
                tasks.push(t);
            }
        }
        Ok(tasks)
    }

    /// Returns the count of assets still pending thumbnail generation.
    pub fn get_pending_thumb_count(conn: &Connection) -> rusqlite::Result<usize> {
        conn.query_row(
            "SELECT count(*) FROM assets WHERE local_path IS NOT NULL AND thumb_state IN ('none', 'failed')",
            [],
            |r| r.get(0),
        )
    }

    /// Starts the background thumbnail worker pool of 4 tasks.
    /// `db_path`: path to SQLite database so workers can open connections.
    pub fn start_worker_pool(
        self: Arc<Self>,
        db_path: PathBuf,
        on_thumb_ready: Option<ThumbReadyCallback>,
    ) {
        self.running.store(true, Ordering::SeqCst);
        let sem = Arc::new(tokio::sync::Semaphore::new(4));

        tokio::spawn(async move {
            while self.running.load(Ordering::SeqCst) {
                // Fetch next task: priority first, then db query
                let conn = match Connection::open(&db_path) {
                    Ok(c) => c,
                    Err(_) => {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        continue;
                    }
                };

                let mut next_task: Option<AssetThumbTask> = None;

                // 1. Check priority queue
                {
                    let mut pq = self.priority_ids.lock().unwrap();
                    let in_flight = self.active_in_flight.lock().unwrap();
                    while let Some(pid) = pq.pop_front() {
                        if !in_flight.contains(&pid) {
                            if crate::thumbs::thumb_exists(&pid, ThumbSize::Small) {
                                crate::thumbs::update_thumb_state(&conn, &pid, "ready");
                                continue;
                            }
                            // Look up asset in db
                            let row = conn.query_row(
                                "SELECT id, kind, local_path FROM assets WHERE id = ?1 AND local_path IS NOT NULL AND thumb_state IN ('none', 'failed')",
                                [&pid],
                                |r| {
                                    Ok(AssetThumbTask {
                                        id: r.get(0)?,
                                        kind: r.get(1)?,
                                        local_path: r.get(2)?,
                                    })
                                },
                            );
                            if let Ok(task) = row {
                                next_task = Some(task);
                                break;
                            }
                        }
                    }
                }

                // 2. Fall back to standard queue
                if next_task.is_none() {
                    let pending = Self::query_pending_assets(&conn).unwrap_or_default();
                    let in_flight = self.active_in_flight.lock().unwrap();
                    for t in pending {
                        if !in_flight.contains(&t.id) {
                            next_task = Some(t);
                            break;
                        }
                    }
                }

                let task = match next_task {
                    Some(t) => t,
                    None => {
                        // No tasks right now; wait before checking again
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        continue;
                    }
                };

                // Mark in-flight
                self.active_in_flight.lock().unwrap().insert(task.id.clone());

                let permit = match sem.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => break,
                };

                let worker_self = Arc::clone(&self);
                let db_path_clone = db_path.clone();
                let on_ready = on_thumb_ready.clone();

                tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    let conn = Connection::open(&db_path_clone).ok();
                    let src = Path::new(&task.local_path);

                    let res = if task.kind == "video" {
                        generate_video_thumb_with_conn(src, &task.id, ThumbSize::Small, conn.as_ref())
                    } else {
                        generate_image_thumb_with_conn(src, &task.id, ThumbSize::Small, conn.as_ref())
                    };

                    if res.is_ok() {
                        if let Some(cb) = &on_ready {
                            cb(&task.id);
                        }
                    }

                    worker_self.active_in_flight.lock().unwrap().remove(&task.id);
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[test]
    fn worker_prioritize_and_pending_queries() {
        let store = Store::open_in_memory("mac-1").unwrap();
        let conn = store.connection();

        conn.execute(
            "INSERT INTO assets (id, kind, filename, size, taken_at, date_at, state_at, local_path, thumb_state)
             VALUES ('a1', 'photo', 'f1.jpg', 10, 'now', 'now', 'now', '/path/1', 'none'),
                    ('a2', 'photo', 'f2.jpg', 10, 'now', 'now', 'now', '/path/2', 'failed'),
                    ('a3', 'photo', 'f3.jpg', 10, 'now', 'now', 'now', '/path/3', 'ready')",
            [],
        ).unwrap();

        let pending = ThumbWorker::query_pending_assets(conn).unwrap();
        assert_eq!(pending.len(), 2);
        let ids: Vec<String> = pending.into_iter().map(|p| p.id).collect();
        assert!(ids.contains(&"a1".to_string()));
        assert!(ids.contains(&"a2".to_string()));
        assert!(!ids.contains(&"a3".to_string()));

        let worker = ThumbWorker::new();
        worker.prioritize(vec!["a2".to_string(), "a1".to_string()]);
        let mut q = worker.priority_ids.lock().unwrap();
        assert_eq!(q.pop_front().unwrap(), "a2");
        assert_eq!(q.pop_front().unwrap(), "a1");
    }
}
