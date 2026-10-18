use super::{
    process::{Child, Jobs},
    util,
};
use crate::core::{
    policy::{Health, PENDING_MS, QUERY_MS},
    protocol::{Query, Selection, Source, Status, VERSION},
};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub enum Outcome {
    Selected(u64, Selection),
    Empty(u64, Status),
    Fault(u64, &'static str),
    Paused,
    Copied(u64, u32),
}
struct Pending {
    query: Query,
    at: Instant,
    copy: Option<String>,
}
struct Worker {
    child: Child,
    busy: Option<Instant>,
    last_seen: Instant,
    terminating: bool,
    handshaken: bool,
}
struct Starting {
    query: Query,
    at: Instant,
    job: Arc<Jobs>,
    cancel: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    result: mpsc::Receiver<Result<Child, String>>,
    terminating: bool,
}
enum Slot {
    Starting(Starting),
    Running(Worker),
    Gone,
}
impl Slot {
    fn query(&self) -> Option<&Query> {
        match self {
            Self::Starting(s) => Some(&s.query),
            Self::Running(w) => Some(&w.child.query),
            Self::Gone => None,
        }
    }
    fn terminating(&self) -> bool {
        match self {
            Self::Starting(s) => s.terminating,
            Self::Running(w) => w.terminating,
            Self::Gone => true,
        }
    }
    fn oldest(&self) -> Instant {
        match self {
            Self::Starting(s) => s.at,
            Self::Running(w) => w.child.start,
            Self::Gone => Instant::now(),
        }
    }
    fn kill(&mut self) {
        match self {
            Self::Starting(s) => {
                s.cancel.store(true, Ordering::Release);
                s.job.kill();
                s.terminating = true;
            }
            Self::Running(w) => {
                w.child.kill();
                w.terminating = true;
            }
            Self::Gone => {}
        }
    }
}
fn emit(result: &Arc<Mutex<VecDeque<Outcome>>>, outcome: Outcome) {
    let mut queue = result.lock().unwrap();
    if queue.len() >= 16 {
        queue.pop_front();
        super::diagnostics::event("outcome_overflow", serde_json::json!({"dropped":1}));
    }
    queue.push_back(outcome);
}

fn cancel_worker(
    w: &mut Worker,
    health: &mut Health,
    result: &Arc<Mutex<VecDeque<Outcome>>>,
    now: u64,
    code: &'static str,
) {
    if !w.terminating {
        if w.busy.is_some() {
            emit(result, Outcome::Fault(w.child.query.id, code));
        }
        if !w.child.query.clipboard {
            health.abandon(w.child.query.source, now);
        }
        w.child.kill();
        w.terminating = true;
    }
}
fn cancel_slot(
    slot: &mut Slot,
    health: &mut Health,
    result: &Arc<Mutex<VecDeque<Outcome>>>,
    now: u64,
    code: &'static str,
) {
    match slot {
        Slot::Running(w) => cancel_worker(w, health, result, now, code),
        Slot::Starting(s) if !s.terminating => {
            emit(result, Outcome::Fault(s.query.id, code));
            if !s.query.clipboard {
                health.abandon(s.query.source, now);
            }
            slot.kill();
        }
        _ => {}
    }
}
fn failure(health: &mut Health, query: &Query, handshaken: bool, now: u64, protocol_fault: bool) {
    super::diagnostics::event(
        "worker_fault",
        serde_json::json!({"id":query.id,"source_pid":query.source.pid,"clipboard":query.clipboard,"handshaken":handshaken,"protocol_fault":protocol_fault}),
    );
    // A clipboard renderer is not attributed to the foreground UIA provider.
    // All process creation attempts still consume the one shared rolling budget.
    if query.clipboard && handshaken {
        return;
    }
    if !query.clipboard && !handshaken {
        health.abandon(query.source, now);
    }
    health.failure(
        if handshaken { Some(query.source) } else { None },
        now,
        protocol_fault,
    );
}
fn stop_worker(
    w: &mut Worker,
    health: &mut Health,
    result: &Arc<Mutex<VecDeque<Outcome>>>,
    now: u64,
    code: &'static str,
    protocol_fault: bool,
) {
    if !w.terminating {
        failure(health, &w.child.query, w.handshaken, now, protocol_fault);
        emit(result, Outcome::Fault(w.child.query.id, code));
        w.child.kill();
        w.terminating = true;
    }
}
fn poll_slot(
    slot: &mut Slot,
    health: &mut Health,
    result: &Arc<Mutex<VecDeque<Outcome>>>,
    now: u64,
) {
    if let Slot::Starting(s) = slot {
        if !s.terminating && s.at.elapsed() >= Duration::from_millis(QUERY_MS) {
            failure(health, &s.query, false, now, false);
            emit(result, Outcome::Fault(s.query.id, "worker_start_timeout"));
            s.cancel.store(true, Ordering::Release);
            s.job.kill();
            s.terminating = true;
        }
        if s.thread.as_ref().is_some_and(|t| t.is_finished()) {
            let _ = s.thread.take().unwrap().join();
            let reply = s.result.try_recv();
            match reply {
                Ok(Ok(child)) => {
                    if s.terminating {
                        child.kill();
                    }
                    *slot = Slot::Running(Worker {
                        child,
                        busy: Some(s.at),
                        last_seen: Instant::now(),
                        terminating: s.terminating,
                        handshaken: false,
                    });
                }
                _ => {
                    if !s.terminating {
                        failure(health, &s.query, false, now, true);
                        emit(result, Outcome::Fault(s.query.id, "worker_start"));
                    }
                    *slot = Slot::Gone;
                }
            }
        }
    }
    if let Slot::Running(w) = slot {
        if !w.terminating
            && (w
                .busy
                .is_some_and(|at| at.elapsed() >= Duration::from_millis(QUERY_MS))
                || w.last_seen.elapsed() >= Duration::from_millis(QUERY_MS))
        {
            stop_worker(w, health, result, now, "worker_timeout", false);
        }
        // Drain while terminating too, so a bounded reader queue cannot keep the
        // reader thread alive and prevent confirmed reap of an already dead process.
        while let Ok(reply) = w.child.result.try_recv() {
            let reply = match reply {
                Ok(r) => r,
                Err(_) => {
                    stop_worker(w, health, result, now, "worker_protocol_or_exit", true);
                    break;
                }
            };
            if w.terminating {
                continue;
            }
            if reply.id < w.child.query.id && reply.nonce == w.child.query.nonce {
                continue;
            }
            let selected = reply.status == Status::Selected;
            if reply.version != VERSION
                || reply.build != env!("BEEK_BUILD_ID")
                || reply.nonce != w.child.query.nonce
                || reply.id != w.child.query.id
                || reply.source != w.child.query.source
                || (reply.status == Status::ClipboardWritten
                    && w.child.query.write_buffer.is_none())
                || selected != reply.selection.is_some()
                || (reply.status == Status::ClipboardWritten) != reply.clipboard_sequence.is_some()
                || reply.selection.as_ref().is_some_and(|s| {
                    !s.validate()
                        || s.source != reply.source
                        || s.epoch != w.child.query.epoch
                        || s.clipboard_sequence.is_some() != w.child.query.clipboard
                        || (w.child.query.clipboard
                            && !w.child.query.explicit_clipboard
                            && s.clipboard_sequence != w.child.query.clipboard_sequence)
                })
            {
                stop_worker(w, health, result, now, "worker_protocol", true);
                break;
            }
            if w.busy
                .is_some_and(|at| at.elapsed() >= Duration::from_millis(QUERY_MS))
            {
                stop_worker(w, health, result, now, "worker_timeout", false);
                continue;
            }
            w.last_seen = Instant::now();
            match reply.status {
                Status::Ready => w.handshaken = true,
                Status::Alive => {}
                Status::Changed => {
                    if util::is_current(reply.source) {
                        super::native::selection_changed();
                    }
                }
                status => {
                    if !w.handshaken || w.busy.is_none() {
                        stop_worker(w, health, result, now, "worker_protocol", true);
                        continue;
                    }
                    w.busy = None;
                    if status == Status::Failed {
                        w.child.kill();
                        w.terminating = true;
                    }
                    if !w.child.query.clipboard {
                        if matches!(
                            status,
                            Status::Selected
                                | Status::Empty
                                | Status::Unsupported
                                | Status::Protected
                                | Status::TooLong
                        ) {
                            health.success(reply.source);
                        } else {
                            health.abandon(reply.source, now);
                        }
                    }
                    super::diagnostics::event(
                        "selection_reply",
                        serde_json::json!({"id":reply.id,"source_pid":reply.source.pid,"status":format!("{status:?}"),"clipboard":w.child.query.clipboard}),
                    );
                    emit(
                        result,
                        if status == Status::ClipboardWritten {
                            Outcome::Copied(reply.id, reply.clipboard_sequence.unwrap())
                        } else if let Some(s) = reply.selection {
                            Outcome::Selected(reply.id, s)
                        } else {
                            Outcome::Empty(reply.id, status)
                        },
                    );
                }
            }
        }
        if !w.terminating && w.child.exited() {
            stop_worker(w, health, result, now, "worker_crash", true);
        }
        if !w.terminating && !util::identity_alive(w.child.query.source) {
            cancel_worker(w, health, result, now, "source_gone");
        }
        if w.terminating && w.child.reaped() {
            *slot = Slot::Gone;
        }
    }
}
fn launch(pending: Pending, main: Arc<Jobs>) -> Result<Slot, String> {
    let job = Jobs::new(1)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, result) = mpsc::sync_channel(1);
    let (query, at) = (pending.query.clone(), pending.at);
    let child_job = job.clone();
    let cancelled = cancel.clone();
    let thread = thread::spawn(move || {
        #[cfg(feature = "test-fixtures")]
        if std::env::var("BEEK_TEST_SPAWN_DELAY_MS").ok().as_deref() == Some("2200") {
            thread::sleep(Duration::from_millis(2200));
        }
        let child = Child::spawn(pending.query, pending.copy, main, child_job, cancelled);
        let _ = tx.send(child);
    });
    Ok(Slot::Starting(Starting {
        query,
        at,
        job,
        cancel,
        thread: Some(thread),
        result,
        terminating: false,
    }))
}
pub struct Coordinator {
    stats: Arc<Mutex<serde_json::Value>>,
    pending: Arc<Mutex<Option<Pending>>>,
    pub result: Arc<Mutex<VecDeque<Outcome>>>,
    stop: Arc<AtomicBool>,
    restore: Arc<AtomicBool>,
    nonce: String,
}
impl Coordinator {
    pub fn start() -> Result<Self, String> {
        let jobs = Jobs::new(3)?;
        let nonce = util::nonce()?;
        let pending = Arc::new(Mutex::new(None::<Pending>));
        let result = Arc::new(Mutex::new(VecDeque::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let restore = Arc::new(AtomicBool::new(false));
        let (p, r, s, reset) = (
            pending.clone(),
            result.clone(),
            stop.clone(),
            restore.clone(),
        );
        let stats = Arc::new(Mutex::new(serde_json::json!({})));
        let thread_stats = stats.clone();
        thread::spawn(move || {
            let clock = Instant::now();
            let mut health = Health::default();
            let mut slots = Vec::<Slot>::new();
            let mut waiting: Option<Pending> = None;
            let mut last_prune = Instant::now();
            let mut reported_pause = false;
            let mut system_epoch = super::native::SYSTEM_EPOCH.load(Ordering::Acquire);
            while !s.load(Ordering::Acquire) {
                let now = clock.elapsed().as_millis() as u64;
                let context = super::native::SYSTEM_EPOCH.load(Ordering::Acquire);
                if context != system_epoch {
                    system_epoch = context;
                    if let Some(old) = waiting.take() {
                        emit(&r, Outcome::Fault(old.query.id, "system_context_changed"));
                    }
                    if let Some(old) = p.lock().unwrap().take() {
                        emit(&r, Outcome::Fault(old.query.id, "system_context_changed"));
                    }
                    for slot in &mut slots {
                        cancel_slot(slot, &mut health, &r, now, "system_context_changed");
                    }
                }
                if reset.swap(false, Ordering::AcqRel) {
                    health.restore();
                }
                let newest = { p.lock().unwrap().take() };
                if let Some(new) = newest {
                    if let Some(old) = waiting.replace(new) {
                        emit(&r, Outcome::Fault(old.query.id, "superseded"));
                    }
                }
                for slot in &mut slots {
                    poll_slot(slot, &mut health, &r, now);
                }
                slots.retain(|s| !matches!(s, Slot::Gone));
                if last_prune.elapsed() >= Duration::from_secs(10) {
                    health.retain_live(|pid, created| util::process_alive(pid, created));
                    last_prune = Instant::now();
                }
                if health.paused() {
                    if !reported_pause {
                        emit(&r, Outcome::Paused);
                        reported_pause = true;
                    }
                    if waiting.as_ref().is_some_and(|p| !p.query.clipboard) {
                        emit(&r, Outcome::Paused);
                        waiting = None;
                    }
                    for slot in &mut slots {
                        if slot.query().is_some_and(|q| !q.clipboard) {
                            cancel_slot(slot, &mut health, &r, now, "platform_paused");
                        }
                    }
                } else {
                    reported_pause = false;
                }
                if let Some(q) = waiting.as_ref() {
                    if q.at.elapsed() >= Duration::from_millis(PENDING_MS) {
                        emit(&r, Outcome::Fault(q.query.id, "pending_expired"));
                        waiting = None;
                    } else if !(if q.query.explicit_clipboard {
                        util::identity_alive(q.query.source)
                    } else {
                        util::is_current(q.query.source)
                    }) || (!q.query.clipboard && !health.allowed(q.query.source, now))
                    {
                        emit(&r, Outcome::Empty(q.query.id, Status::SourceChanged));
                        waiting = None;
                    } else {
                        let clipboard = q.query.clipboard;
                        let reusable = slots.iter().position(|slot| match slot {
                            Slot::Running(w) => {
                                !w.terminating
                                    && w.busy.is_none()
                                    && w.child.query.clipboard == clipboard
                                    && (clipboard
                                        || (w.child.query.source.pid == q.query.source.pid
                                            && w.child.query.source.created
                                                == q.query.source.created))
                            }
                            _ => false,
                        });
                        if let Some(index) = reusable {
                            if !clipboard && !health.begin(q.query.source, now) {
                                waiting = None;
                                continue;
                            }
                            let q = waiting.take().unwrap();
                            if let Slot::Running(w) = &mut slots[index] {
                                if w.child.send(q.query, q.copy).is_ok() {
                                    w.busy = Some(q.at);
                                    w.last_seen = Instant::now();
                                } else {
                                    stop_worker(w, &mut health, &r, now, "worker_send", true);
                                }
                            }
                        } else if let Some(index) = slots.iter().position(|s| {
                            s.query().is_some_and(|old| {
                                old.clipboard == clipboard
                                    && old.source.pid == q.query.source.pid
                                    && old.source.created == q.query.source.created
                            })
                        }) {
                            let stale = slots[index].query().is_some_and(|old| {
                                if clipboard {
                                    old.clipboard_sequence != q.query.clipboard_sequence
                                } else {
                                    old.source != q.query.source || old.epoch != q.query.epoch
                                }
                            });
                            if stale && !slots[index].terminating() {
                                cancel_slot(
                                    &mut slots[index],
                                    &mut health,
                                    &r,
                                    now,
                                    "superseded_context",
                                );
                            }
                        } else {
                            let count = slots
                                .iter()
                                .filter(|s| s.query().is_some_and(|q| q.clipboard == clipboard))
                                .count();
                            if count < if clipboard { 1 } else { 2 } {
                                if health.attempt(now) {
                                    if !clipboard && !health.begin(q.query.source, now) {
                                        waiting = None;
                                        continue;
                                    }
                                    let q = waiting.take().unwrap();
                                    let id = q.query.id;
                                    let source = q.query.source;
                                    let is_clipboard = q.query.clipboard;
                                    match launch(q, jobs.clone()) {
                                        Ok(slot) => slots.push(slot),
                                        Err(_) => {
                                            if !is_clipboard {
                                                health.abandon(source, now);
                                            }
                                            health.failure(None, now, true);
                                            emit(&r, Outcome::Fault(id, "worker_start"));
                                        }
                                    }
                                }
                            } else {
                                if let Some(slot) = slots
                                    .iter_mut()
                                    .filter(|s| {
                                        s.query().is_some_and(|old| {
                                            old.clipboard == clipboard
                                                && (if clipboard {
                                                    old.clipboard_sequence
                                                        != q.query.clipboard_sequence
                                                } else {
                                                    old.source != q.query.source
                                                        || old.epoch != q.query.epoch
                                                        || !util::is_current(old.source)
                                                })
                                        }) && !s.terminating()
                                    })
                                    .min_by_key(|s| s.oldest())
                                {
                                    cancel_slot(slot, &mut health, &r, now, "preempted_context");
                                }
                            }
                        }
                    }
                }
                let (sources, quarantined, attempts) = health.summary(now);
                *thread_stats.lock().unwrap() = serde_json::json!({"uia_slots":slots.iter().filter(|s|s.query().is_some_and(|q|!q.clipboard)).count(),"clipboard_slots":slots.iter().filter(|s|s.query().is_some_and(|q|q.clipboard)).count(),"terminating":slots.iter().filter(|s|s.query().is_some()&&s.terminating()).count(),"attempts_last_10s":attempts,"fault_sources":sources,"quarantined_sources":quarantined,"paused":health.paused()});
                thread::sleep(Duration::from_millis(10));
            }
            for slot in &mut slots {
                slot.kill();
            }
        });
        Ok(Self {
            stats,
            pending,
            result,
            stop,
            restore,
            nonce,
        })
    }
    pub fn snapshot(&self) -> serde_json::Value {
        self.stats.lock().unwrap().clone()
    }
    pub fn request(&self, id: u64, source: Source, epoch: u64, point: [i32; 2], clipboard: bool) {
        self.enqueue(id, source, epoch, point, clipboard, false, None, None);
    }
    pub fn explicit_clipboard(&self, id: u64, source: Source, epoch: u64) {
        self.enqueue(id, source, epoch, [0, 0], true, true, None, None);
    }
    pub fn copy(&self, id: u64, source: Source, epoch: u64, text: String, sequence: u32) {
        self.enqueue(
            id,
            source,
            epoch,
            [0, 0],
            true,
            true,
            Some(text),
            Some(sequence),
        );
    }
    pub fn paste(&self, id: u64, source: Source, epoch: u64, sequence: u32) {
        self.enqueue(id, source, epoch, [0, 0], true, true, None, Some(sequence));
    }
    fn enqueue(
        &self,
        id: u64,
        source: Source,
        epoch: u64,
        point: [i32; 2],
        clipboard: bool,
        explicit_clipboard: bool,
        copy: Option<String>,
        sequence: Option<u32>,
    ) {
        let mouse = if clipboard {
            None
        } else {
            super::native::mouse_anchor(source)
        };
        let old = self.pending.lock().unwrap().replace(Pending {
            query: Query {
                version: VERSION,
                nonce: self.nonce.clone(),
                id,
                source,
                epoch,
                point: mouse.as_ref().map_or(point, |m| m.point),
                pointer_anchor: mouse.is_some(),
                pointer_end: mouse.and_then(|m| m.end),
                clipboard,
                explicit_clipboard,
                write_buffer: None,
                clipboard_sequence: if clipboard {
                    Some(sequence.unwrap_or_else(|| unsafe {
                        windows::Win32::System::DataExchange::GetClipboardSequenceNumber()
                    }))
                } else {
                    None
                },
            },
            at: Instant::now(),
            copy,
        });
        if let Some(old) = old {
            emit(&self.result, Outcome::Fault(old.query.id, "superseded"));
        }
    }
    pub fn restore(&self) {
        self.restore.store(true, Ordering::Release);
    }
}
impl Drop for Coordinator {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
