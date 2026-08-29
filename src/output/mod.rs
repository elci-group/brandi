//! Central output policy, semantic events, sanitization, and terminal lifecycle.

use crate::cli::{Format, OutputArgs};
use clap::ValueEnum;
use form3::compat::{Colorize, StyledText};
use form3::term::ColorSupport;
use serde::Serialize;
use std::io::{self, IsTerminal, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthChar;

#[derive(ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnimationChoice {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnicodeChoice {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[value(rename_all = "kebab-case")]
pub enum Fidelity {
    #[default]
    Auto,
    Static,
    Storyboard,
    Sketch,
    Fill,
    Print,
    Show,
    Movie,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Plain,
    Form3,
    Barbara,
}

#[derive(Clone, Debug)]
pub struct OutputContext {
    pub format: Format,
    pub color: bool,
    pub animation: bool,
    pub unicode: bool,
    pub width: Option<u16>,
    pub quiet: bool,
    pub verbose: bool,
    pub fidelity: Fidelity,
    pub backend: Backend,
    pub stdout_terminal: bool,
    pub stderr_terminal: bool,
    pub barbara_fallback: Option<&'static str>,
}

static CONTEXT: OnceLock<OutputContext> = OnceLock::new();
static BARBARA_AVAILABLE: OnceLock<bool> = OnceLock::new();
static STDOUT_EMITTED: AtomicBool = AtomicBool::new(false);

impl OutputContext {
    pub fn resolve(args: &OutputArgs) -> Self {
        Self::resolve_for(args, io::stdout().is_terminal(), io::stderr().is_terminal())
    }

    pub fn resolve_for(args: &OutputArgs, stdout_terminal: bool, stderr_terminal: bool) -> Self {
        let machine = matches!(args.format, Format::Json | Format::Jsonl);
        let disabled_by_env = std::env::var_os("NO_COLOR").is_some()
            || std::env::var("TERM").is_ok_and(|term| term == "dumb");
        let color = !machine
            && match args.color {
                ColorChoice::Always => true,
                ColorChoice::Never => false,
                ColorChoice::Auto => stdout_terminal && !disabled_by_env,
            };
        let unicode = !machine
            && match args.unicode {
                UnicodeChoice::Always => true,
                UnicodeChoice::Never => false,
                UnicodeChoice::Auto => stdout_terminal && !disabled_by_env,
            };
        let animation = !machine
            && match args.animation {
                AnimationChoice::Always => stderr_terminal,
                AnimationChoice::Never => false,
                AnimationChoice::Auto => {
                    stderr_terminal && std::env::var_os("CI").is_none() && !disabled_by_env
                }
            }
            && !matches!(args.fidelity, Fidelity::Static | Fidelity::Storyboard)
            && !args.quiet;

        let rich = matches!(
            args.fidelity,
            Fidelity::Print | Fidelity::Show | Fidelity::Movie
        );
        let barbara_available = rich && *BARBARA_AVAILABLE.get_or_init(probe_barbara);
        let backend = if machine || (!color && !animation) {
            Backend::Plain
        } else {
            // Barbara 0.1 has no live event-ingestion contract. Probe it so an
            // explicit rich request has deterministic fallback diagnostics.
            Backend::Form3
        };
        let barbara_fallback = rich.then_some(if barbara_available {
            "Barbara has no compatible live-event protocol; using 3form"
        } else {
            "Barbara is unavailable; using 3form"
        });

        Self {
            format: args.format,
            color,
            animation,
            unicode,
            width: args.width,
            quiet: args.quiet,
            verbose: args.verbose,
            fidelity: args.fidelity,
            backend,
            stdout_terminal,
            stderr_terminal,
            barbara_fallback,
        }
    }

    pub fn color_support(&self) -> ColorSupport {
        if self.color {
            ColorSupport::Standard16
        } else {
            ColorSupport::NoColor
        }
    }
}

pub fn install(context: OutputContext) {
    if context.verbose {
        if let Some(reason) = context.barbara_fallback {
            let _ = writeln!(io::stderr().lock(), "renderer: {reason}");
        }
    }
    let _ = CONTEXT.set(context);
}

pub fn context() -> &'static OutputContext {
    CONTEXT.get_or_init(|| OutputContext {
        format: Format::Human,
        color: false,
        animation: false,
        unicode: false,
        width: None,
        quiet: false,
        verbose: false,
        fidelity: Fidelity::Static,
        backend: Backend::Plain,
        stdout_terminal: false,
        stderr_terminal: false,
        barbara_fallback: None,
    })
}

fn probe_barbara() -> bool {
    let Ok(mut child) = Command::new("barbara")
        .arg("fidelities")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_millis(250);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// Write one durable stdout line. Rendering policy is resolved before command
/// execution; command modules never own stream handles directly.
pub fn line(value: impl std::fmt::Display) -> io::Result<()> {
    if context().quiet && context().format == Format::Human {
        return Ok(());
    }
    let mut value = value.to_string();
    if context().format == Format::Jsonl {
        let parsed: serde_json::Value = serde_json::from_str(&value).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("JSONL renderer received a non-JSON record: {error}"),
            )
        })?;
        let result = writeln!(io::stdout().lock(), "{parsed}");
        if result.is_ok() {
            STDOUT_EMITTED.store(true, Ordering::Release);
        }
        return result;
    }
    if context().format == Format::Human {
        if let Some(width) = context().width {
            value = wrap_visible(&value, usize::from(width));
        }
    }
    if context().format == Format::Human && context().color && !value.contains('\u{1b}') {
        let lower = value.to_ascii_lowercase();
        let severity = if lower.starts_with("warning") || lower.contains(" incomplete") {
            Severity::Warning
        } else if lower.starts_with("problem")
            || lower.starts_with("invalid")
            || lower.contains(" failed")
        {
            Severity::Error
        } else if lower.contains(" ok")
            || lower.starts_with("created")
            || lower.starts_with("rendered")
            || lower.starts_with("published")
            || lower.starts_with("staged")
            || lower.starts_with("synced")
        {
            Severity::Success
        } else {
            Severity::Info
        };
        let result = writeln!(io::stdout().lock(), "{}", styled(value, severity));
        if result.is_ok() {
            STDOUT_EMITTED.store(true, Ordering::Release);
        }
        result
    } else {
        let result = writeln!(io::stdout().lock(), "{value}");
        if result.is_ok() {
            STDOUT_EMITTED.store(true, Ordering::Release);
        }
        result
    }
}

fn wrap_visible(value: &str, width: usize) -> String {
    let mut out = String::with_capacity(value.len());
    let mut column = 0usize;
    let mut escape = 0u8;
    for ch in value.chars() {
        if escape == 1 {
            out.push(ch);
            escape = if ch == '[' { 2 } else { 0 };
            continue;
        }
        if escape == 2 {
            out.push(ch);
            if ('@'..='~').contains(&ch) {
                escape = 0;
            }
            continue;
        }
        if ch == '\u{1b}' {
            escape = 1;
            out.push(ch);
            continue;
        }
        if ch == '\n' {
            out.push(ch);
            column = 0;
            continue;
        }
        let char_width = ch.width().unwrap_or(0);
        if column > 0 && column.saturating_add(char_width) > width {
            out.push('\n');
            column = 0;
        }
        out.push(ch);
        column = column.saturating_add(char_width);
    }
    out
}

/// Write rendered stdout bytes without appending a newline.
pub fn write(value: impl std::fmt::Display) -> io::Result<()> {
    if context().quiet && context().format == Format::Human {
        return Ok(());
    }
    let result = write!(io::stdout().lock(), "{value}");
    if result.is_ok() {
        STDOUT_EMITTED.store(true, Ordering::Release);
    }
    result
}

/// Default cap for [`emit_list`] — long enough to be useful, short enough
/// that a large scan/queue doesn't degrade into a wall of text.
pub const DEFAULT_LIST_CAP: usize = 20;

/// Print `items` one per line via [`line`], capped at `cap` entries with a
/// `"… +{k} more"` footer when there are more. Under `--quiet`, prints
/// nothing at all — the caller's own summary line (via `line`) still gets
/// through, since `--quiet` means "only the final outcome," not silence.
pub fn emit_list<S: AsRef<str>>(items: &[S], cap: usize) -> io::Result<()> {
    if context().quiet {
        return Ok(());
    }
    let shown = items.len().min(cap);
    for item in &items[..shown] {
        line(item.as_ref())?;
    }
    if items.len() > shown {
        line(format!(
            "… +{} more (use --format json for the full list)",
            items.len() - shown
        ))?;
    }
    Ok(())
}

pub fn warning(value: impl std::fmt::Display) -> io::Result<()> {
    writeln!(
        io::stderr().lock(),
        "{}",
        styled(value.to_string(), Severity::Warning)
    )
}

/// Write and flush a durable interactive prompt on stderr.
pub fn prompt(value: impl std::fmt::Display) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    write!(stderr, "{}", styled(value.to_string(), Severity::Info))?;
    stderr.flush()
}

/// Emit a styled diagnostic on stderr only when `--verbose` is active.
pub fn verbose(value: impl std::fmt::Display) -> io::Result<()> {
    if context().verbose {
        writeln!(
            io::stderr().lock(),
            "{}",
            styled(value.to_string(), Severity::Info)
        )?;
    }
    Ok(())
}

/// Emit one durable pipeline milestone on stderr for human output. The
/// marker and colour are rendered by 3form, while machine output carries the
/// same progress through [`OutputEvent`] records and the final report.
pub fn milestone(
    current: u64,
    total: u64,
    label: impl std::fmt::Display,
    detail: Option<&str>,
    severity: Severity,
) -> io::Result<()> {
    if context().format != Format::Human || context().quiet {
        return Ok(());
    }
    let marker = if context().unicode {
        match severity {
            Severity::Success => "◆",
            Severity::Warning => "▲",
            Severity::Error => "✕",
            Severity::Info | Severity::Muted => "◇",
        }
    } else {
        match severity {
            Severity::Success => "+",
            Severity::Warning => "!",
            Severity::Error => "x",
            Severity::Info | Severity::Muted => ">",
        }
    };
    let mut message = format!("{marker} [{current}/{total}] {label}");
    if let Some(detail) = detail {
        message.push_str(" — ");
        message.push_str(detail);
    }
    writeln!(io::stderr().lock(), "{}", styled(message, severity))
}

pub fn event(event: &OutputEvent) -> io::Result<()> {
    if context().format == Format::Jsonl {
        line(
            serde_json::to_string(event)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        )
    } else {
        Ok(())
    }
}

pub fn emit_failure(error: &crate::error::BrandiError) -> io::Result<()> {
    let diagnostic = serde_json::json!({
        "schema_version": "brandi-error-v1",
        "status": "error",
        "code": error.code(),
        "message": error.to_string(),
        "action": error.action(),
        "docs": format!("https://docs.elci.group/brandi/errors/{}", error.code())
    });
    match context().format {
        Format::Human => Ok(()),
        Format::Json if STDOUT_EMITTED.load(Ordering::Acquire) => Ok(()),
        Format::Json => line(
            serde_json::to_string_pretty(&diagnostic)
                .map_err(|failure| io::Error::new(io::ErrorKind::InvalidData, failure))?,
        ),
        Format::Jsonl => line(diagnostic),
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Success,
    Warning,
    Error,
    Info,
    Muted,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutputEvent {
    CommandStarted {
        command: String,
        sequence: u64,
    },
    PhaseStarted {
        id: String,
        label: String,
        sequence: u64,
    },
    Progressed {
        phase: String,
        current: u64,
        total: Option<u64>,
        sequence: u64,
    },
    Diagnostic {
        severity: Severity,
        code: String,
        message: String,
        action: Option<String>,
        sequence: u64,
    },
    Preview {
        project: String,
        destination: String,
        consequence: String,
        confirmation: String,
        sequence: u64,
    },
    StateChanged {
        subject: String,
        from: String,
        to: String,
        sequence: u64,
    },
    CommandFinished {
        status: String,
        summary: String,
        sequence: u64,
    },
}

/// Neutralize control bytes that could alter terminal state. Newlines and tabs
/// remain because human results intentionally contain structured text.
pub fn sanitize_untrusted(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\n' | '\t' => ch,
            ch if ch == '\u{1b}' || ch.is_control() => '�',
            ch => ch,
        })
        .collect()
}

pub fn styled(value: impl Into<String>, severity: Severity) -> StyledText {
    let support = context().color_support();
    let text = sanitize_untrusted(&value.into());
    let styled = match severity {
        Severity::Success => text.green(),
        Severity::Warning => text.yellow(),
        Severity::Error => text.red(),
        Severity::Info => text.cyan(),
        Severity::Muted => text.dimmed(),
    };
    styled.with_color_support(support)
}

/// A delayed, bounded-rate stderr spinner. Dropping it clears the transient
/// region. Redirected and machine output never start the worker.
pub struct ProgressGuard {
    done: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ProgressGuard {
    pub fn start(label: impl Into<String>) -> Self {
        let enabled = context().animation && context().stderr_terminal;
        let label = sanitize_untrusted(&label.into());
        let done = Arc::new(AtomicBool::new(false));
        let worker = enabled.then(|| {
            let done = Arc::clone(&done);
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(200));
                if done.load(Ordering::Acquire) {
                    return;
                }
                let spinner = if context().unicode {
                    form3::anim::Spinner::default()
                } else {
                    form3::anim::Spinner::new(&form3::anim::ASCII)
                };
                let mut tick = 0;
                while !done.load(Ordering::Acquire) {
                    let _ = write!(io::stderr().lock(), "\r{} {label}", spinner.frame(tick));
                    let _ = io::stderr().flush();
                    tick += 1;
                    thread::sleep(Duration::from_millis(100));
                }
                let _ = write!(io::stderr().lock(), "\r\x1b[2K");
                let _ = io::stderr().flush();
            })
        });
        Self { done, worker }
    }

    pub fn finish(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for ProgressGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> OutputArgs {
        OutputArgs {
            format: Format::Human,
            color: ColorChoice::Auto,
            animation: AnimationChoice::Auto,
            fidelity: Fidelity::Auto,
            unicode: UnicodeChoice::Auto,
            width: None,
            quiet: false,
            verbose: false,
        }
    }

    #[test]
    fn machine_output_forces_static_plain_policy() {
        let mut args = args();
        args.format = Format::Json;
        args.color = ColorChoice::Always;
        args.animation = AnimationChoice::Always;
        args.unicode = UnicodeChoice::Always;
        let policy = OutputContext::resolve_for(&args, true, true);
        assert!(!policy.color);
        assert!(!policy.animation);
        assert!(!policy.unicode);
        assert_eq!(policy.backend, Backend::Plain);
    }

    #[test]
    fn redirected_auto_output_is_static_plain() {
        let policy = OutputContext::resolve_for(&args(), false, false);
        assert!(!policy.color);
        assert!(!policy.animation);
        assert!(!policy.unicode);
    }

    #[test]
    fn untrusted_control_bytes_are_neutralized() {
        assert_eq!(sanitize_untrusted("ok\u{1b}[2J\u{7}"), "ok�[2J�");
    }

    #[test]
    fn wrapping_counts_wide_unicode_and_ignores_ansi() {
        assert_eq!(wrap_visible("ab界c", 4), "ab界\nc");
        assert_eq!(
            wrap_visible("\u{1b}[31mabcd\u{1b}[0m", 3),
            "\u{1b}[31mabc\nd\u{1b}[0m"
        );
    }

    #[test]
    fn every_finite_presentation_policy_combination_preserves_invariants() {
        let formats = [Format::Human, Format::Json, Format::Jsonl];
        let booleans = [false, true];
        let colors = [ColorChoice::Auto, ColorChoice::Always, ColorChoice::Never];
        let animations = [
            AnimationChoice::Auto,
            AnimationChoice::Always,
            AnimationChoice::Never,
        ];
        let unicodes = [
            UnicodeChoice::Auto,
            UnicodeChoice::Always,
            UnicodeChoice::Never,
        ];
        let fidelities = [
            Fidelity::Auto,
            Fidelity::Static,
            Fidelity::Storyboard,
            Fidelity::Sketch,
            Fidelity::Fill,
            Fidelity::Print,
            Fidelity::Show,
            Fidelity::Movie,
        ];
        let mut checked = 0usize;
        for format in formats {
            for stdout_terminal in booleans {
                for stderr_terminal in booleans {
                    for color in colors {
                        for animation in animations {
                            for unicode in unicodes {
                                for fidelity in fidelities {
                                    for quiet in booleans {
                                        let args = OutputArgs {
                                            format,
                                            color,
                                            animation,
                                            fidelity,
                                            unicode,
                                            width: Some(80),
                                            quiet,
                                            verbose: false,
                                        };
                                        let policy = OutputContext::resolve_for(
                                            &args,
                                            stdout_terminal,
                                            stderr_terminal,
                                        );
                                        checked += 1;
                                        if matches!(format, Format::Json | Format::Jsonl) {
                                            assert!(!policy.color);
                                            assert!(!policy.animation);
                                            assert!(!policy.unicode);
                                            assert_eq!(policy.backend, Backend::Plain);
                                        }
                                        if quiet || animation == AnimationChoice::Never {
                                            assert!(!policy.animation);
                                        }
                                        if matches!(
                                            fidelity,
                                            Fidelity::Static | Fidelity::Storyboard
                                        ) {
                                            assert!(!policy.animation);
                                        }
                                        if color == ColorChoice::Never {
                                            assert!(!policy.color);
                                        }
                                        if unicode == UnicodeChoice::Never {
                                            assert!(!policy.unicode);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 5_184);
    }
}
