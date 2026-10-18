//! Internal fault/L1 runner. Compiled only with test-fixtures; never packaged.
use super::{
    native,
    selection::{Coordinator, Outcome},
    util,
};
use std::{
    sync::{atomic::Ordering, mpsc},
    time::{Duration, Instant},
};
fn wait(coordinator: &Coordinator, id: u64) -> (String, Option<String>, u128) {
    let at = Instant::now();
    loop {
        let result = {
            let mut queue = coordinator.result.lock().unwrap();
            let index = queue.iter().position(|r| match r {
                Outcome::Selected(i, _)
                | Outcome::Empty(i, _)
                | Outcome::Fault(i, _)
                | Outcome::Copied(i, _) => *i == id,
                _ => false,
            });
            index.and_then(|i| queue.remove(i))
        };
        if let Some(result) = result {
            match result {
                Outcome::Selected(_, s) => {
                    return ("Selected".into(), Some(s.text), at.elapsed().as_millis())
                }
                Outcome::Empty(_, s) => return (format!("{s:?}"), None, at.elapsed().as_millis()),
                Outcome::Fault(_, code) => return (code.into(), None, at.elapsed().as_millis()),
                Outcome::Copied(_, _) => return ("Copied".into(), None, at.elapsed().as_millis()),
                _ => {}
            }
        }
        if at.elapsed() > Duration::from_secs(4) {
            return ("HarnessTimeout".into(), None, at.elapsed().as_millis());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
pub fn run() {
    let args = std::env::args().collect::<Vec<_>>();
    let arg = |name: &str| {
        args.iter()
            .position(|v| v == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if args.iter().any(|v| v == "--help") {
        println!("BeeK internal Windows harness\n--suite l1|fault|network|cancel-probe|clipboard-isolation --source HWND --output FILE [--count 100] [--fixture textarea|paragraph] [--mode query-hang|callback-hang|crash|malformed|oversize|pre-ready-hang] [--base http://127.0.0.1:1441]\nRun in an ordinary interactive desktop; the matching helper must be beside this executable.");
        return;
    }
    let Some(output) = arg("--output") else {
        eprintln!("Missing --output; use --help");
        std::process::exit(2);
    };
    let suite = arg("--suite").unwrap_or("l1".into());
    let Some(hwnd) = arg("--source").and_then(|v| v.parse().ok()) else {
        eprintln!("Missing/invalid --source; use --help");
        std::process::exit(2);
    };
    let run = (|| -> Result<serde_json::Value, String> {
        super::diagnostics::initialize();
        super::pair::helper()?;
        if suite == "network" {
            return network(arg("--base").as_deref().unwrap_or("http://127.0.0.1:1441"));
        }
        let (tx, _rx) = mpsc::sync_channel(32);
        let _desktop = native::Desktop::start(tx)?;
        let coordinator = Coordinator::start()?;
        let epoch = native::EPOCH.load(Ordering::Acquire);
        let source = util::source(util::hwnd(hwnd), epoch).ok_or("source not eligible")?;
        let mut rows = vec![];
        let mut passed = true;
        if suite == "l1" {
            let expected = match arg("--fixture").as_deref() {
                Some("textarea") => "Hello intelligent agent. 中文划词测试，人工智能体。 A😀中B",
                Some("paragraph") => "The curious bee reads English and 中文 across two lines.",
                _ => "Hello intelligent agent. 中文 fixture.",
            };
            let count = arg("--count")
                .and_then(|v| v.parse().ok())
                .unwrap_or(100u64)
                .min(1000);
            for id in 1..=count {
                coordinator.request(id, source, epoch, util::cursor(), false);
                let (status, text, ms) = wait(&coordinator, id);
                let equal = text.as_deref() == Some(expected);
                passed &= equal;
                rows.push(serde_json::json!({"attempt":id,"status":status,"elapsed_ms":ms,"exact_fixture_match":equal}));
                std::thread::sleep(Duration::from_millis(25));
            }
        } else if suite == "clipboard-isolation" {
            std::env::set_var("BEEK_TEST_HELPER_MODE", "clipboard-hang");
            coordinator.explicit_clipboard(1, source, epoch);
            std::thread::sleep(Duration::from_millis(100));
            coordinator.request(2, source, epoch, util::cursor(), false);
            let (status, text, ms) = wait(&coordinator, 2);
            passed &= status == "Selected"
                && text.as_deref() == Some("Hello intelligent agent. 中文 fixture.")
                && ms < 800;
            rows.push(serde_json::json!({"attempt":2,"status":status,"elapsed_ms":ms,"slots":coordinator.snapshot()}));
            let (status, _, ms) = wait(&coordinator, 1);
            passed &= status == "worker_timeout" && ms < 1600;
            rows.push(serde_json::json!({"attempt":1,"status":status,"elapsed_ms":ms,"slots":coordinator.snapshot()}));
            std::env::remove_var("BEEK_TEST_HELPER_MODE");
        } else if suite == "cancel-probe" {
            std::env::set_var("BEEK_TEST_HELPER_MODE", "query-hang");
            for id in 1..=3 {
                if id == 2 {
                    std::thread::sleep(Duration::from_millis(1100));
                }
                if id == 3 {
                    std::thread::sleep(Duration::from_millis(5100));
                }
                coordinator.request(id, source, epoch, util::cursor(), false);
                let (status, _, ms) = wait(&coordinator, id);
                passed &= status == "worker_timeout";
                rows.push(serde_json::json!({"attempt":id,"status":status,"elapsed_ms":ms}));
            }
            std::thread::sleep(Duration::from_millis(30100));
            coordinator.request(4, source, epoch, util::cursor(), false);
            std::thread::sleep(Duration::from_millis(500));
            native::SYSTEM_EPOCH.fetch_add(1, Ordering::AcqRel);
            let (status, _, ms) = wait(&coordinator, 4);
            passed &= status == "system_context_changed";
            rows.push(serde_json::json!({"attempt":4,"status":status,"elapsed_ms":ms}));
            std::env::remove_var("BEEK_TEST_HELPER_MODE");
            std::thread::sleep(Duration::from_millis(1100));
            coordinator.request(5, source, epoch, util::cursor(), false);
            let (status, text, ms) = wait(&coordinator, 5);
            passed &= status == "Selected"
                && text.as_deref() == Some("Hello intelligent agent. 中文 fixture.");
            rows.push(serde_json::json!({"attempt":5,"status":status,"elapsed_ms":ms,"slots":coordinator.snapshot()}));
        } else if suite == "fault" {
            let mode = arg("--mode").unwrap_or("query-hang".into());
            std::env::set_var("BEEK_TEST_HELPER_MODE", &mode);
            let attempts = if mode == "pre-ready-hang" { 4 } else { 5 };
            for id in 1..=attempts {
                let _ = std::fs::write(format!("{output}.progress"), format!("attempt {id}"));
                if id == 2 {
                    std::thread::sleep(Duration::from_millis(1100));
                }
                if id == 3 {
                    std::thread::sleep(Duration::from_millis(5100));
                }
                if id == 4 && mode != "pre-ready-hang" {
                    std::thread::sleep(Duration::from_millis(30100));
                }
                coordinator.request(id, source, epoch, util::cursor(), false);
                let (status, _, ms) = wait(&coordinator, id);
                passed &= ms <= 1800 && status != "Selected" && status != "HarnessTimeout";
                if id == attempts {
                    passed &= ms < 350;
                }
                rows.push(serde_json::json!({"attempt":id,"status":status,"elapsed_ms":ms,"slots":coordinator.snapshot()}));
            }
            std::env::remove_var("BEEK_TEST_HELPER_MODE");
        } else {
            return Err("unknown suite".into());
        }
        let snapshot = coordinator.snapshot();
        drop(coordinator);
        std::thread::sleep(Duration::from_millis(200));
        Ok(
            serde_json::json!({"suite":suite,"source_pid":source.pid,"passed":passed,"rows":rows,"final":snapshot,"capabilities":super::capabilities("harness")}),
        )
    })();
    let value = match run {
        Ok(v) => v,
        Err(e) => serde_json::json!({"passed":false,"error":e}),
    };
    let passed = value["passed"] == true;
    std::fs::write(output, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    if !passed {
        std::process::exit(1);
    }
}

fn network(base: &str) -> Result<serde_json::Value, String> {
    use crate::core::stream::{Mailbox, Pool, Progress, Request};
    let pool = Pool::new();
    let mut rows = vec![];
    let mut passed = true;
    let request = |id, path: &str| Request {
        generation: id,
        text: "Hello public fixture.".into(),
        key: "public-fixture-key".into(),
        model: "public-model".into(),
        endpoint: format!("{base}/{path}"),
    };
    fn done(m: &Mailbox) -> Result<String, String> {
        let at = Instant::now();
        loop {
            if let Progress::Done(result) = &*m.lock().unwrap() {
                return result.clone();
            }
            if at.elapsed() > Duration::from_secs(62) {
                return Err("HarnessTimeout".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    for (id, path) in [
        "sse/normal",
        "sse/fragmented",
        "sse/disconnect",
        "sse/empty",
        "sse/oversized-frame",
        "sse/oversized-output",
        "http/401",
        "http/403",
        "http/429",
        "http/500",
        "nonstream",
    ]
    .iter()
    .enumerate()
    {
        let at = Instant::now();
        let result = done(&pool.submit(request(id as u64, path)));
        let expected = id < 2;
        let good = result.is_ok() == expected
            && result
                .as_ref()
                .map(|s| s == "公开测试译文：智能体。😀")
                .unwrap_or(true)
            && !result.as_ref().err().is_some_and(|e| {
                e.contains("BEEK_PUBLIC_ERROR_SENTINEL")
                    || e.contains("public-fixture-key")
                    || e == "HarnessTimeout"
            });
        passed &= good;
        rows.push(serde_json::json!({"fixture":path,"passed":good,"elapsed_ms":at.elapsed().as_millis(),"result_kind":if result.is_ok(){"success"}else{"error"}}));
    }
    let at = Instant::now();
    let a = pool.submit(request(101, "sse/slow"));
    std::thread::sleep(Duration::from_millis(100));
    let b = pool.submit(request(102, "sse/slow"));
    std::thread::sleep(Duration::from_millis(100));
    let mut replaced = vec![];
    for id in 103..113 {
        replaced.push(pool.submit(request(id, "sse/normal")));
    }
    pool.cancel_pending();
    let canceled = replaced
        .iter()
        .all(|m| matches!(&*m.lock().unwrap(), Progress::Done(Err(_))));
    let ra = done(&a);
    let rb = done(&b);
    let ms = at.elapsed().as_millis();
    let good = canceled && ra.is_err() && rb.is_err() && (59_500..=61_500).contains(&ms);
    passed &= good;
    rows.push(serde_json::json!({"fixture":"two-slow-total-deadline-and-latest-pending-cancel","passed":good,"elapsed_ms":ms,"canceled_pending":canceled}));
    Ok(
        serde_json::json!({"suite":"network","passed":passed,"rows":rows,"capabilities":super::capabilities("harness")}),
    )
}
