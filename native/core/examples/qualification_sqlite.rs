//! Full SQLite scale probe for 0.21 qualification. No time cutoff.
//! Usage: cargo run -p shadowcode-core --release --example qualification_sqlite -- 1000000
use serde_json::json;
use shadowcode_core::store::Store;
use std::{
    env, fs,
    time::{Duration, Instant},
};

fn rss_kb() -> u64 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("VmRSS:"))
                .and_then(|line| line.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

fn main() {
    let n: usize = env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000_000);
    let root = tempfile::tempdir().expect("tempdir");
    let db_path = root.path().join("db");
    let open = Instant::now();
    let store = Store::open(&db_path).unwrap();
    let open_ms = open.elapsed().as_millis();
    let session = store
        .create_session(root.path(), "mock", "qualification-scale")
        .unwrap();
    let sid = session["id"].as_str().unwrap().to_owned();
    let started = Instant::now();
    let mut peak = rss_kb();
    for i in 0..n {
        store
            .add_event(
                if i.is_multiple_of(7) {
                    "user.message"
                } else {
                    "model.delta"
                },
                &json!({"text": format!("row-{i}"), "i": i}),
                Some(&sid),
                None,
            )
            .unwrap();
        if i.is_multiple_of(50_000) {
            peak = peak.max(rss_kb());
            eprintln!(
                "inserted={i} elapsed_s={:.1} rss_kb={peak}",
                started.elapsed().as_secs_f64()
            );
        }
    }
    let insert = started.elapsed();
    peak = peak.max(rss_kb());
    let reopen = Instant::now();
    drop(store);
    let store = Store::open(&db_path).unwrap();
    let startup_ms = reopen.elapsed().as_millis();
    let t = Instant::now();
    let recent = store.recent_events(&sid, 20).unwrap();
    let recent_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let after = store.events_after(&sid, 0, None, 200).unwrap();
    let catch_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let page = store.history_page(&sid, i64::MAX).unwrap();
    let history_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let search = store.sessions_in("qualification", 50, None).unwrap();
    let search_ms = t.elapsed().as_millis();
    let stats = store.local_stats().unwrap();
    let bytes = fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
    let report = json!({
        "n": n,
        "open_empty_ms": open_ms,
        "insert_ms": insert.as_millis(),
        "insert_s": insert.as_secs_f64(),
        "insert_per_sec": n as f64 / insert.as_secs_f64().max(0.001),
        "startup_existing_ms": startup_ms,
        "recent_events_ms": recent_ms,
        "events_after_ms": catch_ms,
        "history_page_ms": history_ms,
        "session_search_ms": search_ms,
        "recent_len": recent.len(),
        "after_len": after.len(),
        "history_len": page["events"].as_array().map(|v| v.len()),
        "search_len": search.len(),
        "db_bytes": bytes,
        "peak_rss_kb": peak,
        "local_stats": stats,
        "duration": Duration::from_millis(0).as_secs_f64(),
        "cutoff_s": null,
        "reached_requested_n": true
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    assert_eq!(recent.len(), 20.min(n));
    assert_eq!(stats["events"].as_i64().unwrap(), n as i64);
    assert_eq!(stats["telemetry"], false);
}
