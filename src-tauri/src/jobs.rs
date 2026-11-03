//! Background jobs (imports, exports, supercuts, folder moves) with progress events for the UI.
//! Each running job keeps its latest state, so a window opened mid-job can show it, and
//! pausable jobs (imports) can be paused and resumed.

use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobUpdate {
    pub id: String,
    pub kind: String,
    /// What the job works on, within its kind (an import's source: "outplayed" | "library").
    pub tag: String,
    /// In the language that was active when the job started.
    pub title: String,
    /// running | done | error | cancelled
    pub state: String,
    pub progress: f64,
    pub message: Option<String>,
    /// Recording or project produced by the job.
    pub result_id: Option<String>,
    pub pausable: bool,
    pub paused: bool,
    /// Seconds since the job started.
    pub elapsed: f64,
}

struct Running {
    cancel: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    last: Arc<Mutex<JobUpdate>>,
}

pub struct JobCtx {
    app: AppHandle,
    last: Arc<Mutex<JobUpdate>>,
    pub cancel: Arc<AtomicBool>,
    pub paused: Arc<AtomicBool>,
    started: Instant,
    last_emit: Mutex<Instant>,
}

impl JobCtx {
    fn emit(&self, force: bool) {
        let mut last_emit = self.last_emit.lock();
        if !force && last_emit.elapsed().as_millis() < 200 {
            return;
        }
        *last_emit = Instant::now();
        let mut u = self.last.lock();
        u.paused = self.paused.load(Ordering::Relaxed);
        u.elapsed = self.started.elapsed().as_secs_f64();
        let _ = self.app.emit("job", u.clone());
    }

    pub fn progress(&self, p: f64) {
        self.last.lock().progress = p.clamp(0.0, 1.0);
        self.emit(p >= 1.0);
    }

    /// Progress plus a status line (shown under the job, and kept as its final message).
    pub fn update(&self, p: f64, msg: &str) {
        let changed = {
            let mut u = self.last.lock();
            let changed = u.message.as_deref() != Some(msg);
            u.message = Some(msg.to_string());
            u.progress = p.clamp(0.0, 1.0);
            changed
        };
        if changed {
            log::info!("{}: {msg} ({:.0}%)", self.last.lock().title, p * 100.0);
        }
        self.emit(changed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Block while the job is paused. Returns Err if it gets cancelled meanwhile.
    pub fn wait_if_paused(&self) -> anyhow::Result<()> {
        while self.is_paused() {
            if self.cancelled() {
                anyhow::bail!("cancelled");
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct Jobs {
    running: Mutex<HashMap<String, Running>>,
}

impl Jobs {
    pub fn spawn<F>(&self, app: &AppHandle, kind: &str, title: &str, work: F) -> String
    where
        F: FnOnce(&JobCtx) -> anyhow::Result<Option<String>> + Send + 'static,
    {
        self.spawn_with(app, kind, "", title, false, false, work)
    }

    /// `pausable` jobs get Pause/Resume in the UI; `start_paused` restores a paused job after a restart.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_with<F>(&self, app: &AppHandle, kind: &str, tag: &str, title: &str, pausable: bool, start_paused: bool, work: F) -> String
    where
        F: FnOnce(&JobCtx) -> anyhow::Result<Option<String>> + Send + 'static,
    {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(start_paused));
        let base = JobUpdate {
            id: id.clone(),
            kind: kind.into(),
            tag: tag.into(),
            title: title.into(),
            state: "running".into(),
            progress: 0.0,
            message: None,
            result_id: None,
            pausable,
            paused: start_paused,
            elapsed: 0.0,
        };
        let last = Arc::new(Mutex::new(base.clone()));
        self.running.lock().insert(id.clone(), Running { cancel: cancel.clone(), paused: paused.clone(), last: last.clone() });
        let ctx = JobCtx { app: app.clone(), last, cancel, paused, started: Instant::now(), last_emit: Mutex::new(Instant::now()) };
        let app = app.clone();
        let _ = app.emit("job", base);
        std::thread::spawn(move || {
            let result = work(&ctx);
            let mut update = ctx.last.lock().clone();
            update.paused = false;
            update.elapsed = ctx.started.elapsed().as_secs_f64();
            match result {
                Ok(result_id) => {
                    update.state = "done".into();
                    update.progress = 1.0;
                    update.result_id = result_id;
                }
                Err(_) if ctx.cancelled() => update.state = "cancelled".into(),
                Err(e) => {
                    log::error!("job {} failed: {e:#}", update.title);
                    update.state = "error".into();
                    update.message = Some(format!("{e:#}"));
                }
            }
            if let Some(core) = crate::core::get() {
                core.jobs.running.lock().remove(&update.id);
            }
            let _ = app.emit("job", update);
            let _ = app.emit("library-changed", ());
        });
        id
    }

    pub fn cancel(&self, id: &str) {
        if let Some(r) = self.running.lock().get(id) {
            r.cancel.store(true, Ordering::Relaxed);
            r.paused.store(false, Ordering::Relaxed);
        }
    }

    /// Pause or resume. Returns the job kind when the job exists and is pausable.
    pub fn set_paused(&self, app: &AppHandle, id: &str, paused: bool) -> Option<String> {
        let jobs = self.running.lock();
        let r = jobs.get(id)?;
        let mut u = r.last.lock();
        if !u.pausable {
            return None;
        }
        r.paused.store(paused, Ordering::Relaxed);
        u.paused = paused;
        let _ = app.emit("job", u.clone());
        Some(u.kind.clone())
    }

    /// Latest state of every running job.
    pub fn list(&self) -> Vec<JobUpdate> {
        self.running
            .lock()
            .values()
            .map(|r| {
                let mut u = r.last.lock().clone();
                u.paused = r.paused.load(Ordering::Relaxed);
                u
            })
            .collect()
    }
}
