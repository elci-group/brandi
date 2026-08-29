//! Watch daemon: re-lints the project when surfaces change.
//!
//! The daemon keeps its state in `<root>/.brandi/state/`:
//! `daemon.pid` (pidfile), `daemon.log` (append-only log), `report.json`
//! (latest lint report), and `history.jsonl` (one JSON line per lint run).

use crate::brief::Brief;
use crate::error::{BrandiError, Result};
use crate::guidelines::Guidelines;
use crate::types::LintReport;
use crate::{report, rules, surface};
use notify::{RecursiveMode, Watcher};
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

/// Main-loop wake interval: how often we check for events and the stop flag.
const TICK: Duration = Duration::from_millis(250);
/// Quiet period after the last change before a re-lint is triggered.
const DEBOUNCE: Duration = Duration::from_millis(750);

/// Set by the signal handler when the daemon should shut down.
static STOP: AtomicBool = AtomicBool::new(false);

/// `<root>/.brandi/state` — where all daemon state lives.
pub fn state_dir(root: &Path) -> PathBuf {
    root.join(".brandi").join("state")
}

/// Path of the daemon pidfile.
pub fn pidfile_path(root: &Path) -> PathBuf {
    state_dir(root).join("daemon.pid")
}

/// Path of the daemon log file.
pub fn log_path(root: &Path) -> PathBuf {
    state_dir(root).join("daemon.log")
}

/// Path of the latest lint report.
pub fn report_path(root: &Path) -> PathBuf {
    state_dir(root).join("report.json")
}

/// Path of the lint-run history (JSON Lines).
pub fn history_path(root: &Path) -> PathBuf {
    state_dir(root).join("history.jsonl")
}

/// True when a process with `pid` exists (Linux `/proc`).
pub fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// True when `/proc/<pid>/cmdline` mentions "brandi" — our guard against
/// acting on a recycled pid that now belongs to a foreign process.
pub fn pid_is_brandi(pid: u32) -> bool {
    match std::fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).contains("brandi"),
        Err(_) => false,
    }
}

/// What the pidfile says about the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PidState {
    /// Pidfile points at a live brandi process.
    Running(u32),
    /// Pidfile exists but is stale: dead pid, foreign process, or garbage.
    Stale,
    /// No pidfile.
    Absent,
}

/// Inspect the pidfile for `project_root`.
pub fn check_pidfile(project_root: &Path) -> PidState {
    let pidfile = pidfile_path(project_root);
    if !pidfile.exists() {
        return PidState::Absent;
    }
    match read_pidfile(&pidfile) {
        Some(pid) if pid_alive(pid) && pid_is_brandi(pid) => PidState::Running(pid),
        _ => PidState::Stale,
    }
}

/// Parse a pidfile; unreadable or garbage contents yield `None`.
fn read_pidfile(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Atomically claim the pidfile for this process, closing the TOCTOU
/// window between `start()`'s pidfile check and the eventual write:
/// two `daemon start` calls racing each other could otherwise both see
/// `PidState::Absent` and both go on to write a pidfile with a plain
/// truncating write, silently overwriting one another. Uses an exclusive
/// create so only one instance can win; the loser sees `AlreadyExists`,
/// re-checks (a stale pidfile from a crashed process is removed and
/// retried; a genuinely live winner is reported as already running).
fn claim_pidfile(project_root: &Path) -> Result<()> {
    let pidfile = pidfile_path(project_root);
    for _ in 0..20 {
        match check_pidfile(project_root) {
            PidState::Running(pid) => {
                return Err(BrandiError::Daemon(format!(
                    "another daemon instance is already running (pid {pid})"
                )));
            }
            PidState::Stale => {
                let _ = std::fs::remove_file(&pidfile);
            }
            PidState::Absent => {}
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pidfile)
        {
            Ok(mut file) => {
                write!(file, "{}", std::process::id())?;
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
            Err(e) => return Err(BrandiError::Io(e)),
        }
    }
    Err(BrandiError::Daemon(
        "could not claim the daemon pidfile; another instance keeps winning the race".to_string(),
    ))
}

/// Run the daemon in the foreground, watching `project_root` for changes.
///
/// Loads the brief and guidelines, runs an initial lint (report + history + log), then
/// re-lints whenever files change, debounced. Shuts down cleanly on
/// SIGTERM/SIGINT.
pub fn run(project_root: &Path) -> Result<()> {
    // Brief/guidelines first: either missing is a hard error before any state exists.
    let brief = Brief::load(project_root)?;
    let guidelines = Guidelines::load(project_root)?;
    // Work on the canonical root: notify reports absolute event paths, so a
    // relative root like `.` would never match in `event_is_relevant`.
    let project_root = &project_root.canonicalize()?;

    // Lint before writing the pidfile, so `daemon start` only sees a pidfile
    // once the first report is on disk, and a failed lint leaves no stale pid.
    let report = run_lint(&brief, &guidelines, project_root)?;
    std::fs::create_dir_all(state_dir(project_root))?;
    write_report(project_root, &report)?;
    append_history(project_root, &report)?;
    log_line(
        project_root,
        &format!(
            "started (pid {}), initial score {} (errors {}, warnings {})",
            std::process::id(),
            report.scores.overall,
            report.scores.errors,
            report.scores.warnings
        ),
    )?;
    claim_pidfile(project_root)?;
    install_signal_handlers();
    let mut prev_overall = report.scores.overall;

    // Watch the whole tree recursively; events arrive as `notify::Result<Event>`.
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(w) => w,
        Err(e) => {
            let _ = std::fs::remove_file(pidfile_path(project_root));
            return Err(BrandiError::Daemon(format!(
                "failed to create file watcher: {e}"
            )));
        }
    };
    if let Err(e) = watcher.watch(project_root, RecursiveMode::Recursive) {
        let _ = std::fs::remove_file(pidfile_path(project_root));
        return Err(BrandiError::Daemon(format!(
            "failed to watch {}: {e}",
            project_root.display()
        )));
    }

    let mut last_event: Option<Instant> = None;
    loop {
        if STOP.load(Ordering::SeqCst) {
            log_line(project_root, "stopping")?;
            let _ = std::fs::remove_file(pidfile_path(project_root));
            return Ok(());
        }

        match rx.recv_timeout(TICK) {
            Ok(Ok(event)) => {
                if event_is_relevant(project_root, &event) {
                    last_event = Some(Instant::now());
                }
            }
            // A watcher error (e.g. event overflow) can mean missed changes:
            // schedule a re-lint to be safe.
            Ok(Err(_)) => last_event = Some(Instant::now()),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                let _ = std::fs::remove_file(pidfile_path(project_root));
                return Err(BrandiError::Daemon(
                    "watcher channel disconnected".to_string(),
                ));
            }
        }
        // Drain events that piled up during this tick.
        while let Ok(event) = rx.try_recv() {
            match event {
                Ok(event) => {
                    if event_is_relevant(project_root, &event) {
                        last_event = Some(Instant::now());
                    }
                }
                Err(_) => last_event = Some(Instant::now()),
            }
        }

        // Re-lint only after DEBOUNCE of quiet since the last event.
        let due = last_event.is_some_and(|t| t.elapsed() >= DEBOUNCE);
        if due {
            last_event = None;
            match relint_and_record(project_root, prev_overall) {
                Ok(new_overall) => prev_overall = new_overall,
                // A failed re-lint (e.g. a brief/guidelines file mid-edit) must not kill
                // the daemon; log it and keep watching.
                Err(e) => {
                    let _ = log_line(project_root, &format!("re-lint failed: {e}"));
                }
            }
        }
    }
}

/// Start the daemon in the background for `project_root`.
pub fn start(project_root: &Path) -> Result<()> {
    match check_pidfile(project_root) {
        PidState::Running(pid) => {
            return Err(BrandiError::Daemon(format!("already running (pid {pid})")));
        }
        // Stale pidfile (dead pid or foreign process): remove it silently.
        PidState::Stale => {
            let _ = std::fs::remove_file(pidfile_path(project_root));
        }
        PidState::Absent => {}
    }

    std::fs::create_dir_all(state_dir(project_root))?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path(project_root))?;
    let log_err = log.try_clone()?;

    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("daemon")
        .arg("run")
        .arg("--path")
        .arg(project_root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(log_err));
    // New process group so the daemon survives this CLI process (and is not
    // hit by signals sent to our group, e.g. Ctrl-C in the terminal).
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    // Detached on purpose: the child is never waited on.
    let _child = cmd.spawn()?;

    // The child writes its own pidfile once the first report is on disk;
    // give it up to 3s.
    for _ in 0..30 {
        if let Some(pid) = read_pidfile(&pidfile_path(project_root)) {
            match crate::output::context().format {
                crate::cli::Format::Human => {
                    crate::output::line(format_args!("brandi daemon started (pid {pid})"))?
                }
                crate::cli::Format::Json | crate::cli::Format::Jsonl => {
                    crate::output::line(serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": "brandi-daemon-state-v1",
                        "state": "running",
                        "pid": pid
                    }))?)?
                }
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(BrandiError::Daemon(format!(
        "daemon did not start (no pidfile after 3s); see {}",
        log_path(project_root).display()
    )))
}

/// Stop the background daemon for `project_root`.
pub fn stop(project_root: &Path) -> Result<()> {
    let pidfile = pidfile_path(project_root);
    if !pidfile.exists() {
        return Err(BrandiError::NotFound("daemon not running".to_string()));
    }
    // Garbage pidfile contents are stale by definition.
    let Some(pid) = read_pidfile(&pidfile) else {
        let _ = std::fs::remove_file(&pidfile);
        return Err(BrandiError::Daemon("stale pidfile removed".to_string()));
    };
    if !pid_is_brandi(pid) {
        let _ = std::fs::remove_file(&pidfile);
        return Err(BrandiError::Daemon("stale pidfile removed".to_string()));
    }
    // Safety: `libc::kill` only delivers a signal; `pid` comes from our own
    // pidfile and was just verified to belong to a brandi process.
    let rc = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    if rc != 0 {
        return Err(BrandiError::Daemon(format!("failed to signal pid {pid}")));
    }
    // Wait up to 5s for the process to exit.
    for _ in 0..25 {
        if !pid_alive(pid) {
            match crate::output::context().format {
                crate::cli::Format::Human => crate::output::line("brandi daemon stopped")?,
                crate::cli::Format::Json | crate::cli::Format::Jsonl => {
                    crate::output::line(serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": "brandi-daemon-state-v1",
                        "state": "stopped",
                        "pid": pid
                    }))?)?
                }
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(BrandiError::Daemon("failed to stop".to_string()))
}

/// Print the daemon status for `project_root`.
pub fn status(project_root: &Path) -> Result<()> {
    let (state, pid) = match check_pidfile(project_root) {
        PidState::Running(pid) => ("running", Some(pid)),
        PidState::Stale => {
            let _ = std::fs::remove_file(pidfile_path(project_root));
            ("not_running_stale_pidfile_removed", None)
        }
        PidState::Absent => ("not_running", None),
    };

    // A missing or unparseable report is not an error for a status command.
    let report = std::fs::read_to_string(report_path(project_root))
        .ok()
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok());
    let report_summary = report.as_ref().map(|value| {
        let ts = value
            .get("timestamp")
            .and_then(|t| t.as_str())
            .unwrap_or("unknown");
        let overall = number_at(value, "/scores/overall");
        let errors = number_at(value, "/scores/errors");
        let warnings = number_at(value, "/scores/warnings");
        let infos = number_at(value, "/scores/infos");
        (ts.to_string(), overall, errors, warnings, infos)
    });
    match crate::output::context().format {
        crate::cli::Format::Json | crate::cli::Format::Jsonl => {
            crate::output::line(serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "brandi-daemon-status-v1",
                "state": state,
                "pid": pid,
                "report": report_summary.as_ref().map(|(timestamp, overall, errors, warnings, infos)| serde_json::json!({
                    "timestamp": timestamp,
                    "overall": overall,
                    "errors": errors,
                    "warnings": warnings,
                    "infos": infos
                }))
            }))?)?
        }
        crate::cli::Format::Human => {
            match (state, pid) {
                ("running", Some(pid)) => crate::output::line(format_args!("running (pid {pid})"))?,
                ("not_running_stale_pidfile_removed", _) => {
                    crate::output::line("not running (removed stale pidfile)")?
                }
                _ => crate::output::line("not running")?,
            }
            if let Some((ts, overall, errors, warnings, infos)) = report_summary {
                crate::output::line(format_args!("last run: {ts}"))?;
                crate::output::line(format_args!("brand coherence: {overall}/100"))?;
                crate::output::line(format_args!(
                    "errors: {errors}  warnings: {warnings}  infos: {infos}"
                ))?;
            } else {
                crate::output::line("no report yet")?;
            }
        }
    }
    Ok(())
}

/// Read a numeric field out of the JSON report; missing fields read as 0.
fn number_at(value: &serde_json::Value, pointer: &str) -> u64 {
    value.pointer(pointer).and_then(|n| n.as_u64()).unwrap_or(0)
}

/// Run the full lint pipeline and build the report.
fn run_lint(brief: &Brief, guidelines: &Guidelines, root: &Path) -> Result<LintReport> {
    let scan = surface::scan_project(root)?;
    let findings = rules::run_rules_on_surfaces(brief, guidelines, root, &scan.surfaces);
    Ok(report::build_report_from_scan(root, findings, &scan, None))
}

/// Persist `report` as `report.json` under the state directory.
fn write_report(root: &Path, report: &LintReport) -> Result<()> {
    std::fs::write(report_path(root), report::render_json(report)?)?;
    Ok(())
}

/// Append one JSON line to `history.jsonl`.
/// Hard cap on `history.jsonl` line count. Without one, a daemon left
/// running for months on an actively-edited repo grows the file forever;
/// once the cap is crossed the oldest entries are trimmed back down to
/// `HISTORY_TRIM_TARGET` lines, keeping the most recent history.
const HISTORY_MAX_LINES: usize = 5_000;
const HISTORY_TRIM_TARGET: usize = 2_000;

fn append_history(root: &Path, report: &LintReport) -> Result<()> {
    let path = history_path(root);
    {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(file, "{}", history_line(report)?)?;
    }
    trim_history_if_needed(&path)
}

/// Trim `path` to the most recent `HISTORY_TRIM_TARGET` lines once it
/// exceeds `HISTORY_MAX_LINES`, via a write-to-temp-then-rename so a
/// concurrent reader never observes a partially-written file.
fn trim_history_if_needed(path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= HISTORY_MAX_LINES {
        return Ok(());
    }
    let mut trimmed: String = lines[lines.len() - HISTORY_TRIM_TARGET..].join("\n");
    trimmed.push('\n');
    let tmp = path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, trimmed)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// Append one timestamped line to `daemon.log`.
fn log_line(root: &Path, message: &str) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path(root))?;
    writeln!(file, "[{}] {message}", now_ts())?;
    Ok(())
}

/// Re-lint, persist report + history, and log the score change.
/// Returns the new overall score.
fn relint_and_record(root: &Path, prev_overall: u32) -> Result<u32> {
    // Reload the brief/guidelines on every re-lint rather than reusing the copy
    // loaded at daemon startup: `.brandi/*.yaml` edits are relevant watch
    // events (only `.brandi/state/` is excluded), so without this a user
    // fixing a rule mistake mid-session would keep getting scored against
    // the stale in-memory brief/guidelines until they restarted the daemon, with no
    // indication why the score didn't move. A brief/guidelines file that's mid-edit and
    // momentarily invalid just fails this re-lint like any other transient
    // error — the caller already logs it and retries on the next event.
    let brief = Brief::load(root)?;
    let guidelines = Guidelines::load(root)?;
    let report = run_lint(&brief, &guidelines, root)?;
    write_report(root, &report)?;
    append_history(root, &report)?;
    log_line(
        root,
        &format!(
            "score {} -> {} (errors {}, warnings {})",
            prev_overall, report.scores.overall, report.scores.errors, report.scores.warnings
        ),
    )?;
    Ok(report.scores.overall)
}

/// One `history.jsonl` entry; field order in the JSON follows declaration order.
#[derive(Serialize)]
struct HistoryEntry {
    ts: String,
    scoring_model: String,
    score: u32,
    errors: usize,
    warnings: usize,
    infos: usize,
}

/// Serialize the history line for one report.
fn history_line(report: &LintReport) -> Result<String> {
    let entry = HistoryEntry {
        ts: now_ts(),
        scoring_model: report.scores.model_version.clone(),
        score: report.scores.overall,
        errors: report.scores.errors,
        warnings: report.scores.warnings,
        infos: report.scores.infos,
    };
    Ok(serde_json::to_string(&entry)?)
}

/// Current UTC timestamp in RFC 3339, used for log and history entries.
fn now_ts() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// True when a file-event should trigger a re-lint: at least one affected
/// path is inside the project, not ignored, and outside the daemon's own
/// state directory.
fn event_is_relevant(root: &Path, event: &notify::Event) -> bool {
    event
        .paths
        .iter()
        .any(|path| match path.strip_prefix(root) {
            Ok(rel) => {
                !rel.as_os_str().is_empty()
                // `is_ignored` is expected to cover `.brandi/state`; we check
                // it explicitly as well — reacting to our own report writes
                // would retrigger the daemon forever.
                && !rel.starts_with(".brandi/state")
                && !surface::is_ignored(rel)
            }
            Err(_) => false,
        })
}

/// Signal handler for SIGTERM/SIGINT: raise the stop flag. Only an atomic
/// store, so it is async-signal-safe.
extern "C" fn on_stop_signal(_: i32) {
    STOP.store(true, Ordering::SeqCst);
}

/// Route SIGTERM and SIGINT to `on_stop_signal`.
fn install_signal_handlers() {
    // Coerce to a function pointer first: `libc::sighandler_t` is an integer
    // type, and casting a function item straight to an integer is linted.
    let handler: extern "C" fn(i32) = on_stop_signal;
    // Safety: `handler` is a valid `extern "C"` handler that performs only an
    // atomic store, which is safe in signal context; registering it with
    // `libc::signal` has no further preconditions.
    unsafe {
        libc::signal(libc::SIGTERM, handler as libc::sighandler_t);
        libc::signal(libc::SIGINT, handler as libc::sighandler_t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Scores;
    use std::collections::BTreeMap;

    /// A report with known numbers, built without touching other modules.
    fn sample_report() -> LintReport {
        LintReport {
            schema_version: crate::report::REPORT_SCHEMA_VERSION.into(),
            root: PathBuf::from("/tmp/example"),
            timestamp: "2026-07-17T00:00:00+00:00".to_string(),
            scores: Scores {
                model_version: crate::report::SCORING_MODEL_VERSION.into(),
                overall: 82,
                by_kind: BTreeMap::new(),
                errors: 2,
                warnings: 5,
                infos: 1,
                normalized_density_per_1000: 18,
                weighted_penalty_units: 18,
            },
            surface_counts: BTreeMap::new(),
            findings: Vec::new(),
            scan_complete: true,
            scan_diagnostics: Vec::new(),
            baseline_delta: None,
        }
    }

    #[test]
    fn state_paths_live_under_brandi_state() {
        let root = Path::new("/tmp/example");
        assert_eq!(state_dir(root), PathBuf::from("/tmp/example/.brandi/state"));
        assert_eq!(
            pidfile_path(root),
            PathBuf::from("/tmp/example/.brandi/state/daemon.pid")
        );
        assert_eq!(
            log_path(root),
            PathBuf::from("/tmp/example/.brandi/state/daemon.log")
        );
        assert_eq!(
            report_path(root),
            PathBuf::from("/tmp/example/.brandi/state/report.json")
        );
        assert_eq!(
            history_path(root),
            PathBuf::from("/tmp/example/.brandi/state/history.jsonl")
        );
    }

    #[test]
    fn pid_alive_detects_self_and_rejects_huge_pid() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(1 << 22));
    }

    #[test]
    fn check_pidfile_absent_without_pidfile() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(check_pidfile(tmp.path()), PidState::Absent);
    }

    #[test]
    fn check_pidfile_stale_for_dead_pid() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(state_dir(tmp.path())).unwrap();
        std::fs::write(pidfile_path(tmp.path()), (1u32 << 22).to_string()).unwrap();
        assert_eq!(check_pidfile(tmp.path()), PidState::Stale);
    }

    #[test]
    fn check_pidfile_stale_for_garbage_contents() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(state_dir(tmp.path())).unwrap();
        std::fs::write(pidfile_path(tmp.path()), "not-a-pid\n").unwrap();
        assert_eq!(check_pidfile(tmp.path()), PidState::Stale);
    }

    #[test]
    fn check_pidfile_running_for_own_pid() {
        // The test binary's path contains "brandi", so our own pid counts.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(state_dir(tmp.path())).unwrap();
        std::fs::write(pidfile_path(tmp.path()), std::process::id().to_string()).unwrap();
        assert_eq!(
            check_pidfile(tmp.path()),
            PidState::Running(std::process::id())
        );
    }

    #[test]
    fn history_line_identifies_the_scoring_model() {
        let line = history_line(&sample_report()).unwrap();
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        for key in [
            "ts",
            "scoring_model",
            "score",
            "errors",
            "warnings",
            "infos",
        ] {
            assert!(value.get(key).is_some(), "missing key '{key}' in {line}");
        }
        assert_eq!(value["score"], 82);
        assert_eq!(value["errors"], 2);
        assert_eq!(value["warnings"], 5);
        assert_eq!(value["infos"], 1);
        assert_eq!(value["scoring_model"], "scoring-v3-significance");
    }

    #[test]
    fn append_history_writes_one_json_line_per_run() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(state_dir(tmp.path())).unwrap();
        append_history(tmp.path(), &sample_report()).unwrap();
        append_history(tmp.path(), &sample_report()).unwrap();
        let content = std::fs::read_to_string(history_path(tmp.path())).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            for key in [
                "ts",
                "scoring_model",
                "score",
                "errors",
                "warnings",
                "infos",
            ] {
                assert!(value.get(key).is_some(), "missing key '{key}' in {line}");
            }
        }
    }

    #[test]
    fn history_is_trimmed_once_it_grows_too_large() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(state_dir(tmp.path())).unwrap();
        let path = history_path(tmp.path());
        // Pre-seed a file already over the cap, then trigger one more
        // append: it must trim back down instead of growing forever.
        let seeded = (0..HISTORY_MAX_LINES + 5)
            .map(|i| format!("{{\"n\":{i}}}"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&path, seeded).unwrap();
        append_history(tmp.path(), &sample_report()).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), HISTORY_TRIM_TARGET);
        // The most recent entries (including the just-appended one) survive.
        let last: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
        assert!(last.get("ts").is_some());
    }

    #[test]
    fn claim_pidfile_rejects_a_second_claim_while_alive() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(state_dir(tmp.path())).unwrap();
        claim_pidfile(tmp.path()).unwrap();
        assert_eq!(
            check_pidfile(tmp.path()),
            PidState::Running(std::process::id())
        );
        let err = claim_pidfile(tmp.path()).unwrap_err().to_string();
        assert!(err.contains("already running"), "got: {err}");
    }

    #[test]
    fn claim_pidfile_recovers_a_stale_pidfile() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(state_dir(tmp.path())).unwrap();
        // A pid nothing alive is using: stale by definition.
        std::fs::write(pidfile_path(tmp.path()), (1u32 << 22).to_string()).unwrap();
        claim_pidfile(tmp.path()).unwrap();
        assert_eq!(
            check_pidfile(tmp.path()),
            PidState::Running(std::process::id())
        );
    }

    #[test]
    fn log_line_appends_timestamped_message() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(state_dir(tmp.path())).unwrap();
        log_line(tmp.path(), "hello world").unwrap();
        log_line(tmp.path(), "second line").unwrap();
        let content = std::fs::read_to_string(log_path(tmp.path())).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with('['), "no timestamp: {}", lines[0]);
        assert!(lines[0].ends_with("] hello world"), "got: {}", lines[0]);
        assert!(lines[1].ends_with("] second line"), "got: {}", lines[1]);
    }

    #[test]
    fn state_dir_events_are_not_relevant() {
        let tmp = tempfile::tempdir().unwrap();
        let event = notify::Event::new(notify::EventKind::Any)
            .add_path(tmp.path().join(".brandi/state/report.json"));
        assert!(!event_is_relevant(tmp.path(), &event));
    }

    #[test]
    fn file_events_inside_root_are_relevant() {
        let tmp = tempfile::tempdir().unwrap();
        let event = notify::Event::new(notify::EventKind::Any).add_path(tmp.path().join("main.rs"));
        assert!(event_is_relevant(tmp.path(), &event));
    }

    #[test]
    fn events_outside_root_are_not_relevant() {
        let tmp = tempfile::tempdir().unwrap();
        let event =
            notify::Event::new(notify::EventKind::Any).add_path(PathBuf::from("/etc/hostname"));
        assert!(!event_is_relevant(tmp.path(), &event));
    }
}
