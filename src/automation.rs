//! Promotion planning, metric monitoring, Padagonia persistence, and Telegram supervision.

use crate::brief::Brief;
use crate::error::{BrandiError, Result};
use crate::guidelines::Guidelines;
use crate::process::{run_bounded, run_bounded_with_input, MAX_SUBPROCESS_OUTPUT_BYTES};
use crate::{report, rules, social, surface};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PromotionConfig {
    pub project_id: String,
    pub github_repository: String,
    pub github_subpath: String,
    pub padagonia_url: String,
    pub padagonia_key_env: String,
    pub horizon_days: u32,
    pub milestone_threshold: u32,
    pub telegram_channel_id: i64,
    pub telegram_token_env: String,
}

impl PromotionConfig {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(".brandi/promotion.yaml");
        let mut config: Self = if path.exists() {
            serde_yaml::from_str(&fs::read_to_string(path)?)?
        } else {
            Self::default()
        };
        if config.project_id.is_empty() {
            config.project_id = root
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("project")
                .to_string();
        }
        if config.padagonia_key_env.is_empty() {
            config.padagonia_key_env = "PADAGONIA_API_KEY".into();
        }
        if config.horizon_days == 0 {
            config.horizon_days = 30;
        }
        if config.milestone_threshold == 0 {
            config.milestone_threshold = 40;
        }
        if config.telegram_token_env.is_empty() {
            config.telegram_token_env = "TELEGRAM_BOT_TOKEN".into();
        }
        Ok(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricSnapshot {
    pub project_id: String,
    pub timestamp: u64,
    pub head: String,
    pub commits_30d: u64,
    pub contributors_30d: u64,
    pub tags: u64,
    pub github: BTreeMap<String, u64>,
    pub telegram: BTreeMap<String, u64>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MilestoneEvidence {
    pub id: String,
    pub title: String,
    pub release_success: bool,
    pub qualification_success: bool,
    pub semver_kind: String,
    pub novelty: f32,
    pub metric_delta: f32,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MilestoneScore {
    pub score: u32,
    pub qualifies: bool,
    pub breakdown: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromotionItem {
    pub channel: String,
    pub format: String,
    pub angle: String,
    pub success_metric: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromotionPlan {
    pub id: String,
    pub project_id: String,
    pub objective: String,
    pub audience: Vec<String>,
    pub narrative: Vec<String>,
    pub horizon_days: u32,
    pub items: Vec<PromotionItem>,
    pub stop_conditions: Vec<String>,
    pub evidence: Vec<String>,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DraftStatus {
    Pending,
    Approved,
    Rejected,
    Published,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentDraft {
    pub id: String,
    pub project_id: String,
    pub channel: String,
    pub content: String,
    pub evidence_ids: Vec<String>,
    pub status: DraftStatus,
    pub actor: String,
    pub updated_at: u64,
    pub reason: Option<String>,
    #[serde(default)]
    pub revision_hash: String,
    #[serde(default)]
    pub approved_destination: Option<String>,
    #[serde(default)]
    pub approval_expires_at: Option<u64>,
}

impl ContentDraft {
    pub fn expected_revision_hash(&self) -> String {
        revision_hash(self)
    }

    fn ensure_revision_hash(&mut self) {
        if self.revision_hash.is_empty() {
            self.revision_hash = revision_hash(self);
        }
    }

    pub fn approval_is_valid_for(&self, destination: &str) -> bool {
        self.status == DraftStatus::Approved
            && self.approved_destination.as_deref() == Some(destination)
            && self
                .approval_expires_at
                .is_some_and(|expires| expires >= now())
            && self.revision_hash == revision_hash(self)
    }
}

fn revision_hash(draft: &ContentDraft) -> String {
    let mut digest = Sha256::new();
    for value in [
        draft.id.as_str(),
        draft.project_id.as_str(),
        draft.channel.as_str(),
        draft.content.as_str(),
    ] {
        digest.update(value.as_bytes());
        digest.update([0]);
    }
    for evidence in &draft.evidence_ids {
        digest.update(evidence.as_bytes());
        digest.update([0]);
    }
    format!("sha256:{:x}", digest.finalize())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AuthorizationRole {
    Viewer,
    Planner,
    Approver,
    Publisher,
    DeviceOperator,
    Administrator,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct AuthorizationConfig {
    principals: Vec<LocalPrincipal>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct LocalPrincipal {
    uid: u32,
    roles: Vec<AuthorizationRole>,
}

#[derive(Debug, Clone)]
pub struct ActionIdentity {
    label: String,
}

impl ActionIdentity {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn telegram(id: i64) -> Self {
        Self {
            label: format!("telegram-user:{id}"),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_local(uid: u32) -> Self {
        Self {
            label: format!("local-uid:{uid}"),
        }
    }
}

pub fn authorize_local(root: &Path, role: AuthorizationRole) -> Result<ActionIdentity> {
    let uid = unsafe { libc::geteuid() };
    let path = root.join(".brandi/authorization.yaml");
    let outcome = (|| {
        let config: AuthorizationConfig = serde_yaml::from_str(&fs::read_to_string(&path)?)?;
        let principal = config
            .principals
            .iter()
            .find(|principal| principal.uid == uid)
            .ok_or_else(|| BrandiError::Invalid(format!("local uid {uid} is not authorized")))?;
        if principal.roles.contains(&AuthorizationRole::Administrator)
            || principal.roles.contains(&role)
        {
            Ok(ActionIdentity {
                label: format!("local-uid:{uid}"),
            })
        } else {
            Err(BrandiError::Invalid(format!(
                "local uid {uid} lacks the {role:?} role"
            )))
        }
    })();
    if let Err(error) = &outcome {
        let _ = append_security_audit(
            root,
            "authorization",
            &format!("local-uid:{uid}"),
            "local-role-check",
            None,
            "denied",
            Some(&error.to_string()),
        );
    }
    outcome
}

pub fn collect_metrics(root: &Path, config: &PromotionConfig) -> Result<MetricSnapshot> {
    let head = git(root, &["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let mut commit_args = vec!["rev-list", "--count", "--since=30 days ago", "HEAD"];
    if !config.github_subpath.is_empty() {
        commit_args.extend(["--", config.github_subpath.as_str()]);
    }
    let commits_30d = git(root, &commit_args)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut contributor_args = vec!["log", "--since=30 days ago", "--format=%aE", "HEAD"];
    if !config.github_subpath.is_empty() {
        contributor_args.extend(["--", config.github_subpath.as_str()]);
    }
    let contributors_30d = git(root, &contributor_args)
        .map(|value| value.lines().collect::<HashSet<_>>().len() as u64)
        .unwrap_or(0);
    let tags = git(root, &["tag", "--list"])
        .map(|v| v.lines().filter(|l| !l.is_empty()).count() as u64)
        .unwrap_or(0);
    let mut github = BTreeMap::new();
    let mut evidence = vec![format!("git:{head}")];
    if !config.github_repository.is_empty() {
        if let Some(repo) = github_repo(&config.github_repository) {
            for (key, field) in [
                ("stars", "stargazers_count"),
                ("forks", "forks_count"),
                ("open_issues", "open_issues_count"),
                ("watchers", "subscribers_count"),
            ] {
                if let Some(value) = repo.get(field).and_then(|v| v.as_u64()) {
                    github.insert(key.into(), value);
                }
            }
            evidence.push(format!("github:{}@{}", config.github_repository, now()));
        }
    }
    let mut telegram = BTreeMap::new();
    if config.telegram_channel_id != 0 {
        if let Ok(token) = std::env::var(&config.telegram_token_env) {
            if let Some(members) = telegram_member_count(&token, config.telegram_channel_id) {
                telegram.insert("members".into(), members);
                evidence.push(format!("telegram:{}@{}", config.telegram_channel_id, now()));
            }
        }
    }
    let snapshot = MetricSnapshot {
        project_id: config.project_id.clone(),
        timestamp: now(),
        head,
        commits_30d,
        contributors_30d,
        tags,
        github,
        telegram,
        evidence,
    };
    append_jsonl(&state_path(root, "metrics.jsonl"), &snapshot)?;
    queue_padagonia(
        root,
        config,
        "MetricSnapshot",
        &snapshot,
        &snapshot.evidence,
    )?;
    Ok(snapshot)
}

/// Escape a value for embedding in a curl `-K` config-file directive.
/// Curl's config parser treats a quoted value like a double-quoted shell
/// string: `\`, `"`, and a handful of control characters must be escaped.
fn escape_curl_config_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\x0b' => out.push_str("\\v"),
            other => out.push(other),
        }
    }
    out
}

fn curl_config_line(option: &str, value: &str) -> String {
    format!("{option} = \"{}\"\n", escape_curl_config_value(value))
}

/// Run `curl` with secret-bearing values (URLs with an embedded bot token,
/// auth headers, request bodies) supplied through a piped `-K` config file
/// instead of argv, so tokens never appear in `ps` or
/// `/proc/<pid>/cmdline` for the life of the child process. `flags` must
/// carry only non-secret argv switches.
fn curl_with_config(flags: &[&str], config: &str) -> Option<crate::process::BoundedOutput> {
    let mut last = None;
    for attempt in 0..3u64 {
        let output = run_bounded_with_input(
            Command::new("curl").args(flags).args([
                "--connect-timeout",
                "5",
                "--max-time",
                "12",
                "--speed-limit",
                "1",
                "--speed-time",
                "10",
                "--max-filesize",
                "2097152",
                "-K",
                "-",
            ]),
            Some(config.as_bytes()),
            Duration::from_secs(15),
            MAX_SUBPROCESS_OUTPUT_BYTES,
        )
        .ok()?;
        if output.timed_out || output.truncated {
            return None;
        }
        if output.status.success() {
            return Some(output);
        }
        last = Some(output);
        if attempt < 2 {
            let jitter_ms = now() % 97;
            thread::sleep(Duration::from_millis(200 * (1 << attempt) + jitter_ms));
        }
    }
    last
}

fn telegram_member_count(token: &str, chat: i64) -> Option<u64> {
    let body = serde_json::json!({"chat_id": chat});
    let mut curl_cfg = String::new();
    curl_cfg += &curl_config_line(
        "url",
        &format!("https://api.telegram.org/bot{token}/getChatMemberCount"),
    );
    curl_cfg += &curl_config_line("header", "Content-Type: application/json");
    curl_cfg += &curl_config_line("data-binary", &body.to_string());
    let output = curl_with_config(&["-sS", "--fail-with-body", "-X", "POST"], &curl_cfg)?;
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    response["ok"]
        .as_bool()
        .unwrap_or(false)
        .then(|| response["result"].as_u64())
        .flatten()
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = run_bounded(
        Command::new("git").args(args).current_dir(root),
        Duration::from_secs(15),
        MAX_SUBPROCESS_OUTPUT_BYTES,
    )
    .ok()?;
    if output.timed_out || output.truncated {
        return None;
    }
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn github_repo(repository: &str) -> Option<serde_json::Value> {
    let token = std::env::var("GITHUB_TOKEN").ok();
    let mut curl_cfg = String::new();
    curl_cfg += &curl_config_line("url", &format!("https://api.github.com/repos/{repository}"));
    curl_cfg += &curl_config_line("header", "Accept: application/vnd.github+json");
    if let Some(token) = token {
        curl_cfg += &curl_config_line("header", &format!("Authorization: Bearer {token}"));
    }
    let output = curl_with_config(&["-sS", "--fail-with-body"], &curl_cfg)?;
    output
        .status
        .success()
        .then(|| serde_json::from_slice(&output.stdout).ok())
        .flatten()
}

pub fn score_milestone(evidence: &MilestoneEvidence, threshold: u32) -> MilestoneScore {
    let mut breakdown = BTreeMap::new();
    breakdown.insert(
        "release".into(),
        if evidence.release_success { 40 } else { 0 },
    );
    breakdown.insert(
        "qualification".into(),
        if evidence.qualification_success {
            15
        } else {
            0
        },
    );
    breakdown.insert(
        "semver".into(),
        match evidence.semver_kind.as_str() {
            "major" => 10,
            "minor" => 8,
            "patch" => 3,
            _ => 0,
        },
    );
    breakdown.insert(
        "novelty".into(),
        (evidence.novelty.clamp(0.0, 1.0) * 25.0).round() as u32,
    );
    breakdown.insert(
        "metrics".into(),
        (evidence.metric_delta.clamp(0.0, 1.0) * 10.0).round() as u32,
    );
    let score = breakdown.values().sum();
    let anchored =
        evidence.release_success || (evidence.novelty >= 0.7 && evidence.evidence.len() >= 2);
    MilestoneScore {
        score,
        qualifies: anchored && score >= threshold,
        breakdown,
    }
}

pub fn build_plan(root: &Path) -> Result<PromotionPlan> {
    let config = PromotionConfig::load(root)?;
    let brief = Brief::load(root)?;
    let previous = latest_metric(root);
    let metrics = collect_metrics(root, &config)?;
    let focus = promotion_focus(previous.as_ref(), &metrics);
    let audience = brief
        .audience
        .segments
        .iter()
        .map(|s| s.name.clone())
        .collect::<Vec<_>>();
    let narrative = brief
        .audience
        .narratives
        .iter()
        .map(|n| n.narrative.clone())
        .collect::<Vec<_>>();
    let id = format!("plan-{}", now());
    let plan = PromotionPlan {
        id: id.clone(),
        project_id: config.project_id.clone(),
        objective: format!(
            "Make {}'s verified progress legible to its audience",
            brief.identity.product.name
        ),
        audience,
        narrative,
        horizon_days: config.horizon_days,
        items: vec![
            PromotionItem {
                channel: "telegram".into(),
                format: "milestone-note".into(),
                angle: format!(
                    "{focus}. Show what changed, why it matters, and how it was verified"
                ),
                success_metric: "qualified replies and channel views".into(),
            },
            PromotionItem {
                channel: "github".into(),
                format: "release-story".into(),
                angle: "Connect the shipped capability to the product mission".into(),
                success_metric: "release views, clones, stars, and follow-on issues".into(),
            },
        ],
        stop_conditions: vec![
            "Pause when claims lack evidence".into(),
            "Revise when two consecutive snapshots miss the target metric".into(),
        ],
        evidence: metrics.evidence.clone(),
        provider: "deterministic-fallback".into(),
    };
    append_jsonl(&state_path(root, "promotion-plans.jsonl"), &plan)?;
    queue_padagonia(root, &config, "PromotionPlan", &plan, &plan.evidence)?;
    Ok(plan)
}

fn latest_metric(root: &Path) -> Option<MetricSnapshot> {
    fs::read_to_string(state_path(root, "metrics.jsonl"))
        .ok()?
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str(line).ok())
}

fn promotion_focus(previous: Option<&MetricSnapshot>, current: &MetricSnapshot) -> String {
    let delta = |source: &BTreeMap<String, u64>, old: &BTreeMap<String, u64>, key: &str| {
        source
            .get(key)
            .copied()
            .unwrap_or(0)
            .saturating_sub(old.get(key).copied().unwrap_or(0))
    };
    if let Some(previous) = previous {
        if delta(&current.telegram, &previous.telegram, "members") > 0 {
            return "Community momentum: turn member growth into qualified conversation".into();
        }
        if delta(&current.github, &previous.github, "stars") > 0 {
            return "Adoption momentum: connect new GitHub attention to a concrete outcome".into();
        }
    }
    if current.commits_30d >= 10 {
        "Shipping cadence: make sustained engineering progress legible".into()
    } else {
        "Evidence baseline: earn attention with one verified product truth".into()
    }
}

pub fn create_milestone_draft(
    root: &Path,
    evidence: &MilestoneEvidence,
) -> Result<Option<ContentDraft>> {
    let config = PromotionConfig::load(root)?;
    let score = score_milestone(evidence, config.milestone_threshold);
    if !score.qualifies {
        return Ok(None);
    }
    if list_queue(root)?
        .iter()
        .any(|draft| draft.evidence_ids.contains(&evidence.id))
    {
        return Ok(None);
    }
    let brief = Brief::load(root)?;
    let guidelines = Guidelines::load(root)?;
    let content = format!(
        "{} reached {}. {} Verified by: {}.",
        brief.identity.product.name,
        evidence.title.trim_end_matches('.'),
        brief.identity.product.mission,
        evidence.evidence.join(", ")
    );
    validate_content(&guidelines, &content)?;
    let stable = evidence
        .id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let id = format!("draft-{}", stable.trim_matches('-'));
    let mut draft = ContentDraft {
        id,
        project_id: config.project_id.clone(),
        channel: "telegram".into(),
        content,
        evidence_ids: evidence.evidence.clone(),
        status: DraftStatus::Pending,
        actor: "brandi-milestone".into(),
        updated_at: now(),
        reason: None,
        revision_hash: String::new(),
        approved_destination: None,
        approval_expires_at: None,
    };
    draft.ensure_revision_hash();
    append_queue(root, &draft)?;
    queue_padagonia(root, &config, "ContentDraft", &draft, &draft.evidence_ids)?;
    Ok(Some(draft))
}

#[derive(Debug, Deserialize, Default)]
struct KaptaindShipIndex {
    #[serde(default)]
    ships: Vec<KaptaindShip>,
}

#[derive(Debug, Deserialize)]
struct KaptaindShip {
    #[serde(default = "default_ship_kind")]
    kind: String,
    version: String,
    shipped_at: i64,
    #[serde(default)]
    targets: Vec<String>,
    #[serde(default)]
    channels: Vec<String>,
    #[serde(default)]
    artifacts: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct KaptaindReleaseIndex {
    #[serde(default)]
    releases: Vec<KaptaindRelease>,
}

#[derive(Debug, Deserialize)]
struct KaptaindRelease {
    version: String,
    commit: String,
    released_at: i64,
    stability: f32,
    intent: String,
}

fn default_ship_kind() -> String {
    "manual".into()
}

/// Convert Kaptaind's durable release indexes into evidence-scored approval drafts.
/// This consumes metadata only; it never runs a release or changes Kaptaind state.
pub fn ingest_kaptaind_milestones(root: &Path) -> Result<Vec<ContentDraft>> {
    let mut created = Vec::new();
    let mut shipped_versions = HashSet::new();
    let ship_path = root.join(".kaptaind/ship/index.json");
    if ship_path.exists() {
        let index: KaptaindShipIndex = serde_json::from_str(&fs::read_to_string(ship_path)?)?;
        for ship in index.ships {
            shipped_versions.insert(ship.version.clone());
            let evidence_id = format!("kaptaind:ship:{}:{}", ship.version, ship.shipped_at);
            let anchors = vec![
                evidence_id.clone(),
                format!("kaptaind:artifacts:{}", ship.artifacts.len()),
                format!("kaptaind:targets:{}", ship.targets.join(",")),
            ];
            let evidence = MilestoneEvidence {
                id: evidence_id,
                title: format!("shipped v{} ({})", ship.version, ship.kind),
                release_success: true,
                qualification_success: true,
                semver_kind: semver_kind(&ship.version),
                novelty: if ship.channels.is_empty() { 0.4 } else { 0.7 },
                metric_delta: 0.0,
                evidence: anchors,
            };
            if let Some(draft) = create_milestone_draft(root, &evidence)? {
                created.push(draft);
            }
        }
    }

    let release_path = root.join(".kaptaind/releases/index.json");
    if release_path.exists() {
        let index: KaptaindReleaseIndex = serde_json::from_str(&fs::read_to_string(release_path)?)?;
        for release in index.releases {
            if shipped_versions.contains(&release.version) {
                continue;
            }
            let evidence_id = format!(
                "kaptaind:release:{}:{}",
                release.version, release.released_at
            );
            let evidence = MilestoneEvidence {
                id: evidence_id.clone(),
                title: format!("released v{} — {}", release.version, release.intent),
                release_success: true,
                qualification_success: release.stability >= 0.8,
                semver_kind: semver_kind(&release.version),
                novelty: 0.5,
                metric_delta: 0.0,
                evidence: vec![
                    evidence_id,
                    format!("git:{}", release.commit),
                    format!("kaptaind:stability:{:.3}", release.stability),
                ],
            };
            if let Some(draft) = create_milestone_draft(root, &evidence)? {
                created.push(draft);
            }
        }
    }
    Ok(created)
}

fn semver_kind(version: &str) -> String {
    let parts = version
        .trim_start_matches('v')
        .split('.')
        .collect::<Vec<_>>();
    if parts.get(2).is_some_and(|part| *part != "0") {
        "patch"
    } else if parts.get(1).is_some_and(|part| *part != "0") {
        "minor"
    } else {
        "major"
    }
    .into()
}

fn validate_content(guidelines: &Guidelines, content: &str) -> Result<()> {
    let lower = content.to_lowercase();
    for (category, terms) in &guidelines.prohibited.categories {
        for term in terms {
            if lower.contains(&term.to_lowercase()) {
                return Err(BrandiError::Invalid(format!(
                    "milestone copy contains prohibited {category} term `{term}`"
                )));
            }
        }
    }
    if content.matches('!').count() > guidelines.voice.style.max_exclamation_marks {
        return Err(BrandiError::Invalid(
            "milestone copy exceeds the guidelines exclamation budget".into(),
        ));
    }
    Ok(())
}

pub fn list_queue(root: &Path) -> Result<Vec<ContentDraft>> {
    let path = state_path(root, "content-queue.jsonl");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut latest = HashMap::new();
    for line in fs::read_to_string(path)?.lines() {
        if let Ok(mut draft) = serde_json::from_str::<ContentDraft>(line) {
            draft.ensure_revision_hash();
            latest.insert(draft.id.clone(), draft);
        }
    }
    let mut drafts = latest.into_values().collect::<Vec<_>>();
    drafts.sort_by_key(|draft| draft.updated_at);
    Ok(drafts)
}

pub fn decide(
    root: &Path,
    id: &str,
    approve: bool,
    identity: &ActionIdentity,
    confirmation: &str,
    destination: Option<&str>,
    reason: Option<String>,
) -> Result<ContentDraft> {
    let actor = identity.label();
    let action = if approve { "approve" } else { "reject" };
    let outcome = (|| {
        let mut draft = list_queue(root)?
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| BrandiError::NotFound(format!("content draft {id}")))?;
        let project_id = PromotionConfig::load(root)?.project_id;
        if draft.project_id != project_id {
            return Err(BrandiError::Invalid(format!(
                "draft {id} belongs to project `{}`, not `{project_id}`",
                draft.project_id
            )));
        }
        if draft.status != DraftStatus::Pending {
            return Err(BrandiError::Invalid(format!(
                "draft {id} is no longer pending"
            )));
        }
        if confirmation != draft.revision_hash && confirmation != draft.id {
            return Err(BrandiError::Invalid(format!(
                "confirmation must equal draft ID `{}` or revision hash `{}`",
                draft.id, draft.revision_hash
            )));
        }
        if approve && destination.is_none_or(str::is_empty) {
            return Err(BrandiError::Invalid(
                "approval requires an explicit destination".into(),
            ));
        }
        draft.status = if approve {
            DraftStatus::Approved
        } else {
            DraftStatus::Rejected
        };
        draft.actor = actor.clone();
        draft.reason = reason;
        draft.updated_at = now();
        if approve {
            let destination = destination.ok_or_else(|| {
                BrandiError::Invalid("approval requires an explicit destination".into())
            })?;
            draft.approved_destination = Some(destination.to_string());
            draft.approval_expires_at = Some(now() + 15 * 60);
        } else {
            draft.approved_destination = None;
            draft.approval_expires_at = None;
        }
        append_queue(root, &draft)?;
        Ok(draft)
    })();
    let (status, detail) = match &outcome {
        Ok(_) => ("allowed", None),
        Err(error) => ("denied", Some(error.to_string())),
    };
    append_security_audit(
        root,
        action,
        &actor,
        id,
        destination,
        status,
        detail.as_deref(),
    )?;
    outcome
}

/// Record the result of an external delivery without changing the stored copy.
/// Only an explicitly approved draft may advance to a terminal delivery state.
pub fn record_delivery(
    root: &Path,
    id: &str,
    published: bool,
    identity: &ActionIdentity,
    destination: &str,
    reason: Option<String>,
) -> Result<ContentDraft> {
    let actor = identity.label();
    let outcome = (|| {
        let mut draft = list_queue(root)?
            .into_iter()
            .find(|draft| draft.id == id)
            .ok_or_else(|| BrandiError::NotFound(format!("content draft {id}")))?;
        if !draft.approval_is_valid_for(destination) {
            return Err(BrandiError::Invalid(format!(
                "draft {id} has no current approval for destination `{destination}`"
            )));
        }
        draft.status = if published {
            DraftStatus::Published
        } else {
            DraftStatus::Failed
        };
        draft.actor = actor.clone();
        draft.reason = reason;
        draft.updated_at = now();
        append_queue(root, &draft)?;
        Ok(draft)
    })();
    let audit_outcome = match &outcome {
        Ok(_) if published => "allowed",
        Ok(_) => "failed",
        Err(_) => "denied",
    };
    let denied_reason = outcome.as_ref().err().map(ToString::to_string);
    let delivery_reason = outcome
        .as_ref()
        .ok()
        .and_then(|draft| draft.reason.as_deref());
    append_security_audit(
        root,
        if published {
            "publish"
        } else {
            "delivery-failed"
        },
        &actor,
        id,
        Some(destination),
        audit_outcome,
        denied_reason.as_deref().or(delivery_reason),
    )?;
    outcome
}

fn append_queue(root: &Path, draft: &ContentDraft) -> Result<()> {
    append_jsonl(&state_path(root, "content-queue.jsonl"), draft)
}

#[derive(Debug, Serialize, Deserialize)]
struct SecurityAuditEvent {
    timestamp: u64,
    action: String,
    actor: String,
    resource: String,
    destination: Option<String>,
    outcome: String,
    reason: Option<String>,
    previous_hash: String,
    event_hash: String,
}

fn append_security_audit(
    root: &Path,
    action: &str,
    actor: &str,
    resource: &str,
    destination: Option<&str>,
    outcome: &str,
    reason: Option<&str>,
) -> Result<()> {
    let path = state_path(root, "security-audit.jsonl");
    let previous_hash = fs::read_to_string(&path)
        .ok()
        .and_then(|contents| contents.lines().next_back().map(str::to_string))
        .and_then(|line| serde_json::from_str::<SecurityAuditEvent>(&line).ok())
        .map(|event| event.event_hash)
        .unwrap_or_else(|| "genesis".into());
    let timestamp = now();
    let mut digest = Sha256::new();
    for value in [
        timestamp.to_string(),
        action.to_string(),
        actor.to_string(),
        resource.to_string(),
        destination.unwrap_or_default().to_string(),
        outcome.to_string(),
        reason.unwrap_or_default().to_string(),
        previous_hash.clone(),
    ] {
        digest.update(value.as_bytes());
        digest.update([0]);
    }
    let event = SecurityAuditEvent {
        timestamp,
        action: action.into(),
        actor: actor.into(),
        resource: resource.into(),
        destination: destination.map(str::to_string),
        outcome: outcome.into(),
        reason: reason.map(str::to_string),
        previous_hash,
        event_hash: format!("sha256:{:x}", digest.finalize()),
    };
    append_jsonl(&path, &event)
}

fn state_path(root: &Path, name: &str) -> PathBuf {
    root.join(".brandi/state").join(name)
}

fn append_jsonl<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(value)?)?;
    Ok(())
}

#[derive(Serialize)]
struct OutboxRecord<'a, T> {
    external_id: String,
    label: &'a str,
    payload: &'a T,
    evidence: &'a [String],
    created_at: u64,
}

fn queue_padagonia<T: Serialize>(
    root: &Path,
    config: &PromotionConfig,
    label: &str,
    payload: &T,
    evidence: &[String],
) -> Result<()> {
    let record = OutboxRecord {
        external_id: format!("{}-{label}-{}", config.project_id, now()),
        label,
        payload,
        evidence,
        created_at: now(),
    };
    append_jsonl(&state_path(root, "padagonia-outbox.jsonl"), &record)
}

pub fn sync_padagonia(root: &Path) -> Result<usize> {
    let config = PromotionConfig::load(root)?;
    if config.padagonia_url.is_empty() {
        return Ok(0);
    }
    let path = state_path(root, "padagonia-outbox.jsonl");
    if !path.exists() {
        return Ok(0);
    }
    let key = std::env::var(&config.padagonia_key_env).unwrap_or_default();
    if key.is_empty() {
        return Ok(0);
    }
    let lines = fs::read_to_string(&path)?
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut sent = 0usize;
    let mut remaining = Vec::new();
    for line in lines {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) else {
            remaining.push(line);
            continue;
        };
        let body = serde_json::json!({"label":record["label"],"properties":{"external_id":record["external_id"],"payload":record["payload"],"project_id":config.project_id},"provenance":{"agent":"brandi-automation","model":"deterministic","confidence":1.0,"evidence":record["evidence"]}});
        let mut curl_cfg = String::new();
        curl_cfg += &curl_config_line(
            "url",
            &format!(
                "{}/api/v1/nodes",
                config.padagonia_url.trim_end_matches('/')
            ),
        );
        curl_cfg += &curl_config_line("header", "Content-Type: application/json");
        curl_cfg += &curl_config_line("header", &format!("Authorization: Bearer {key}"));
        curl_cfg += &curl_config_line("data-binary", &body.to_string());
        let output = curl_with_config(&["-sS", "--fail-with-body", "-X", "POST"], &curl_cfg);
        if output.is_some_and(|o| o.status.success()) {
            sent += 1
        } else {
            remaining.push(line);
        }
    }
    let tmp = path.with_extension("tmp");
    let body = if remaining.is_empty() {
        String::new()
    } else {
        format!("{}\n", remaining.join("\n"))
    };
    fs::write(&tmp, body)?;
    fs::rename(tmp, path)?;
    Ok(sent)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TelegramConfig {
    pub mode: String,
    pub public_mode: bool,
    pub allowed_users: Vec<i64>,
    pub projects: Vec<BotProject>,
}
impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            mode: "hub".into(),
            public_mode: false,
            allowed_users: Vec::new(),
            projects: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BotProject {
    pub id: String,
    pub path: PathBuf,
    pub token_env: String,
    pub chat_id: i64,
    pub roles: TelegramRoles,
}
impl Default for BotProject {
    fn default() -> Self {
        Self {
            id: String::new(),
            path: PathBuf::from("."),
            token_env: String::new(),
            chat_id: 0,
            roles: TelegramRoles::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct TelegramRoles {
    pub viewers: Vec<i64>,
    pub planners: Vec<i64>,
    pub approvers: Vec<i64>,
    pub publishers: Vec<i64>,
    pub administrators: Vec<i64>,
}

impl TelegramRoles {
    fn allows(&self, user: i64, role: AuthorizationRole) -> bool {
        self.administrators.contains(&user)
            || match role {
                AuthorizationRole::Viewer => self.viewers.contains(&user),
                AuthorizationRole::Planner => self.planners.contains(&user),
                AuthorizationRole::Approver => self.approvers.contains(&user),
                AuthorizationRole::Publisher => self.publishers.contains(&user),
                AuthorizationRole::Administrator => self.administrators.contains(&user),
                AuthorizationRole::DeviceOperator => false,
            }
    }
}

pub fn validate_telegram_config(config: &TelegramConfig) -> Result<()> {
    const MAX_TELEGRAM_PROJECTS: usize = 16;
    if config.mode != "hub" && config.mode != "fleet" {
        return Err(BrandiError::Daemon(
            "telegram mode must be `hub` or `fleet`".into(),
        ));
    }
    if config.projects.is_empty() {
        return Err(BrandiError::Daemon(
            "telegram config has no projects".into(),
        ));
    }
    if config.projects.len() > MAX_TELEGRAM_PROJECTS {
        return Err(BrandiError::Daemon(format!(
            "telegram config exceeds the {MAX_TELEGRAM_PROJECTS} worker limit"
        )));
    }
    if config.allowed_users.is_empty() && !config.public_mode {
        return Err(BrandiError::Daemon(
            "telegram allowed_users is empty; authorization fails closed unless public_mode is explicitly true".into(),
        ));
    }
    let mut ids = HashSet::new();
    for project in &config.projects {
        if project.id.is_empty() || !ids.insert(project.id.clone()) {
            return Err(BrandiError::Daemon(
                "telegram project IDs must be non-empty and unique".into(),
            ));
        }
        if !project.path.is_dir() {
            return Err(BrandiError::Daemon(format!(
                "telegram project path does not exist: {}",
                project.path.display()
            )));
        }
        if config.mode == "fleet" && project.token_env.is_empty() {
            return Err(BrandiError::Daemon(format!(
                "fleet project {} has no token_env",
                project.id
            )));
        }
        let assigned_users = project
            .roles
            .viewers
            .iter()
            .chain(&project.roles.planners)
            .chain(&project.roles.approvers)
            .chain(&project.roles.publishers)
            .chain(&project.roles.administrators);
        if assigned_users
            .into_iter()
            .any(|user| !config.public_mode && !config.allowed_users.contains(user))
        {
            return Err(BrandiError::Daemon(format!(
                "project {} assigns a role to a user outside allowed_users",
                project.id
            )));
        }
    }
    let primary = &config.projects[0];
    if primary.token_env.is_empty() {
        return Err(BrandiError::Daemon(
            "the hub/first project must define token_env".into(),
        ));
    }
    Ok(())
}

pub fn run_telegram_supervisor(path: &Path) -> Result<()> {
    let config: TelegramConfig = serde_yaml::from_str(&fs::read_to_string(path)?)?;
    validate_telegram_config(&config)?;
    if config.public_mode {
        crate::output::warning(
            "Telegram public_mode is enabled; unauthenticated viewers can access read-only commands",
        )?;
    }
    if config.mode == "hub" {
        return telegram_hub_worker(config.projects, config.allowed_users, config.public_mode);
    }
    let mut workers = Vec::new();
    for project in config.projects.clone() {
        let allowed = config.allowed_users.clone();
        let public_mode = config.public_mode;
        workers.push(thread::spawn(move || {
            telegram_worker(project, allowed, public_mode)
        }));
    }
    for worker in workers {
        worker
            .join()
            .map_err(|_| BrandiError::Daemon("telegram worker panicked".into()))??;
    }
    Ok(())
}

fn telegram_worker(project: BotProject, allowed: Vec<i64>, public_mode: bool) -> Result<()> {
    let token = std::env::var(&project.token_env)
        .map_err(|_| BrandiError::Daemon(format!("missing {}", project.token_env)))?;
    let offset_path = state_path(&project.path, "telegram-offset");
    let mut offset = fs::read_to_string(&offset_path)
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or(0);
    loop {
        announce_milestones(&project, &token);
        if let Some(updates) = telegram_updates(&token, offset) {
            for update in &updates {
                let next = update["update_id"].as_i64().unwrap_or(offset) + 1;
                if next > offset {
                    offset = next;
                    persist_offset(&offset_path, offset)?;
                }
                let Some(message) = update.get("message") else {
                    continue;
                };
                let user = message["from"]["id"].as_i64().unwrap_or(0);
                if !public_mode && !allowed.contains(&user) {
                    let _ = append_security_audit(
                        &project.path,
                        "telegram-command",
                        &format!("telegram-user:{user}"),
                        text_from_message(message),
                        None,
                        "denied",
                        Some("user is outside allowed_users"),
                    );
                    continue;
                }
                let chat = message["chat"]["id"].as_i64().unwrap_or(project.chat_id);
                let text = message["text"].as_str().unwrap_or("");
                if text.starts_with('/') {
                    let answer =
                        handle_bot_command(&project, user, text, &token, chat, public_mode);
                    let _ = telegram_send(&token, chat, &answer);
                }
            }
        } else {
            thread::sleep(Duration::from_secs(5));
        }
    }
}

fn telegram_hub_worker(
    projects: Vec<BotProject>,
    allowed: Vec<i64>,
    public_mode: bool,
) -> Result<()> {
    let primary = projects
        .first()
        .ok_or_else(|| BrandiError::Daemon("hub has no project routes".into()))?;
    let token = std::env::var(&primary.token_env)
        .map_err(|_| BrandiError::Daemon(format!("missing {}", primary.token_env)))?;
    let offset_path = state_path(&primary.path, "telegram-hub-offset");
    let mut offset = fs::read_to_string(&offset_path)
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .unwrap_or(0);
    loop {
        for project in &projects {
            announce_milestones(project, &token);
        }
        if let Some(updates) = telegram_updates(&token, offset) {
            for update in &updates {
                offset = offset.max(update["update_id"].as_i64().unwrap_or(offset) + 1);
                persist_offset(&offset_path, offset)?;
                let Some(message) = update.get("message") else {
                    continue;
                };
                let user = message["from"]["id"].as_i64().unwrap_or(0);
                if !public_mode && !allowed.contains(&user) {
                    let _ = append_security_audit(
                        &primary.path,
                        "telegram-command",
                        &format!("telegram-user:{user}"),
                        text_from_message(message),
                        None,
                        "denied",
                        Some("user is outside allowed_users"),
                    );
                    continue;
                }
                let chat = message["chat"]["id"].as_i64().unwrap_or(0);
                let raw = message["text"].as_str().unwrap_or("");
                let (route, text) = parse_hub_route(raw);
                let project = route
                    .and_then(|id| projects.iter().find(|project| project.id == id))
                    .or_else(|| projects.iter().find(|project| project.chat_id == chat))
                    .unwrap_or(primary);
                let answer = handle_bot_command(project, user, text, &token, chat, public_mode);
                let _ = telegram_send(&token, chat, &answer);
            }
        } else {
            thread::sleep(Duration::from_secs(5));
        }
    }
}

fn parse_hub_route(text: &str) -> (Option<&str>, &str) {
    let Some(rest) = text.strip_prefix("/project ") else {
        return (None, text);
    };
    let mut parts = rest.trim().splitn(2, ' ');
    (parts.next(), parts.next().unwrap_or("/status"))
}

fn text_from_message(message: &serde_json::Value) -> &str {
    message["text"].as_str().unwrap_or("")
}

fn telegram_updates(token: &str, offset: i64) -> Option<Vec<serde_json::Value>> {
    let url = format!("https://api.telegram.org/bot{token}/getUpdates?timeout=30&offset={offset}&allowed_updates=%5B%22message%22%5D");
    let curl_cfg = curl_config_line("url", &url);
    let output = curl_with_config(&["-sS", "--fail-with-body"], &curl_cfg)?;
    if !output.status.success() {
        return None;
    }
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    response["result"].as_array().cloned()
}

fn persist_offset(path: &Path, offset: i64) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, offset.to_string())?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn announce_milestones(project: &BotProject, token: &str) {
    if project.chat_id == 0 {
        return;
    }
    if let Ok(drafts) = ingest_kaptaind_milestones(&project.path) {
        for draft in drafts {
            let message = format!(
                "Milestone draft {} awaits approval:\nRevision: {}\nDestination: telegram:{}\nExpires 15 minutes after approval.\n\n{}\n\n/approve {} {} or /reject {} {}",
                draft.id,
                draft.revision_hash,
                project.chat_id,
                draft.content,
                draft.id,
                draft.revision_hash,
                draft.id,
                draft.revision_hash,
            );
            let _ = telegram_send(token, project.chat_id, &message);
        }
    }
}

fn handle_bot_command(
    project: &BotProject,
    user: i64,
    text: &str,
    token: &str,
    reply_chat: i64,
    public_mode: bool,
) -> String {
    let mut parts = text.splitn(2, ' ');
    let command = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("").trim();
    match command.split('@').next().unwrap_or(command) {
        "/status" if public_mode || project.roles.allows(user, AuthorizationRole::Viewer) => {
            status_evidence(&project.path)
        }
        "/metrics" if project.roles.allows(user, AuthorizationRole::Planner) => {
            PromotionConfig::load(&project.path)
                .and_then(|c| collect_metrics(&project.path, &c))
                .and_then(|v| serde_json::to_string_pretty(&v).map_err(Into::into))
                .unwrap_or_else(|e| e.to_string())
        }
        "/campaigns" if project.roles.allows(user, AuthorizationRole::Planner) => {
            build_plan(&project.path)
                .and_then(|v| serde_json::to_string_pretty(&v).map_err(Into::into))
                .unwrap_or_else(|e| e.to_string())
        }
        "/approve"
            if project.roles.allows(user, AuthorizationRole::Approver)
                && project.roles.allows(user, AuthorizationRole::Publisher) =>
        {
            match parse_decision_args(arg) {
                Some((id, confirmation)) => {
                    approve_and_publish(project, id, confirmation, user, token, reply_chat)
                        .unwrap_or_else(|e| e.to_string())
                }
                None => "Usage: /approve <draft-id> <revision-hash>".into(),
            }
        }
        "/reject" if project.roles.allows(user, AuthorizationRole::Approver) => {
            match parse_decision_args(arg) {
                Some((id, confirmation)) => decide(
                    &project.path,
                    id,
                    false,
                    &ActionIdentity::telegram(user),
                    confirmation,
                    None,
                    Some("Rejected in Telegram".into()),
                )
                .map(|d| format!("Rejected {}.", d.id))
                .unwrap_or_else(|e| e.to_string()),
                None => "Usage: /reject <draft-id> <revision-hash>".into(),
            }
        }
        "/ask" if public_mode || project.roles.allows(user, AuthorizationRole::Viewer) => {
            groq_answer(arg, &route_evidence(&project.path, arg))
                .unwrap_or_else(|| route_evidence(&project.path, arg))
        }
        _ => {
            let _ = append_security_audit(
                &project.path,
                "telegram-command",
                &format!("telegram-user:{user}"),
                command,
                None,
                "denied",
                Some("command is unknown or the assigned role is insufficient"),
            );
            "Command denied or unavailable for your assigned role.".into()
        }
    }
}

fn parse_decision_args(value: &str) -> Option<(&str, &str)> {
    let mut parts = value.split_whitespace();
    let id = parts.next()?;
    let confirmation = parts.next()?;
    parts.next().is_none().then_some((id, confirmation))
}

fn approve_and_publish(
    project: &BotProject,
    id: &str,
    confirmation: &str,
    user: i64,
    token: &str,
    reply_chat: i64,
) -> Result<String> {
    let destination = if project.chat_id == 0 {
        reply_chat
    } else {
        project.chat_id
    };
    let destination_key = format!("telegram:{destination}");
    let identity = ActionIdentity::telegram(user);
    let draft = decide(
        &project.path,
        id,
        true,
        &identity,
        confirmation,
        Some(&destination_key),
        None,
    )?;
    let published = telegram_send(token, destination, &draft.content);
    record_delivery(
        &project.path,
        id,
        published,
        &identity,
        &destination_key,
        (!published).then(|| "Telegram send failed".into()),
    )?;
    if published {
        Ok(format!("Published exact approved revision {}.", draft.id))
    } else {
        Err(BrandiError::Network(format!(
            "publishing {} failed; the queue records the failure",
            draft.id
        )))
    }
}

fn status_evidence(root: &Path) -> String {
    let loaded = Brief::load(root).and_then(|brief| Ok((brief, Guidelines::load(root)?)));
    let report = loaded.and_then(|(brief, guidelines)| {
        let findings = rules::run_rules(&brief, &guidelines, root)?;
        let surfaces = surface::scan_surfaces(root)?;
        Ok(report::build_report(root, findings, &surfaces))
    });
    report
        .map(|r| {
            format!(
                "Brand coherence: {}/100 — {} errors, {} warnings.",
                r.scores.overall, r.scores.errors, r.scores.warnings
            )
        })
        .unwrap_or_else(|e| e.to_string())
}

fn route_evidence(root: &Path, question: &str) -> String {
    let lower = question.to_lowercase();
    if lower.contains("campaign") || lower.contains("promotion") {
        Brief::load(root)
            .map(|brief| social::render_plan(&brief, None))
            .unwrap_or_else(|e| e.to_string())
    } else if lower.contains("commit") || lower.contains("git") {
        format!(
            "Git HEAD: {}; recent commits: {}",
            git(root, &["rev-parse", "--short", "HEAD"]).unwrap_or_default(),
            git(root, &["log", "-5", "--oneline"]).unwrap_or_default()
        )
    } else {
        status_evidence(root)
    }
}

fn groq_answer(question: &str, evidence: &str) -> Option<String> {
    let key = std::env::var("GROQ_API_KEY").ok()?;
    let body = serde_json::json!({"model":"groq/compound-mini","messages":[{"role":"system","content":"Answer from the supplied local evidence. Say when evidence is insufficient. Never claim to have executed actions."},{"role":"user","content":format!("Question: {question}\n\nLocal evidence:\n{evidence}")}],"compound_custom":{"tools":{"enabled_tools":["web_search","visit_website"]}}});
    let mut curl_cfg = String::new();
    curl_cfg += &curl_config_line("url", "https://api.groq.com/openai/v1/chat/completions");
    curl_cfg += &curl_config_line("header", "Content-Type: application/json");
    curl_cfg += &curl_config_line("header", &format!("Authorization: Bearer {key}"));
    curl_cfg += &curl_config_line("data-binary", &body.to_string());
    let output = curl_with_config(&["-sS", "--fail-with-body", "-X", "POST"], &curl_cfg)?;
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    response["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
}

fn telegram_send(token: &str, chat: i64, text: &str) -> bool {
    let body = serde_json::json!({"chat_id":chat,"text":text,"disable_web_page_preview":true});
    let mut curl_cfg = String::new();
    curl_cfg += &curl_config_line(
        "url",
        &format!("https://api.telegram.org/bot{token}/sendMessage"),
    );
    curl_cfg += &curl_config_line("header", "Content-Type: application/json");
    curl_cfg += &curl_config_line("data-binary", &body.to_string());
    curl_with_config(&["-sS", "--fail-with-body", "-X", "POST"], &curl_cfg)
        .is_some_and(|o| o.status.success())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curl_config_lines_never_leak_secrets_unescaped() {
        let line = curl_config_line("header", "Authorization: Bearer abc\"; rm -rf /");
        assert_eq!(
            line,
            "header = \"Authorization: Bearer abc\\\"; rm -rf /\"\n"
        );
        let body = curl_config_line("data-binary", "{\"text\":\"line1\\nline2\"}");
        assert!(body.contains("\\\"text\\\""));
        assert!(body.contains("\\\\n"));
    }

    #[test]
    fn curl_config_line_round_trips_through_escaping() {
        for raw in [
            "https://api.telegram.org/botAA:BB/sendMessage",
            "Authorization: Bearer plain-token",
            "path\\with\\backslashes",
            "quote\"inside",
        ] {
            let line = curl_config_line("url", raw);
            assert!(line.starts_with("url = \""));
            assert!(line.trim_end().ends_with('"'));
        }
    }

    #[test]
    fn milestone_requires_anchor_and_threshold() {
        let evidence = MilestoneEvidence {
            id: "m1".into(),
            title: "Shipped".into(),
            release_success: true,
            qualification_success: true,
            semver_kind: "minor".into(),
            novelty: 0.8,
            metric_delta: 0.2,
            evidence: vec!["release:v1".into()],
        };
        let score = score_milestone(&evidence, 40);
        assert!(score.qualifies);
        assert!(score.score >= 40);
        let routine = MilestoneEvidence {
            release_success: false,
            qualification_success: true,
            novelty: 0.2,
            evidence: vec!["commit:a".into()],
            ..evidence
        };
        assert!(!score_milestone(&routine, 40).qualifies);
    }

    #[test]
    fn queue_is_append_only_and_materialized() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".brandi/state")).unwrap();
        let project_id = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let mut draft = ContentDraft {
            id: "d1".into(),
            project_id,
            channel: "telegram".into(),
            content: "verified".into(),
            evidence_ids: vec!["e1".into()],
            status: DraftStatus::Pending,
            actor: "test".into(),
            updated_at: 1,
            reason: None,
            revision_hash: String::new(),
            approved_destination: None,
            approval_expires_at: None,
        };
        draft.ensure_revision_hash();
        append_queue(dir.path(), &draft).unwrap();
        let identity = ActionIdentity::test_local(1000);
        decide(
            dir.path(),
            "d1",
            true,
            &identity,
            "d1",
            Some("adb:test"),
            None,
        )
        .unwrap();
        assert_eq!(
            list_queue(dir.path()).unwrap()[0].status,
            DraftStatus::Approved
        );
        record_delivery(dir.path(), "d1", true, &identity, "adb:test", None).unwrap();
        assert_eq!(
            list_queue(dir.path()).unwrap()[0].status,
            DraftStatus::Published
        );
        assert_eq!(
            fs::read_to_string(state_path(dir.path(), "content-queue.jsonl"))
                .unwrap()
                .lines()
                .count(),
            3
        );
    }

    #[test]
    fn kaptaind_ship_is_ingested_once() {
        let dir = tempfile::tempdir().unwrap();
        Brief::scaffold(dir.path()).unwrap();
        Guidelines::scaffold(dir.path()).unwrap();
        let ship_dir = dir.path().join(".kaptaind/ship");
        fs::create_dir_all(&ship_dir).unwrap();
        fs::write(
            ship_dir.join("index.json"),
            r#"{"ships":[{"kind":"stable","version":"1.2.0","shipped_at":7,"targets":["linux"],"channels":["github"],"artifacts":["brandi"]}]}"#,
        )
        .unwrap();

        let first = ingest_kaptaind_milestones(dir.path()).unwrap();
        let second = ingest_kaptaind_milestones(dir.path()).unwrap();
        assert_eq!(first.len(), 1);
        assert!(second.is_empty());
        assert_eq!(first[0].status, DraftStatus::Pending);
        assert!(first[0].content.contains("shipped v1.2.0"));
    }

    #[test]
    fn hub_route_selects_project_and_command() {
        assert_eq!(
            parse_hub_route("/project alpha /status"),
            (Some("alpha"), "/status")
        );
        assert_eq!(parse_hub_route("/ask why"), (None, "/ask why"));
    }

    #[test]
    fn decision_arguments_require_exact_id_and_revision_hash() {
        assert_eq!(
            parse_decision_args("draft-1 sha256:abc"),
            Some(("draft-1", "sha256:abc"))
        );
        assert_eq!(parse_decision_args("draft-1"), None);
        assert_eq!(parse_decision_args("draft-1 sha256:abc extra"), None);
    }

    #[test]
    fn telegram_authorization_is_fail_closed_and_role_specific() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = TelegramConfig {
            mode: "hub".into(),
            public_mode: false,
            allowed_users: Vec::new(),
            projects: vec![BotProject {
                id: "p".into(),
                path: dir.path().to_path_buf(),
                token_env: "TEST_TOKEN".into(),
                chat_id: 1,
                roles: TelegramRoles {
                    viewers: vec![10],
                    planners: vec![11],
                    approvers: vec![12],
                    publishers: vec![13],
                    administrators: vec![99],
                },
            }],
        };
        assert!(validate_telegram_config(&config).is_err());
        config.allowed_users = vec![10, 11, 12, 13, 99];
        assert!(validate_telegram_config(&config).is_ok());
        let roles = &config.projects[0].roles;
        assert!(roles.allows(11, AuthorizationRole::Planner));
        assert!(!roles.allows(11, AuthorizationRole::Approver));
        assert!(roles.allows(99, AuthorizationRole::Publisher));
        config.allowed_users.clear();
        config.public_mode = true;
        assert!(validate_telegram_config(&config).is_ok());
    }

    #[test]
    fn approvals_deny_replay_stale_cross_project_and_cross_destination_use() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".brandi/state")).unwrap();
        let project_id = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let mut draft = ContentDraft {
            id: "d-sec".into(),
            project_id: project_id.clone(),
            channel: "telegram".into(),
            content: "verified revision".into(),
            evidence_ids: vec!["e1".into()],
            status: DraftStatus::Pending,
            actor: "system".into(),
            updated_at: 1,
            reason: None,
            revision_hash: String::new(),
            approved_destination: None,
            approval_expires_at: None,
        };
        draft.ensure_revision_hash();
        append_queue(dir.path(), &draft).unwrap();
        let identity = ActionIdentity::test_local(1000);
        let approved = decide(
            dir.path(),
            &draft.id,
            true,
            &identity,
            &draft.revision_hash,
            Some("telegram:42"),
            None,
        )
        .unwrap();
        assert!(approved.approval_is_valid_for("telegram:42"));
        assert!(!approved.approval_is_valid_for("telegram:43"));
        assert!(decide(
            dir.path(),
            &draft.id,
            true,
            &identity,
            &draft.id,
            Some("telegram:42"),
            None,
        )
        .is_err());

        let mut stale = approved.clone();
        stale.approval_expires_at = Some(now().saturating_sub(1));
        append_queue(dir.path(), &stale).unwrap();
        assert!(
            record_delivery(dir.path(), &draft.id, true, &identity, "telegram:42", None,).is_err()
        );

        let mut foreign = draft;
        foreign.id = "d-foreign".into();
        foreign.project_id = "another-project".into();
        foreign.status = DraftStatus::Pending;
        foreign.ensure_revision_hash();
        append_queue(dir.path(), &foreign).unwrap();
        assert!(decide(
            dir.path(),
            &foreign.id,
            true,
            &identity,
            &foreign.id,
            Some("telegram:42"),
            None,
        )
        .is_err());

        let audit = fs::read_to_string(state_path(dir.path(), "security-audit.jsonl")).unwrap();
        assert!(audit.lines().count() >= 3);
        let events = audit
            .lines()
            .map(|line| serde_json::from_str::<SecurityAuditEvent>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(events[0].previous_hash, "genesis");
        for pair in events.windows(2) {
            assert_eq!(pair[1].previous_hash, pair[0].event_hash);
        }
    }
}
