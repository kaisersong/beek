//! Structural diagnostics only: content and credentials never enter this queue.
use std::{
    fs::OpenOptions,
    io::Write,
    sync::{mpsc, OnceLock},
    time::Instant,
};
static LOG: OnceLock<mpsc::SyncSender<serde_json::Value>> = OnceLock::new();
static CLOCK: OnceLock<Instant> = OnceLock::new();
pub fn initialize() {
    let Ok(directory) = super::util::known_folder(true).map(|p| p.join("logs")) else {
        return;
    };
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    let path = directory.join("events.jsonl");
    if path.metadata().is_ok_and(|m| m.len() > 2 * 1024 * 1024) {
        let _ = std::fs::rename(&path, directory.join("previous.jsonl"));
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let (tx, rx) = mpsc::sync_channel::<serde_json::Value>(64);
    if LOG.set(tx).is_err() {
        return;
    }
    CLOCK.get_or_init(Instant::now);
    std::thread::spawn(move || {
        while let Ok(record) = rx.recv() {
            if file.metadata().is_ok_and(|m| m.len() > 2 * 1024 * 1024) {
                continue;
            }
            let _ = writeln!(file, "{record}");
            let _ = file.flush();
        }
    });
}
pub fn event(name: &'static str, data: serde_json::Value) {
    if let Some(log) = LOG.get() {
        let record = serde_json::json!({"schema":1,"ms":CLOCK.get_or_init(Instant::now).elapsed().as_millis(),"event":name,"data":data});
        let _ = log.try_send(record);
    }
}
