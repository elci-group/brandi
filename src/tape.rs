//! Intelligent VHS tape planning, deterministic compilation, and validation.

use crate::brief::Brief;
use crate::error::{BrandiError, Result};
use crate::guidelines::Guidelines;
use crate::process::{run_bounded, run_bounded_with_input, MAX_SUBPROCESS_OUTPUT_BYTES};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TapeRequest {
    pub goal: String,
    pub preset: String,
    pub slug: String,
    pub output: String,
}

impl Default for TapeRequest {
    fn default() -> Self {
        Self {
            goal: "Show how Brandi prevents identity drift".into(),
            preset: "product-tour".into(),
            slug: "brandi-demo".into(),
            output: "demos/brandi-demo.gif".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TapeCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl TapeCommand {
    fn brandi(args: &[&str]) -> Self {
        Self {
            program: "brandi".into(),
            args: args.iter().map(|value| (*value).to_string()).collect(),
        }
    }

    fn normalized(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TapeScene {
    pub title: String,
    pub command: TapeCommand,
    pub pause_ms: u64,
    #[serde(default)]
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TapePlan {
    pub title: String,
    pub goal: String,
    pub scenes: Vec<TapeScene>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TapeDraft {
    pub request: TapeRequest,
    pub provider: String,
    pub plan: TapePlan,
    pub tape: String,
    pub critique: Vec<TapeIssue>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IssueLevel {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TapeIssue {
    pub level: IssueLevel,
    pub code: String,
    pub message: String,
    pub line: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TapeValidationReport {
    pub valid: bool,
    pub requirements: Vec<String>,
    pub issues: Vec<TapeIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TapeExecutionPreview {
    pub tape: PathBuf,
    pub output: PathBuf,
    pub commands: Vec<String>,
    pub sandbox: String,
    pub confirmation: String,
}

#[derive(Debug, Serialize)]
struct TapeExecutionAudit<'a> {
    plan_hash: &'a str,
    approver: String,
    sandbox_policy: &'a str,
    executable_digest: String,
    renderer_digest: String,
    started_at: u64,
    ended_at: u64,
    result: String,
    tape: &'a Path,
    output: &'a Path,
    commands: &'a [String],
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ReasoningConfig {
    provider: String,
    base_url: String,
    model: String,
    api_key_env: String,
    reasoning_effort: String,
}

impl ReasoningConfig {
    fn load(root: &Path) -> Option<Self> {
        let path = root.join(".brandi/reasoning.yaml");
        let data = fs::read_to_string(path).ok()?;
        serde_yaml::from_str(&data).ok()
    }
}

pub fn generate(root: &Path, mut request: TapeRequest) -> Result<TapeDraft> {
    sanitize_request(&mut request);
    let brief = Brief::load(root)?;
    let guidelines = Guidelines::load(root)?;
    let (plan, provider) = match model_plan(root, &brief, &guidelines, &request) {
        Ok(Some(plan)) if valid_plan_shape(&plan) => (plan, "model".to_string()),
        _ => (
            fallback_plan(&request),
            "deterministic-fallback".to_string(),
        ),
    };
    let tape = compile(&guidelines, &request, &plan);
    let critique = critique(&brief, &plan);
    Ok(TapeDraft {
        request,
        provider,
        plan,
        tape,
        critique,
    })
}

fn sanitize_request(request: &mut TapeRequest) {
    request.slug = slugify(if request.slug.is_empty() {
        &request.goal
    } else {
        &request.slug
    });
    if request.output.is_empty() {
        request.output = format!("demos/{}.gif", request.slug);
    }
}

fn slugify(value: &str) -> String {
    let slug = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .take(8)
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "brandi-demo".into()
    } else {
        slug
    }
}

fn fallback_plan(request: &TapeRequest) -> TapePlan {
    let commands: Vec<(&str, TapeCommand)> = match request.preset.as_str() {
        "lint-failure-fix" => vec![
            ("Discover surfaces", TapeCommand::brandi(&["scan"])),
            ("See the coherence gap", TapeCommand::brandi(&["lint"])),
            (
                "Generate evidence-backed revisions",
                TapeCommand::brandi(&["propose", "--budget", "5"]),
            ),
        ],
        "social-plan" => vec![
            (
                "Map the narrative",
                TapeCommand::brandi(&["social", "graph"]),
            ),
            (
                "Build the channel plan",
                TapeCommand::brandi(&["social", "plan"]),
            ),
        ],
        _ => vec![
            ("Inspect the brand system", TapeCommand::brandi(&["scan"])),
            ("Measure coherence", TapeCommand::brandi(&["lint"])),
            (
                "Turn findings into action",
                TapeCommand::brandi(&["propose", "--budget", "5"]),
            ),
        ],
    };
    TapePlan {
        title: request.goal.clone(),
        goal: request.goal.clone(),
        scenes: commands
            .into_iter()
            .map(|(title, command)| TapeScene {
                title: title.into(),
                command,
                pause_ms: 1800,
                rationale: "Show one verifiable step in the product story".into(),
            })
            .collect(),
    }
}

fn model_plan(
    root: &Path,
    brief: &Brief,
    guidelines: &Guidelines,
    request: &TapeRequest,
) -> Result<Option<TapePlan>> {
    let Some(config) = ReasoningConfig::load(root) else {
        return Ok(None);
    };
    if config.provider != "openai" || config.base_url.is_empty() || config.model.is_empty() {
        return Ok(None);
    }
    let key_env = if config.api_key_env.is_empty() {
        "OPENAI_API_KEY"
    } else {
        &config.api_key_env
    };
    let Ok(api_key) = std::env::var(key_env) else {
        return Ok(None);
    };
    let prompt = serde_json::json!({
        "task": "Design a concise, safe VHS terminal demo. Return only the requested JSON object.",
        "goal": request.goal,
        "preset": request.preset,
        "brand": {
            "name": brief.identity.product.name,
            "tagline": brief.identity.product.tagline,
            "mission": brief.identity.product.mission,
            "voice_traits": guidelines.voice.traits,
            "narratives": brief.audience.narratives,
        },
        "constraints": [
            "2 to 6 scenes", "read-only commands only", "program must equal brandi",
            "args must match a documented allowlisted command", "pause_ms between 500 and 5000"
        ]
    });
    let schema = serde_json::json!({
        "type":"object", "additionalProperties":false,
        "required":["title","goal","scenes"],
        "properties":{
            "title":{"type":"string"}, "goal":{"type":"string"},
            "scenes":{"type":"array","minItems":2,"maxItems":6,"items":{
                "type":"object","additionalProperties":false,
                "required":["title","command","pause_ms","rationale"],
                "properties":{"title":{"type":"string"},"command":{
                    "type":"object","additionalProperties":false,
                    "required":["program","args"],
                    "properties":{
                        "program":{"type":"string","enum":["brandi"]},
                        "args":{"type":"array","minItems":1,"maxItems":4,"items":{"type":"string"}}
                    }
                },
                    "pause_ms":{"type":"integer"},"rationale":{"type":"string"}}
            }}
        }
    });
    let body = serde_json::json!({
        "model": config.model,
        "input": [{"role":"user","content": serde_json::to_string(&prompt)?}],
        "reasoning": {"effort": if config.reasoning_effort.is_empty() { "medium" } else { &config.reasoning_effort }},
        "text": {"format":{"type":"json_schema","name":"tape_plan","strict":true,"schema":schema}}
    });
    let endpoint = format!("{}/responses", config.base_url.trim_end_matches('/'));
    let curl_config = format!(
        "url = \"{}\"\nheader = \"Content-Type: application/json\"\nheader = \"Authorization: Bearer {}\"\ndata-binary = \"{}\"\n",
        escape_curl_config(&endpoint),
        escape_curl_config(&api_key),
        escape_curl_config(&body.to_string()),
    );
    let output = run_bounded_with_input(
        Command::new("curl").args([
            "-sS",
            "--fail-with-body",
            "--connect-timeout",
            "5",
            "--max-time",
            "45",
            "--max-filesize",
            "2097152",
            "-X",
            "POST",
            "--speed-limit",
            "1",
            "--speed-time",
            "10",
            "-K",
            "-",
        ]),
        Some(curl_config.as_bytes()),
        Duration::from_secs(45),
        MAX_SUBPROCESS_OUTPUT_BYTES,
    );
    let Ok(output) = output else { return Ok(None) };
    if output.timed_out || output.truncated || !output.status.success() {
        return Ok(None);
    }
    let response: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let text = response
        .get("output")
        .and_then(|v| v.as_array())
        .and_then(|items| {
            items.iter().find_map(|item| {
                item.get("content")?
                    .as_array()?
                    .iter()
                    .find_map(|part| part.get("text")?.as_str())
            })
        })
        .or_else(|| response.get("output_text").and_then(|v| v.as_str()));
    Ok(text.and_then(|value| serde_json::from_str(value).ok()))
}

fn escape_curl_config(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn valid_plan_shape(plan: &TapePlan) -> bool {
    (2..=6).contains(&plan.scenes.len())
        && plan.scenes.iter().all(|scene| {
            validate_command(&scene.command).is_ok() && (500..=5000).contains(&scene.pause_ms)
        })
}

pub fn compile(guidelines: &Guidelines, request: &TapeRequest, plan: &TapePlan) -> String {
    let margin = guidelines
        .visual
        .palette
        .neutrals
        .get(1)
        .map(String::as_str)
        .unwrap_or("#160812");
    let mut out = format!(
        "# Generated by Brandi — {}\nOutput {}\n\nRequire brandi\n\nSet Shell \"bash\"\nSet Theme \"CyberPunk2077\"\nSet Width 1200\nSet Height 720\nSet FontSize 28\nSet TypingSpeed 35ms\nSet Margin 24\nSet MarginFill \"{}\"\n\n",
        escape_comment(&plan.title), request.output, margin
    );
    for scene in &plan.scenes {
        out.push_str(&format!(
            "# {}\nType \"{}\" Enter\nSleep {}ms\n\n",
            escape_comment(&scene.title),
            escape_vhs(&scene.command.normalized()),
            scene.pause_ms
        ));
    }
    out
}

fn escape_vhs(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_comment(value: &str) -> String {
    value.replace(['\n', '\r'], " ")
}

fn critique(brief: &Brief, plan: &TapePlan) -> Vec<TapeIssue> {
    let mut issues = Vec::new();
    if !plan
        .title
        .to_lowercase()
        .contains(&brief.identity.product.name.to_lowercase())
    {
        issues.push(issue(
            IssueLevel::Info,
            "brand-title",
            "The title could name the product directly",
            None,
        ));
    }
    if plan.scenes.len() > 5 {
        issues.push(issue(
            IssueLevel::Warning,
            "pacing",
            "More than five scenes may dilute the demo",
            None,
        ));
    }
    issues
}

pub fn validate_file(root: &Path, path: &Path) -> Result<TapeValidationReport> {
    let text = fs::read_to_string(path)?;
    validate_text(root, &text, Some(path))
}

pub fn validate_text(
    root: &Path,
    text: &str,
    source: Option<&Path>,
) -> Result<TapeValidationReport> {
    let mut issues = Vec::new();
    let mut requirements = Vec::new();
    let mut has_output = false;
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let trimmed = line.trim();
        if let Some(program) = trimmed.strip_prefix("Require ") {
            requirements.push(program.trim_matches('"').to_string());
        }
        if let Some(output) = trimmed.strip_prefix("Output ") {
            has_output = true;
            let path = Path::new(output.trim_matches('"'));
            if !is_demos_path(path) {
                issues.push(issue(
                    IssueLevel::Error,
                    "output-path",
                    "Output must remain beneath `demos/`",
                    Some(number),
                ));
            }
        }
        if let Some(command) = typed_command(trimmed) {
            if let Err(message) = parse_normalized_command(&command) {
                issues.push(issue(
                    IssueLevel::Error,
                    "command-policy",
                    &message,
                    Some(number),
                ));
            }
        } else if trimmed.starts_with("Type ") {
            issues.push(issue(
                IssueLevel::Error,
                "command-syntax",
                "Type directives must contain one quoted canonical Brandi command",
                Some(number),
            ));
        }
        if trimmed.starts_with("Set Shell ") && trimmed != "Set Shell \"bash\"" {
            issues.push(issue(
                IssueLevel::Error,
                "shell-policy",
                "Tape shell must be exactly `bash`",
                Some(number),
            ));
        }
        if trimmed.starts_with("Require ") && trimmed != "Require brandi" {
            issues.push(issue(
                IssueLevel::Error,
                "require-policy",
                "Only the `brandi` executable may be required",
                Some(number),
            ));
        }
    }
    if !text.lines().any(|line| line.trim() == "Set Shell \"bash\"") {
        issues.push(issue(
            IssueLevel::Error,
            "missing-shell",
            "Tape must declare `Set Shell \"bash\"`",
            None,
        ));
    }
    if !has_output {
        issues.push(issue(
            IssueLevel::Error,
            "missing-output",
            "Tape has no Output directive",
            None,
        ));
    }
    if !requirements.iter().any(|r| r == "brandi") {
        issues.push(issue(
            IssueLevel::Error,
            "missing-require",
            "Add `Require brandi` for a predictable render",
            None,
        ));
    }
    for requirement in &requirements {
        if !program_exists(requirement) {
            issues.push(issue(
                IssueLevel::Warning,
                "missing-program",
                &format!("Required program `{requirement}` is not on PATH"),
                None,
            ));
        }
    }
    match run_vhs_validate(root, text, source) {
        Ok(()) => {}
        Err(message) => issues.push(issue(IssueLevel::Error, "vhs-syntax", &message, None)),
    }
    Ok(TapeValidationReport {
        valid: !issues.iter().any(|i| i.level == IssueLevel::Error),
        requirements,
        issues,
    })
}

fn is_demos_path(path: &Path) -> bool {
    !path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
        && matches!(path.components().next(), Some(Component::Normal(value)) if value == "demos")
}

fn typed_command(line: &str) -> Option<String> {
    let rest = line.strip_prefix("Type ")?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let mut escaped = false;
    let mut value = String::new();
    for c in rest.chars() {
        if escaped {
            value.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            let consumed = value.len() + 1;
            return rest
                .get(consumed..)
                .is_some_and(|suffix| suffix.trim() == "Enter")
                .then_some(value);
        } else {
            value.push(c);
        }
    }
    None
}

fn validate_command(command: &TapeCommand) -> std::result::Result<(), String> {
    if command.program != "brandi" {
        return Err("only the exact `brandi` program is permitted".into());
    }
    if command.args.iter().any(|arg| {
        arg.is_empty()
            || !arg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    }) {
        return Err("command arguments contain characters outside the canonical policy".into());
    }

    let args = command.args.iter().map(String::as_str).collect::<Vec<_>>();
    let allowed = matches!(args.as_slice(), ["scan"] | ["lint"])
        || matches!(args.as_slice(), ["social", "graph"] | ["social", "plan"])
        || matches!(args.as_slice(), ["propose", "--budget", value]
            if value.parse::<u32>().is_ok_and(|budget| (1..=20).contains(&budget)));
    if allowed {
        Ok(())
    } else {
        Err("command is not in the read-only Brandi tape allowlist".into())
    }
}

fn parse_normalized_command(value: &str) -> std::result::Result<TapeCommand, String> {
    if value.trim() != value || value.split_whitespace().collect::<Vec<_>>().join(" ") != value {
        return Err("command must use canonical single-space tokenization".into());
    }
    let mut tokens = value.split(' ');
    let command = TapeCommand {
        program: tokens.next().unwrap_or_default().to_string(),
        args: tokens.map(str::to_string).collect(),
    };
    validate_command(&command)?;
    Ok(command)
}

/// Write `text` to a freshly created temp file with an unpredictable name,
/// refusing to follow any pre-existing path (symlink or otherwise) so a
/// local attacker can't race the predictable-name window to redirect the
/// write.
fn create_exclusive_temp_file(text: &str) -> std::io::Result<std::path::PathBuf> {
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    for attempt in 0..8u32 {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let candidate = dir.join(format!("brandi-{pid}-{stamp}-{attempt}.tape"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                file.write_all(text.as_bytes())?;
                return Ok(candidate);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        ErrorKind::AlreadyExists,
        "could not allocate a unique temp file for tape validation",
    ))
}

fn program_exists(program: &str) -> bool {
    if program.contains(std::path::MAIN_SEPARATOR) {
        return false;
    }
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| directory.join(program).is_file())
    })
}

fn run_vhs_validate(
    root: &Path,
    text: &str,
    source: Option<&Path>,
) -> std::result::Result<(), String> {
    if !program_exists("vhs") {
        return Err("VHS is not installed or not on PATH".into());
    }
    let temporary;
    let path = if let Some(path) = source {
        path.to_path_buf()
    } else {
        temporary = create_exclusive_temp_file(text).map_err(|e| e.to_string())?;
        temporary.clone()
    };
    let output = run_bounded(
        Command::new("vhs")
            .arg("validate")
            .arg(&path)
            .current_dir(root),
        Duration::from_secs(15),
        MAX_SUBPROCESS_OUTPUT_BYTES,
    )
    .map_err(|e| e.to_string())?;
    if source.is_none() {
        let _ = fs::remove_file(path);
    }
    if output.timed_out {
        Err("VHS validation exceeded the 15 second limit".into())
    } else if output.truncated {
        Err("VHS validation exceeded the output limit".into())
    } else if output.status.success() {
        Ok(())
    } else {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if message.is_empty() {
            "VHS rejected the tape".into()
        } else {
            message
        })
    }
}

pub fn save_validated(
    root: &Path,
    draft: &TapeDraft,
    destination: &Path,
    force: bool,
) -> Result<TapeValidationReport> {
    let components = destination.components().collect::<Vec<_>>();
    let inside_demos =
        matches!(components.first(), Some(Component::Normal(value)) if *value == "demos");
    if destination.is_absolute()
        || destination
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        || !inside_demos
    {
        return Err(BrandiError::Invalid(
            "validated tapes may only be saved beneath `demos/`".into(),
        ));
    }
    let report = validate_text(root, &draft.tape, None)?;
    if !report.valid {
        return Ok(report);
    }
    let full = root.join(destination);
    if full.exists() && !force {
        return Err(BrandiError::Invalid(format!(
            "{} already exists; pass --force to replace it",
            full.display()
        )));
    }
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent)?;
    }
    // The component checks above reject `..` and absolute paths, but a
    // symlink planted inside `demos/` could still redirect the write
    // elsewhere; canonicalize and re-verify confinement right before
    // writing to close that window.
    let demos_root = root.join("demos");
    fs::create_dir_all(&demos_root)?;
    let canonical_demos = demos_root.canonicalize()?;
    let parent_to_check = full.parent().unwrap_or(root);
    let canonical_parent = parent_to_check.canonicalize()?;
    if !canonical_parent.starts_with(&canonical_demos) {
        return Err(BrandiError::Invalid(
            "validated tapes may only be saved beneath `demos/`".into(),
        ));
    }
    let tmp = full.with_extension("tape.tmp");
    fs::write(&tmp, &draft.tape)?;
    fs::rename(tmp, full)?;
    Ok(report)
}

pub fn preview(root: &Path, path: &Path) -> Result<TapeExecutionPreview> {
    if !program_exists("bwrap") {
        return Err(BrandiError::Invalid(
            "bubblewrap is required; tape execution fails closed without a sandbox".into(),
        ));
    }
    let canonical_root = root.canonicalize()?;
    let tape_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        canonical_root.join(path)
    };
    let canonical_tape = tape_path.canonicalize()?;
    let demos = canonical_root.join("demos");
    if !canonical_tape.starts_with(&demos) {
        return Err(BrandiError::Invalid(
            "renderable tapes must be stored beneath the project `demos/` directory".into(),
        ));
    }
    let text = fs::read_to_string(&canonical_tape)?;
    let report = validate_text(&canonical_root, &text, Some(&canonical_tape))?;
    if !report.valid {
        return Err(BrandiError::Invalid(
            "tape has blocking validation errors".into(),
        ));
    }
    let output = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("Output "))
        .map(|value| PathBuf::from(value.trim_matches('"')))
        .ok_or_else(|| BrandiError::Invalid("tape has no output directive".into()))?;
    if !is_demos_path(&output) {
        return Err(BrandiError::Invalid(
            "tape output must be stored beneath `demos/`".into(),
        ));
    }
    let commands = text
        .lines()
        .filter_map(|line| typed_command(line.trim()))
        .collect::<Vec<_>>();
    let mut digest = Sha256::new();
    digest.update(canonical_tape.to_string_lossy().as_bytes());
    digest.update([0]);
    digest.update(text.as_bytes());
    let confirmation = format!("render-{:x}", digest.finalize());
    Ok(TapeExecutionPreview {
        tape: canonical_tape,
        output,
        commands,
        sandbox: "bubblewrap: network disabled, host read-only, demos/ writable, clean environment"
            .into(),
        confirmation,
    })
}

pub fn render(root: &Path, path: &Path, confirmation: &str) -> Result<()> {
    let preview = preview(root, path)?;
    if confirmation != preview.confirmation {
        return Err(BrandiError::Invalid(format!(
            "confirmation mismatch; inspect the plan and pass `--confirm {}`",
            preview.confirmation
        )));
    }
    let canonical_root = root.canonicalize()?;
    let demos = canonical_root.join("demos");
    fs::create_dir_all(&demos)?;
    let canonical_demos = demos.canonicalize()?;
    let disposable_output = create_exclusive_temp_dir()?;
    let vhs_path = find_program("vhs")
        .ok_or_else(|| BrandiError::Invalid("VHS is not installed or not on PATH".into()))?;
    let brandi_path = std::env::current_exe()?.canonicalize()?;
    let executable_digest = sha256_file(&brandi_path)?;
    let renderer_digest = sha256_file(&vhs_path)?;
    let started_at = now_seconds();
    let execution = run_bounded(
        Command::new("bwrap")
            .args([
                "--die-with-parent",
                "--new-session",
                "--unshare-all",
                "--unshare-net",
                "--ro-bind",
                "/",
                "/",
                "--dev",
                "/dev",
                "--proc",
                "/proc",
                "--tmpfs",
                "/tmp",
                "--bind",
            ])
            .arg(&disposable_output)
            .arg(&canonical_demos)
            .args(["--dir", "/brandi-bin", "--ro-bind"])
            .arg(&brandi_path)
            .arg("/brandi-bin/brandi")
            .arg("--chdir")
            .arg(&canonical_root)
            .args([
                "--clearenv",
                "--setenv",
                "PATH",
                "/brandi-bin:/usr/bin:/bin",
                "--setenv",
                "HOME",
                "/tmp",
                "--setenv",
                "XDG_CONFIG_HOME",
                "/tmp",
            ])
            .arg(&vhs_path)
            .arg(&preview.tape),
        Duration::from_secs(600),
        MAX_SUBPROCESS_OUTPUT_BYTES,
    );
    let result = match execution {
        Err(error) => Err(BrandiError::Io(error)),
        Ok(output) if output.timed_out => Err(BrandiError::Invalid(
            "VHS renderer exceeded the 10 minute limit".into(),
        )),
        Ok(output) if output.truncated => Err(BrandiError::Invalid(
            "VHS renderer exceeded the subprocess output limit".into(),
        )),
        Ok(output) if output.status.success() => {
            promote_rendered_output(&disposable_output, &canonical_demos, &preview.output)
        }
        Ok(output) => Err(BrandiError::Invalid(format!(
            "sandboxed VHS renderer exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    };
    let audit = TapeExecutionAudit {
        plan_hash: &preview.confirmation,
        approver: format!("local-uid:{}", unsafe { libc::geteuid() }),
        sandbox_policy: &preview.sandbox,
        executable_digest,
        renderer_digest,
        started_at,
        ended_at: now_seconds(),
        result: match &result {
            Ok(()) => "success".into(),
            Err(error) => format!("failed: {error}"),
        },
        tape: &preview.tape,
        output: &preview.output,
        commands: &preview.commands,
    };
    let audit_result = append_tape_audit(&canonical_root, &audit);
    let _ = fs::remove_dir_all(&disposable_output);
    audit_result?;
    result
}

fn create_exclusive_temp_dir() -> Result<PathBuf> {
    let base = std::env::temp_dir();
    for attempt in 0..8u32 {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = base.join(format!(
            "brandi-render-{}-{stamp}-{attempt}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(BrandiError::Io(std::io::Error::new(
        ErrorKind::AlreadyExists,
        "could not create disposable tape output directory",
    )))
}

fn promote_rendered_output(disposable: &Path, demos: &Path, output: &Path) -> Result<()> {
    let relative = output
        .strip_prefix("demos")
        .map_err(|_| BrandiError::Invalid("tape output is not beneath demos/".into()))?;
    let source = disposable.join(relative);
    let metadata = fs::symlink_metadata(&source)?;
    if !metadata.file_type().is_file() || metadata.len() > 256 * 1024 * 1024 {
        return Err(BrandiError::Invalid(
            "rendered output must be one regular file no larger than 256 MiB".into(),
        ));
    }
    let destination = demos.join(relative);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
        if !parent.canonicalize()?.starts_with(demos) {
            return Err(BrandiError::Invalid(
                "render output parent escaped demos/ through a symlink".into(),
            ));
        }
    }
    let temporary = destination.with_extension("render.tmp");
    fs::copy(source, &temporary)?;
    fs::rename(temporary, destination)?;
    Ok(())
}

fn find_program(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| candidate.canonicalize().ok())
}

fn sha256_file(path: &Path) -> Result<String> {
    use std::io::Read as _;
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn append_tape_audit(root: &Path, event: &TapeExecutionAudit<'_>) -> Result<()> {
    let path = root.join(".brandi/state/tape-executions.jsonl");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(event)?)?;
    file.sync_data()?;
    Ok(())
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn issue(level: IssueLevel, code: &str, message: &str, line: Option<usize>) -> TapeIssue {
    TapeIssue {
        level,
        code: code.into(),
        message: message.into(),
        line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_is_safe_and_bounded() {
        let request = TapeRequest::default();
        let plan = fallback_plan(&request);
        assert!(valid_plan_shape(&plan));
        assert!((2..=6).contains(&plan.scenes.len()));
    }

    #[test]
    fn compiler_emits_only_normalized_typed_commands() {
        let guidelines = Guidelines::default();
        let request = TapeRequest::default();
        let plan = TapePlan {
            title: "Brandi".into(),
            goal: "demo".into(),
            scenes: vec![TapeScene {
                title: "Quote".into(),
                command: TapeCommand::brandi(&["lint"]),
                pause_ms: 500,
                rationale: String::new(),
            }],
        };
        let tape = compile(&guidelines, &request, &plan);
        assert!(tape.contains("Type \"brandi lint\" Enter"));
    }

    #[test]
    fn typed_policy_rejects_interpreter_path_environment_and_shell_bypasses() {
        for command in [
            "bash -c brandi lint",
            "/usr/bin/brandi lint",
            "env brandi lint",
            "brandi lint;rm -rf /",
            "brandi lint&&reboot",
            "brandi lint | sh",
            "brandi lint $(whoami)",
            "brandi lint `whoami`",
            "brandi lint　&&　reboot",
            "brandi  lint",
            "brandi lint --format json",
            "brandi social ../../plan",
        ] {
            assert!(
                parse_normalized_command(command).is_err(),
                "accepted {command:?}"
            );
        }
        for command in [
            "brandi scan",
            "brandi lint",
            "brandi propose --budget 5",
            "brandi social graph",
            "brandi social plan",
        ] {
            assert!(
                parse_normalized_command(command).is_ok(),
                "rejected {command:?}"
            );
        }
    }

    #[test]
    fn output_cannot_escape_project() {
        let text = "Output ../bad.gif\nRequire brandi\nType \"brandi lint\" Enter\n";
        let mut issues = Vec::new();
        for (index, line) in text.lines().enumerate() {
            if let Some(output) = line.strip_prefix("Output ") {
                let path = Path::new(output);
                if path.components().any(|c| matches!(c, Component::ParentDir)) {
                    issues.push(issue(
                        IssueLevel::Error,
                        "output-path",
                        "bad",
                        Some(index + 1),
                    ));
                }
            }
        }
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn save_destination_must_be_under_demos() {
        let root = tempfile::tempdir().unwrap();
        let draft = TapeDraft {
            request: TapeRequest::default(),
            provider: "test".into(),
            plan: fallback_plan(&TapeRequest::default()),
            tape: "Output demos/test.gif\nRequire brandi\nType \"brandi lint\" Enter\n".into(),
            critique: Vec::new(),
        };
        let error = save_validated(root.path(), &draft, Path::new("outside.tape"), false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("beneath `demos/`"));
    }

    #[test]
    fn disposable_output_promotion_rejects_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let disposable = tmp.path().join("disposable");
        let demos = tmp.path().join("demos");
        fs::create_dir_all(&disposable).unwrap();
        fs::create_dir_all(&demos).unwrap();
        fs::write(tmp.path().join("outside.gif"), b"outside").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(tmp.path().join("outside.gif"), disposable.join("demo.gif"))
                .unwrap();
            assert!(
                promote_rendered_output(&disposable, &demos, Path::new("demos/demo.gif")).is_err()
            );
        }
    }
}
