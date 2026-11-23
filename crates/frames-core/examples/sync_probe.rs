//! Replays the bucket's change log into a new, in-memory library, as a new
//! computer's first sync does, and reports what comes out: each device's
//! batches, any that fail to apply (the real sync stops at the first), the
//! albums and how many photos. Read only: it lists and downloads, nothing more.
//! Usage: cargo run --example sync_probe -- <settings.json>
//! The secret key comes from the OS keychain, as in the apps.

use std::collections::BTreeMap;

use frames_core::keys::{parse_log_key, LOG_PREFIX, SNAPSHOT_PREFIX};
use frames_core::ops::Batch;
use frames_core::s4::{load_secret_from_os_keychain, ObjectStore, S4Client, S4Config};
use frames_core::store::Store;

fn main() {
    let path = std::env::args().nth(1).expect("usage: sync_probe <settings.json>");
    let settings: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).expect("read settings")).expect("parse settings");
    let text = |key: &str| settings[key].as_str().unwrap_or_default().to_string();
    let access_key_id = text("access_key_id");
    let secret = load_secret_from_os_keychain(&access_key_id)
        .expect("read the keychain")
        .expect("no secret in the keychain for this access key");
    let s4 = S4Client::new(S4Config {
        endpoint: text("endpoint"),
        region: text("region"),
        bucket: text("bucket"),
        access_key_id,
        secret_access_key: secret,
        ..S4Config::default()
    });

    let snapshots = s4.list_objects(SNAPSHOT_PREFIX).expect("list snapshots");
    println!("snapshots: {}", snapshots.len());
    for snapshot in &snapshots {
        println!("  {} ({} bytes)", snapshot.key, snapshot.size);
    }

    let mut logs: Vec<(String, u64, String)> = s4
        .list_objects(LOG_PREFIX)
        .expect("list the change log")
        .into_iter()
        .filter_map(|o| parse_log_key(&o.key).map(|(device, seq)| (device.to_string(), seq, o.key.clone())))
        .collect();
    logs.sort_by(|a, b| (a.0.as_str(), a.1).cmp(&(b.0.as_str(), b.1)));
    let mut devices: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    for (device, seq, _) in &logs {
        devices.entry(device).or_default().push(*seq);
    }
    for (device, seqs) in &devices {
        let gaps: Vec<u64> = (1..=*seqs.last().unwrap()).filter(|s| !seqs.contains(s)).collect();
        println!("device {device}: {} batches, seq {}..{}, missing {:?}", seqs.len(), seqs[0], seqs.last().unwrap(), gaps);
    }

    let mut store = Store::open_in_memory("probe-0000").expect("open a scratch library");
    let mut failures = 0;
    for (_, seq, key) in &logs {
        let bytes = match s4.get_object(key) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                println!("  {key}: listed but gone");
                continue;
            }
            Err(err) => {
                println!("  {key}: download failed: {err}");
                failures += 1;
                continue;
            }
        };
        let batch: Batch = match serde_json::from_slice(&bytes) {
            Ok(batch) => batch,
            Err(err) => {
                println!("  {key}: can't be read: {err}");
                failures += 1;
                continue;
            }
        };
        let kinds: Vec<String> = batch
            .ops
            .iter()
            .map(|op| {
                let value = serde_json::to_value(op).unwrap_or_default();
                let kind = value["op"].as_str().unwrap_or("?").to_string();
                match value.get("name") {
                    Some(name) => format!("{kind} {name}"),
                    None if kind.starts_with("album") => format!("{kind} {}", value["id"]),
                    None => kind,
                }
            })
            .collect();
        let albums: Vec<&String> = kinds.iter().filter(|k| k.starts_with("album")).collect();
        if batch.seq != *seq {
            println!("  {key}: says it is seq {} of {}", batch.seq, batch.device);
        }
        match store.apply_remote_batch(&batch) {
            Ok(applied) => {
                if !applied || !albums.is_empty() {
                    let note = if applied { "" } else { " NOT APPLIED (seq already seen)" };
                    println!("  {key} {}: {} ops{note}; album ops: {albums:?}", batch.at, kinds.len());
                }
            }
            Err(err) => {
                println!("  {key}: failed to apply: {err}; ops: {kinds:?}");
                failures += 1;
            }
        }
    }

    println!("batches that failed: {failures}");
    let all = store.count_assets(&frames_core::store::SearchFilter::default()).expect("count photos");
    println!("photos and videos: {all}");
    for album in store.list_albums().expect("list albums") {
        println!("album {} \"{}\": {} photos", album.id, album.name, album.asset_count);
    }
}
