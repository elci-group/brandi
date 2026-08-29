//! Evidence-first multi-provider generation workflow for `brandi direct`.
//!
//! The planner deliberately follows RUGID's projector/platform split: planning
//! is pure and deterministic, while provider adapters own effects. Every
//! effect is written beneath a content-addressed run directory before a
//! selected asset can be promoted into the project.

use crate::assets;
use crate::brief::Brief;
use crate::error::{BrandiError, Result};
use crate::guidelines::{parse_hex_color, Guidelines, VisualLanguage};
use crate::process::{run_bounded, run_bounded_with_input, MAX_SUBPROCESS_OUTPUT_BYTES};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const CONFIG_PATH: &str = ".brandi/direct.yaml";
const STATE_DIR: &str = ".brandi/state/direct";
const MAX_RUNS: usize = 64;
const MAX_CONCURRENCY: usize = 8;
const MAX_SOURCE_IMAGES: usize = 32;
const MAX_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
const PROVIDER_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    Parallel,
    Sequential,
    Iterative,
}

impl Default for Strategy {
    fn default() -> Self {
        Self::Parallel
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum KaptaindMode {
    None,
    Analyze,
    Aoc,
    AocPush,
}

impl Default for KaptaindMode {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone)]
pub struct DirectOptions {
    pub section: String,
    pub runs: Option<usize>,
    pub strategy: Option<Strategy>,
    pub routes: Vec<String>,
    pub reviewer: Option<String>,
    pub max_concurrency: Option<usize>,
    pub max_cost_usd: Option<f64>,
    pub sources: Vec<PathBuf>,
    pub preflight_only: bool,
    pub kaptaind: KaptaindMode,
    pub confirm_postflight: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DirectConfig {
    pub schema_version: String,
    pub runs: usize,
    pub strategy: Strategy,
    pub max_concurrency: usize,
    pub max_cost_usd: Option<f64>,
    pub providers: Vec<ProviderConfig>,
    pub reviewer: Option<ReviewerConfig>,
    pub objectives: Vec<ObjectiveConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderConfig {
    pub id: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key_env: String,
    pub models: Vec<String>,
    pub command: Vec<String>,
    pub cost_per_run_usd: Option<f64>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            id: "vasilis".into(),
            kind: ProviderKind::Vasilis,
            base_url: String::new(),
            api_key_env: String::new(),
            models: vec!["deterministic-svg-v1".into()],
            command: Vec::new(),
            cost_per_run_usd: Some(0.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Vasilis,
    OpenaiImages,
    Command,
}

impl Default for ProviderKind {
    fn default() -> Self {
        Self::Vasilis
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReviewerConfig {
    pub id: String,
    pub kind: ReviewerKind,
    pub base_url: String,
    pub api_key_env: String,
    pub model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewerKind {
    OpenaiResponses,
    Deterministic,
}

impl Default for ReviewerKind {
    fn default() -> Self {
        Self::Deterministic
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ObjectiveConfig {
    pub id: String,
    pub description: String,
    pub width: u32,
    pub height: u32,
    pub output: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectPlan {
    pub schema_version: String,
    pub plan_id: String,
    pub project: PathBuf,
    pub section: String,
    pub section_value: serde_json::Value,
    pub system_prompt: String,
    pub strategy: Strategy,
    pub max_concurrency: usize,
    pub estimated_cost_usd: f64,
    pub max_cost_usd: Option<f64>,
    pub sources: Vec<SourceEvidence>,
    pub palette: Vec<ColorEvidence>,
    /// The nine visual-language axes from `.brandi/visual.yaml`, handed to
    /// generation providers such as VASILIS instead of hardcoded literals.
    pub visual_language: VisualLanguage,
    /// Forbidden visual motifs from `.brandi/prohibited.yaml`.
    pub forbidden_motifs: Vec<String>,
    pub objectives: Vec<ObjectiveConfig>,
    pub actions: Vec<GenerationAction>,
    pub reviewer: ReviewerRoute,
    pub postflight: PostflightPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEvidence {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorEvidence {
    pub hex: String,
    pub srgb: [u8; 3],
    pub relative_luminance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationAction {
    pub id: String,
    pub objective: String,
    pub provider: String,
    pub model: String,
    pub sequence: usize,
    pub parent_action: Option<String>,
    pub prompt: String,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewerRoute {
    pub provider: String,
    pub model: String,
    pub rubric: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostflightPlan {
    pub mode: KaptaindMode,
    pub confirmation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectEvidence {
    pub schema_version: String,
    pub plan: DirectPlan,
    pub started_at: String,
    pub completed_at: String,
    pub candidates: Vec<CandidateEvidence>,
    pub review: BatchReview,
    pub selected: BTreeMap<String, String>,
    pub artifacts: PreflightArtifacts,
    pub postflight: PostflightEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateEvidence {
    pub action_id: String,
    pub objective: String,
    pub provider: String,
    pub model: String,
    pub path: Option<PathBuf>,
    pub sha256: Option<String>,
    pub bytes: Option<u64>,
    pub duration_ms: u128,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchReview {
    pub provider: String,
    pub model: String,
    pub assessments: Vec<AssetAssessment>,
    pub reviewer_confidence: f64,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetAssessment {
    pub candidate_id: String,
    pub rank: usize,
    pub quality_score: f64,
    pub confidence: f64,
    pub constraint_scores: BTreeMap<String, f64>,
    pub strengths: Vec<String>,
    pub risks: Vec<String>,
    pub recommendation: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PreflightArtifacts {
    pub json: PathBuf,
    pub svg: PathBuf,
    pub rdf: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PostflightEvidence {
    pub requested: KaptaindMode,
    pub analyzed: bool,
    pub aoc_started: bool,
    pub aoc_shipped: bool,
    pub pushed: bool,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirectOutcome {
    pub plan_id: String,
    pub preflight_only: bool,
    pub section: String,
    pub section_value: serde_json::Value,
    pub palette: Vec<ColorEvidence>,
    pub strategy: Strategy,
    pub actions: usize,
    pub estimated_cost_usd: f64,
    pub routes: Vec<String>,
    pub evidence: PathBuf,
    pub preflight: PreflightArtifacts,
    pub generated: usize,
    pub selected: BTreeMap<String, String>,
    pub postflight_confirmation: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirectScopeOutcome {
    pub schema_version: String,
    pub portfolio: bool,
    pub root: PathBuf,
    pub projects: Vec<ProjectDirectOutcome>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectDirectOutcome {
    pub project: PathBuf,
    pub outcome: Option<DirectOutcome>,
    pub error: Option<String>,
}

impl DirectConfig {
    pub fn load(root: &Path, guidelines: &Guidelines) -> Result<Self> {
        let path = root.join(CONFIG_PATH);
        let mut config = if path.exists() {
            serde_yaml::from_str::<Self>(&fs::read_to_string(&path)?)?
        } else {
            Self::default_for(guidelines)
        };
        if config.schema_version.is_empty() {
            config.schema_version = "brandi-direct-config-v1".into();
        }
        validate_config(&config)?;
        Ok(config)
    }

    fn default_for(guidelines: &Guidelines) -> Self {
        Self {
            schema_version: "brandi-direct-config-v1".into(),
            runs: 1,
            strategy: Strategy::Parallel,
            max_concurrency: 1,
            max_cost_usd: Some(0.0),
            providers: vec![ProviderConfig::default()],
            reviewer: Some(ReviewerConfig {
                id: "deterministic".into(),
                kind: ReviewerKind::Deterministic,
                model: "asset-constraints-v1".into(),
                ..ReviewerConfig::default()
            }),
            objectives: vec![
                ObjectiveConfig {
                    id: "social-card".into(),
                    description: "the company social card".into(),
                    width: guidelines.visual.assets.social_card.width,
                    height: guidelines.visual.assets.social_card.height,
                    output: guidelines.visual.assets.social_card_path.clone(),
                },
                ObjectiveConfig {
                    id: "thumbnail".into(),
                    description: "the company thumbnail".into(),
                    width: guidelines.visual.assets.thumbnail.width,
                    height: guidelines.visual.assets.thumbnail.height,
                    output: guidelines.visual.assets.thumbnail_path.clone(),
                },
            ],
        }
    }
}

pub fn plan(root: &Path, options: &DirectOptions) -> Result<(DirectConfig, DirectPlan)> {
    Brief::validate(root)?;
    Guidelines::validate(root)?;
    let brief = Brief::load(root)?;
    let guidelines = Guidelines::load(root)?;
    let mut config = DirectConfig::load(root, &guidelines)?;
    if let Some(runs) = options.runs {
        config.runs = runs;
    }
    if let Some(strategy) = options.strategy {
        config.strategy = strategy;
    }
    if let Some(value) = options.max_concurrency {
        config.max_concurrency = value;
    }
    if options.max_cost_usd.is_some() {
        config.max_cost_usd = options.max_cost_usd;
    }
    validate_config(&config)?;

    let brief_value = serde_json::to_value(&brief)?;
    let section_value = select_section(&brief_value, &options.section)?;
    let sources = collect_sources(root, &options.sources)?;
    let palette = collect_palette(&guidelines);
    let routes = resolve_routes(&config, &options.routes)?;
    let system_prompt = system_prompt(&brief, &guidelines, &options.section);
    let actions = project_actions(&config, &routes, &system_prompt, &section_value);
    let estimated_cost_usd = actions.iter().map(|action| action.estimated_cost_usd).sum();
    if let Some(limit) = config.max_cost_usd {
        if estimated_cost_usd > limit + f64::EPSILON {
            return Err(BrandiError::Invalid(format!(
                "direct plan costs ${estimated_cost_usd:.4}, above the ${limit:.4} budget"
            )));
        }
    }
    let reviewer = resolve_reviewer(&config, options.reviewer.as_deref())?;
    let plan_basis = serde_json::json!({
        "section": options.section,
        "section_value": section_value,
        "strategy": config.strategy,
        "sources": sources,
        "palette": palette,
        "objectives": config.objectives,
        "actions": actions,
        "reviewer": reviewer,
    });
    let plan_id = format!(
        "direct-{}",
        &digest_bytes(&serde_json::to_vec(&plan_basis)?)[..16]
    );
    let confirmation = (!matches!(options.kaptaind, KaptaindMode::None | KaptaindMode::Analyze))
        .then(|| {
            format!(
                "{plan_id}:{}",
                match options.kaptaind {
                    KaptaindMode::Aoc => "commit",
                    KaptaindMode::AocPush => "push",
                    _ => "",
                }
            )
        });
    let plan = DirectPlan {
        schema_version: "brandi-direct-plan-v1".into(),
        plan_id,
        project: root.to_path_buf(),
        section: options.section.clone(),
        section_value,
        system_prompt,
        strategy: config.strategy,
        max_concurrency: config.max_concurrency,
        estimated_cost_usd,
        max_cost_usd: config.max_cost_usd,
        sources,
        palette,
        visual_language: guidelines.visual.visual_language.clone(),
        forbidden_motifs: guidelines.prohibited.visual_motifs.clone(),
        objectives: config.objectives.clone(),
        actions,
        reviewer,
        postflight: PostflightPlan {
            mode: options.kaptaind,
            confirmation,
        },
    };
    Ok((config, plan))
}

pub fn run(root: &Path, options: &DirectOptions) -> Result<DirectOutcome> {
    let (config, plan) = plan(root, options)?;
    let run_dir = root.join(STATE_DIR).join(&plan.plan_id);
    fs::create_dir_all(run_dir.join("candidates"))?;
    let artifacts = write_preflight(&run_dir, &plan)?;
    if options.preflight_only {
        return Ok(DirectOutcome {
            plan_id: plan.plan_id.clone(),
            preflight_only: true,
            section: plan.section.clone(),
            section_value: plan.section_value.clone(),
            palette: plan.palette.clone(),
            strategy: plan.strategy,
            actions: plan.actions.len(),
            estimated_cost_usd: plan.estimated_cost_usd,
            routes: plan
                .actions
                .iter()
                .map(|item| format!("{}/{}", item.provider, item.model))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
            evidence: run_dir.join("evidence.json"),
            preflight: artifacts,
            generated: 0,
            selected: BTreeMap::new(),
            postflight_confirmation: plan.postflight.confirmation.clone(),
        });
    }
    require_postflight_confirmation(&plan, options.confirm_postflight.as_deref())?;
    let started_at = chrono::Utc::now().to_rfc3339();
    let mut postflight = begin_postflight(root, &plan)?;
    let candidates = execute_actions(root, &run_dir, &config, &plan)?;
    let review = review_batch(root, &config, &plan, &candidates)?;
    let selected = promote_winners(root, &plan, &candidates, &review)?;
    finish_postflight(root, &plan, &mut postflight)?;
    let evidence = DirectEvidence {
        schema_version: "brandi-direct-evidence-v1".into(),
        plan: plan.clone(),
        started_at,
        completed_at: chrono::Utc::now().to_rfc3339(),
        candidates,
        review,
        selected: selected.clone(),
        artifacts: artifacts.clone(),
        postflight,
    };
    let evidence_path = run_dir.join("evidence.json");
    fs::write(&evidence_path, serde_json::to_vec_pretty(&evidence)?)?;
    Ok(DirectOutcome {
        plan_id: plan.plan_id.clone(),
        preflight_only: false,
        section: plan.section.clone(),
        section_value: plan.section_value.clone(),
        palette: plan.palette.clone(),
        strategy: plan.strategy,
        actions: plan.actions.len(),
        estimated_cost_usd: plan.estimated_cost_usd,
        routes: plan
            .actions
            .iter()
            .map(|item| format!("{}/{}", item.provider, item.model))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect(),
        evidence: evidence_path,
        preflight: artifacts,
        generated: evidence
            .candidates
            .iter()
            .filter(|item| item.status == "generated")
            .count(),
        selected,
        postflight_confirmation: evidence.plan.postflight.confirmation,
    })
}

pub fn run_scoped(
    path: Option<&Path>,
    portfolio: bool,
    options: &DirectOptions,
) -> Result<DirectScopeOutcome> {
    let root = crate::creative::resolve_root(path, portfolio)?;
    let projects = if portfolio {
        crate::creative::discover_projects(&root)?
    } else {
        vec![root.clone()]
    };
    if projects.is_empty() {
        return Err(BrandiError::NotFound(format!(
            "no Brandi-compliant projects found beneath {}",
            root.display()
        )));
    }
    let projects = projects
        .into_iter()
        .map(|project| match run(&project, options) {
            Ok(outcome) => ProjectDirectOutcome {
                project,
                outcome: Some(outcome),
                error: None,
            },
            Err(error) => ProjectDirectOutcome {
                project,
                outcome: None,
                error: Some(error.to_string()),
            },
        })
        .collect();
    Ok(DirectScopeOutcome {
        schema_version: "brandi-direct-scope-v1".into(),
        portfolio,
        root,
        projects,
    })
}

fn validate_config(config: &DirectConfig) -> Result<()> {
    if config.runs == 0 || config.runs > MAX_RUNS {
        return Err(BrandiError::Invalid(format!(
            "direct runs must be between 1 and {MAX_RUNS}"
        )));
    }
    if config.max_concurrency == 0 || config.max_concurrency > MAX_CONCURRENCY {
        return Err(BrandiError::Invalid(format!(
            "direct max_concurrency must be between 1 and {MAX_CONCURRENCY}"
        )));
    }
    if config.providers.is_empty() || config.objectives.is_empty() {
        return Err(BrandiError::Invalid(
            "direct requires at least one provider and objective".into(),
        ));
    }
    let mut ids = std::collections::HashSet::new();
    for provider in &config.providers {
        if provider.id.trim().is_empty() || !ids.insert(&provider.id) || provider.models.is_empty()
        {
            return Err(BrandiError::Invalid(
                "direct provider ids must be non-empty and unique, with at least one model".into(),
            ));
        }
        if provider.kind == ProviderKind::Command && provider.command.is_empty() {
            return Err(BrandiError::Invalid(format!(
                "command provider '{}' has no command",
                provider.id
            )));
        }
    }
    for objective in &config.objectives {
        if objective.id.trim().is_empty() || objective.width == 0 || objective.height == 0 {
            return Err(BrandiError::Invalid(
                "direct objectives require an id and non-zero dimensions".into(),
            ));
        }
        confined(rootless(&objective.output), "objective output")?;
    }
    Ok(())
}

fn rootless(path: &Path) -> &Path {
    path
}

fn select_section(value: &serde_json::Value, section: &str) -> Result<serde_json::Value> {
    let pointer = if section.is_empty() || section == "all" {
        return Ok(value.clone());
    } else {
        format!("/{}", section.trim_matches('/').replace('.', "/"))
    };
    value.pointer(&pointer).cloned().ok_or_else(|| BrandiError::Invalid(format!(
        "brief section '{section}' does not exist; use all, identity, identity.product, audience, or a valid JSON path"
    )))
}

fn collect_sources(root: &Path, explicit: &[PathBuf]) -> Result<Vec<SourceEvidence>> {
    let paths = if explicit.is_empty() {
        assets::detect_assets(root)?
            .into_iter()
            .map(|item| PathBuf::from(item.path))
            .take(MAX_SOURCE_IMAGES)
            .collect()
    } else {
        if explicit.len() > MAX_SOURCE_IMAGES {
            return Err(BrandiError::Invalid(format!(
                "direct accepts at most {MAX_SOURCE_IMAGES} source images"
            )));
        }
        explicit.to_vec()
    };
    let mut out = Vec::new();
    for relative in paths {
        confined(&relative, "source image")?;
        let path = root.join(&relative);
        let metadata = fs::metadata(&path)
            .map_err(|_| BrandiError::NotFound(format!("source image {}", path.display())))?;
        if !metadata.is_file() || metadata.len() > MAX_ARTIFACT_BYTES as u64 {
            return Err(BrandiError::Invalid(format!(
                "source image {} is not a bounded regular file",
                path.display()
            )));
        }
        let (width, height) = image::image_dimensions(&path).unwrap_or((0, 0));
        out.push(SourceEvidence {
            path: relative,
            sha256: digest_file(&path)?,
            bytes: metadata.len(),
            width,
            height,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn collect_palette(guidelines: &Guidelines) -> Vec<ColorEvidence> {
    std::iter::once(&guidelines.visual.palette.primary)
        .chain(guidelines.visual.palette.secondary.iter())
        .chain(guidelines.visual.palette.neutrals.iter())
        .filter_map(|hex| {
            parse_hex_color(hex).map(|rgb| ColorEvidence {
                hex: format!("#{:02X}{:02X}{:02X}", rgb.0, rgb.1, rgb.2),
                srgb: [rgb.0, rgb.1, rgb.2],
                relative_luminance: luminance(rgb),
            })
        })
        .collect()
}

fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
    let linear = |value: u8| {
        let value = f64::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

fn resolve_routes(
    config: &DirectConfig,
    requested: &[String],
) -> Result<Vec<(ProviderConfig, String)>> {
    let mut routes = Vec::new();
    if requested.is_empty() {
        for provider in &config.providers {
            for model in &provider.models {
                routes.push((provider.clone(), model.clone()));
            }
        }
    } else {
        for route in requested {
            let (provider_id, model) = route.split_once('/').ok_or_else(|| {
                BrandiError::Invalid(format!("route '{route}' must be provider/model"))
            })?;
            let provider = config
                .providers
                .iter()
                .find(|item| item.id == provider_id)
                .ok_or_else(|| {
                    BrandiError::Invalid(format!("unknown direct provider '{provider_id}'"))
                })?;
            if !provider.models.iter().any(|candidate| candidate == model) {
                return Err(BrandiError::Invalid(format!(
                    "model '{model}' is not configured for provider '{provider_id}'"
                )));
            }
            routes.push((provider.clone(), model.into()));
        }
    }
    Ok(routes)
}

fn system_prompt(brief: &Brief, guidelines: &Guidelines, section: &str) -> String {
    format!(
        "You are Brandi's senior identity designer. Generate production-ready visual assets grounded only in the supplied brief section and guidelines. Preserve the exact product name '{}'. Use only the supplied sRGB palette, honor '{}' typography, maintain at least {:.0}% intentional whitespace, avoid prohibited language and generic stock motifs, and do not invent claims. Each result must remain legible at thumbnail size and be suitable for independent critical review. Brief section: {section}.",
        brief.identity.product.name,
        guidelines.visual.typography.style,
        guidelines.visual.assets.min_whitespace * 100.0,
    )
}

fn project_actions(
    config: &DirectConfig,
    routes: &[(ProviderConfig, String)],
    system: &str,
    section: &serde_json::Value,
) -> Vec<GenerationAction> {
    let mut actions = Vec::new();
    for objective in &config.objectives {
        let mut previous = None;
        for sequence in 0..config.runs {
            let (provider, model) = &routes[sequence % routes.len()];
            let id = format!("{}-{:03}", objective.id, sequence + 1);
            let parent_action =
                matches!(config.strategy, Strategy::Sequential | Strategy::Iterative)
                    .then(|| previous.clone())
                    .flatten();
            let phase = match config.strategy {
                Strategy::Parallel => "Create an independent option",
                Strategy::Sequential => "Broaden the previous direction without copying it",
                Strategy::Iterative => "Improve the prior option against critical feedback",
            };
            actions.push(GenerationAction {
                id: id.clone(),
                objective: objective.id.clone(),
                provider: provider.id.clone(),
                model: model.clone(),
                sequence,
                parent_action,
                estimated_cost_usd: provider.cost_per_run_usd.unwrap_or(0.0),
                prompt: format!(
                    "{system}\n\nObjective: {} ({}×{}). {phase}. Brief evidence: {}",
                    objective.description, objective.width, objective.height, section
                ),
            });
            previous = Some(id);
        }
    }
    actions
}

fn resolve_reviewer(config: &DirectConfig, requested: Option<&str>) -> Result<ReviewerRoute> {
    let reviewer = config.reviewer.clone().unwrap_or(ReviewerConfig {
        id: "deterministic".into(),
        model: "asset-constraints-v1".into(),
        ..ReviewerConfig::default()
    });
    if let Some(route) = requested {
        let (id, model) = route
            .split_once('/')
            .ok_or_else(|| BrandiError::Invalid("reviewer must be provider/model".into()))?;
        if id != reviewer.id || model != reviewer.model {
            return Err(BrandiError::Invalid(format!(
                "reviewer '{route}' is not configured"
            )));
        }
    }
    Ok(ReviewerRoute {
        provider: reviewer.id,
        model: reviewer.model,
        rubric: vec![
            "brief fidelity".into(),
            "guideline compliance".into(),
            "palette accuracy".into(),
            "legibility".into(),
            "distinctiveness".into(),
            "technical fitness".into(),
        ],
    })
}

fn write_preflight(run_dir: &Path, plan: &DirectPlan) -> Result<PreflightArtifacts> {
    let json = run_dir.join("preflight.json");
    let svg = run_dir.join("preflight.svg");
    let rdf = run_dir.join("preflight.rdf");
    fs::write(&json, serde_json::to_vec_pretty(plan)?)?;
    fs::write(&svg, preflight_svg(plan))?;
    fs::write(&rdf, preflight_rdf(plan))?;
    Ok(PreflightArtifacts { json, svg, rdf })
}

fn preflight_svg(plan: &DirectPlan) -> String {
    let width = 1200;
    let source_rows = plan.sources.len().max(1);
    let height = 640 + source_rows * 52;
    let mut out = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" color-interpolation="sRGB">
<rect width="100%" height="100%" fill="#FFFFFF"/><style>text{{font-family:ui-monospace,monospace;fill:#111}}.h{{font-size:28px;font-weight:700}}.s{{font-size:17px}}.m{{font-size:14px;fill:#444}}</style>
<text x="40" y="52" class="h">Brandi direct pre-flight</text>
<text x="40" y="86" class="s">Plan {}</text>
<text x="40" y="116" class="s">Section: {}</text>
<text x="40" y="146" class="s">Strategy: {:?} · {} actions · concurrency {} · estimated ${:.4}</text>
<text x="40" y="184" class="s">Exact sRGB palette</text>
"##,
        xml(&plan.plan_id),
        xml(&plan.section),
        plan.strategy,
        plan.actions.len(),
        plan.max_concurrency,
        plan.estimated_cost_usd
    );
    for (index, color) in plan.palette.iter().enumerate() {
        let x = 40 + (index % 8) * 140;
        let y = 205 + (index / 8) * 90;
        let foreground = if color.relative_luminance > 0.45 {
            "#000000"
        } else {
            "#FFFFFF"
        };
        out.push_str(&format!(r##"<rect x="{x}" y="{y}" width="124" height="58" fill="{}" stroke="#222"/><text x="{}" y="{}" font-size="13" fill="{foreground}" style="fill:{foreground}">{}</text>
"##, color.hex, x + 10, y + 34, color.hex));
    }
    let objective_y = 315;
    out.push_str(&format!(
        r#"<text x="40" y="{objective_y}" class="s">Generation budget</text>
"#
    ));
    for (index, objective) in plan.objectives.iter().enumerate() {
        out.push_str(&format!(
            r#"<text x="40" y="{}" class="m">{} · {}×{} · output {}</text>
"#,
            objective_y + 30 + index * 24,
            xml(&objective.id),
            objective.width,
            objective.height,
            xml(&objective.output.display().to_string())
        ));
    }
    let sources_y = objective_y + 65 + plan.objectives.len() * 24;
    out.push_str(&format!(
        r#"<text x="40" y="{sources_y}" class="s">Source-image evidence</text>
"#
    ));
    if plan.sources.is_empty() {
        out.push_str(&format!(
            r#"<text x="40" y="{}" class="m">No source images discovered or supplied.</text>
"#,
            sources_y + 30
        ));
    }
    for (index, source) in plan.sources.iter().enumerate() {
        let y = sources_y + 25 + index * 52;
        out.push_str(&format!(r#"<image x="40" y="{y}" width="42" height="42" preserveAspectRatio="xMidYMid meet" href="{}"/><text x="98" y="{}" class="m">{} · {}×{} · {}</text>
"#, xml(&source.path.display().to_string()), y + 26, xml(&source.path.display().to_string()), source.width, source.height, &source.sha256[..23]));
    }
    out.push_str("</svg>\n");
    out
}

fn preflight_rdf(plan: &DirectPlan) -> String {
    let mut out = format!("#! /usr/bin/env rugid\n# Brandi/RUGID pre-flight v1\nwindow:::title[Brandi direct {}]::sdims[100,100]\n\tpane:::id[header]::parent[Brandi direct {}]::origin[0,0]::extent[1,0.12]::stack[p]\n\t\ttext:::parent[header]::value[Direct pre-flight {}]::size[0.25]::color[#111111]\n\tpane:::id[palette]::parent[Brandi direct {}]::origin[0,0.12]::extent[1,0.25]::stack[s]\n", plan.plan_id, plan.plan_id, plan.plan_id, plan.plan_id);
    for (index, color) in plan.palette.iter().enumerate() {
        out.push_str(&format!(
            "\t\twidget:::id[swatch_{index}]::parent[palette]::extent[flex,1]::fill[{}]\n",
            color.hex
        ));
    }
    out.push_str(&format!("\tpane:::id[evidence]::parent[Brandi direct {}]::origin[0,0.37]::extent[1,0.63]::stack[p]\n", plan.plan_id));
    for (index, source) in plan.sources.iter().enumerate() {
        out.push_str(&format!("\t\twidget:::id[source_{index}]::parent[evidence]::extent[1,flex]\n\t\t\ttext:::parent[source_{index}]::value[{} · {}]::size[0.18]::color[#111111]\n", rdf_text(&source.path.display().to_string()), &source.sha256[..23]));
    }
    out
}

fn execute_actions(
    root: &Path,
    run_dir: &Path,
    config: &DirectConfig,
    plan: &DirectPlan,
) -> Result<Vec<CandidateEvidence>> {
    let mut pending = VecDeque::from(plan.actions.clone());
    let mut completed = Vec::new();
    // RUGID-style budgeted scheduler. Independent actions are chunked up to
    // max_concurrency; dependent actions remain ordered by their parent edge.
    while !pending.is_empty() {
        let mut batch = Vec::new();
        let mut deferred = VecDeque::new();
        while let Some(action) = pending.pop_front() {
            let ready = action.parent_action.as_ref().is_none_or(|parent| {
                completed
                    .iter()
                    .any(|item: &CandidateEvidence| &item.action_id == parent)
            });
            if ready && batch.len() < plan.max_concurrency {
                batch.push(action);
            } else {
                deferred.push_back(action);
            }
        }
        if batch.is_empty() {
            return Err(BrandiError::Invalid(
                "direct action graph contains an unresolved dependency".into(),
            ));
        }
        let results = std::thread::scope(|scope| {
            let handles = batch
                .into_iter()
                .map(|action| {
                    scope.spawn(move || execute_one(root, run_dir, config, plan, &action))
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| {
                    handle.join().unwrap_or_else(|_| CandidateEvidence {
                        action_id: "worker-panic".into(),
                        objective: String::new(),
                        provider: String::new(),
                        model: String::new(),
                        path: None,
                        sha256: None,
                        bytes: None,
                        duration_ms: 0,
                        status: "failed".into(),
                        error: Some("provider worker panicked".into()),
                    })
                })
                .collect::<Vec<_>>()
        });
        completed.extend(results);
        pending = deferred;
    }
    completed.sort_by(|a, b| a.action_id.cmp(&b.action_id));
    Ok(completed)
}

fn execute_one(
    root: &Path,
    run_dir: &Path,
    config: &DirectConfig,
    plan: &DirectPlan,
    action: &GenerationAction,
) -> CandidateEvidence {
    let started = Instant::now();
    let provider = config
        .providers
        .iter()
        .find(|item| item.id == action.provider);
    let result = provider
        .ok_or_else(|| {
            BrandiError::Invalid(format!(
                "provider '{}' disappeared after planning",
                action.provider
            ))
        })
        .and_then(|provider| {
            let objective = plan
                .objectives
                .iter()
                .find(|item| item.id == action.objective)
                .ok_or_else(|| {
                    BrandiError::Invalid("objective disappeared after planning".into())
                })?;
            let extension = match provider.kind {
                ProviderKind::Vasilis => "svg",
                _ => "png",
            };
            let output = run_dir
                .join("candidates")
                .join(format!("{}.{}", action.id, extension));
            match provider.kind {
                ProviderKind::Vasilis => execute_vasilis(root, plan, action, objective, &output),
                ProviderKind::OpenaiImages => {
                    execute_openai_images(provider, action, objective, &output)
                }
                ProviderKind::Command => {
                    execute_command_provider(provider, plan, action, objective, &output)
                }
            }?;
            let metadata = fs::metadata(&output)?;
            if !metadata.is_file()
                || metadata.len() == 0
                || metadata.len() > MAX_ARTIFACT_BYTES as u64
            {
                return Err(BrandiError::Invalid(
                    "provider produced an empty or oversized artifact".into(),
                ));
            }
            Ok((output, metadata.len()))
        });
    match result {
        Ok((path, bytes)) => CandidateEvidence {
            action_id: action.id.clone(),
            objective: action.objective.clone(),
            provider: action.provider.clone(),
            model: action.model.clone(),
            sha256: digest_file(&path).ok(),
            path: Some(path),
            bytes: Some(bytes),
            duration_ms: started.elapsed().as_millis(),
            status: "generated".into(),
            error: None,
        },
        Err(error) => CandidateEvidence {
            action_id: action.id.clone(),
            objective: action.objective.clone(),
            provider: action.provider.clone(),
            model: action.model.clone(),
            path: None,
            sha256: None,
            bytes: None,
            duration_ms: started.elapsed().as_millis(),
            status: "failed".into(),
            error: Some(error.to_string()),
        },
    }
}

/// Build the VASILIS `VisualRequirement` JSON for one generation action.
///
/// `visual_language` and `forbidden_motifs` come from the plan's guidelines
/// snapshot (`.brandi/visual.yaml` / `.brandi/prohibited.yaml`), not from
/// hardcoded literals. Every axis is marked `"observed"`: the value was read
/// from a config file a human authored, which is VASILIS's "detected/read
/// from an authoritative existing source" sense of the word — not a live
/// request nor an inference.
fn vasilis_requirement(
    plan: &DirectPlan,
    action: &GenerationAction,
    objective: &ObjectiveConfig,
) -> serde_json::Value {
    let sourced = |value: &str| serde_json::json!({"value": value, "source": "observed"});
    let visual_language = &plan.visual_language;
    serde_json::json!({
        "id": action.id, "revision": action.sequence + 1,
        "namespace": {"tenant":"local","project":plan.plan_id,"application":"brandi-direct"},
        "source":"brandi", "semantic_role":objective.description,
        "asset_type": if objective.id.contains("logo") { "logo" } else { "illustration" },
        "surface":{"name":objective.id,"width":objective.width,"height":objective.height,"theme":"any"},
        "purpose":action.prompt,
        "constraints":{"required_format":"svg","max_bytes":MAX_ARTIFACT_BYTES,"max_pixels":u64::from(objective.width)*u64::from(objective.height),"transparent_background":false},
        "priority":"normal",
        "brand_context":{
            "id":format!("{}-brand",plan.plan_id),
            "revision":1,
            "palette":plan.palette.iter().map(|item| &item.hex).collect::<Vec<_>>(),
            "visual_language":{
                "geometry": sourced(&visual_language.geometry),
                "complexity": sourced(&visual_language.complexity),
                "density": sourced(&visual_language.density),
                "contrast": sourced(&visual_language.contrast),
                "corner_language": sourced(&visual_language.corner_language),
                "lighting": sourced(&visual_language.lighting),
                "depth": sourced(&visual_language.depth),
                "texture": sourced(&visual_language.texture),
                "illustration_style": sourced(&visual_language.illustration_style)
            },
            "forbidden_motifs": plan.forbidden_motifs
        },
        "required_variants":["master"], "evidence":{"brief_section":plan.section,"sources":plan.sources}
    })
}

fn execute_vasilis(
    root: &Path,
    plan: &DirectPlan,
    action: &GenerationAction,
    objective: &ObjectiveConfig,
    output: &Path,
) -> Result<()> {
    let requirement = vasilis_requirement(plan, action, objective);
    let request = output.with_extension("request.json");
    fs::write(&request, serde_json::to_vec_pretty(&requirement)?)?;
    let result = run_bounded(
        Command::new("vasilis")
            .arg("generate")
            .arg("--requirement")
            .arg(&request)
            .arg("--output")
            .arg(output)
            .current_dir(root),
        PROVIDER_TIMEOUT,
        MAX_SUBPROCESS_OUTPUT_BYTES,
    )
    .map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            BrandiError::NotFound("vasilis provider is not installed".into())
        } else {
            BrandiError::Io(error)
        }
    })?;
    if result.timed_out || result.truncated || !result.status.success() {
        return Err(BrandiError::Invalid(format!(
            "Vasilis generation failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        )));
    }
    Ok(())
}

fn execute_openai_images(
    provider: &ProviderConfig,
    action: &GenerationAction,
    objective: &ObjectiveConfig,
    output: &Path,
) -> Result<()> {
    let key_env = if provider.api_key_env.is_empty() {
        "OPENAI_API_KEY"
    } else {
        &provider.api_key_env
    };
    let api_key = std::env::var(key_env).map_err(|_| {
        BrandiError::NotFound(format!(
            "provider '{}' requires environment variable {key_env}",
            provider.id
        ))
    })?;
    let endpoint = format!(
        "{}/images/generations",
        provider.base_url.trim_end_matches('/')
    );
    let size = supported_openai_size(objective.width, objective.height);
    let body = serde_json::json!({"model":action.model,"prompt":action.prompt,"n":1,"size":size,"response_format":"b64_json"});
    let config = curl_json_config(&endpoint, &api_key, &body);
    let result = run_bounded_with_input(
        Command::new("curl").args([
            "-sS",
            "--fail-with-body",
            "--connect-timeout",
            "10",
            "--max-time",
            "180",
            "--max-filesize",
            "67108864",
            "-X",
            "POST",
            "-K",
            "-",
        ]),
        Some(config.as_bytes()),
        PROVIDER_TIMEOUT,
        MAX_ARTIFACT_BYTES * 2,
    )?;
    if result.timed_out || result.truncated || !result.status.success() {
        return Err(BrandiError::Network(format!(
            "provider '{}' failed: {}",
            provider.id,
            String::from_utf8_lossy(&result.stderr).trim()
        )));
    }
    let response: serde_json::Value = serde_json::from_slice(&result.stdout)?;
    let encoded = response
        .pointer("/data/0/b64_json")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            BrandiError::Network(format!("provider '{}' returned no image", provider.id))
        })?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| {
            BrandiError::Invalid(format!("provider image was not valid base64: {error}"))
        })?;
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(BrandiError::Invalid(
            "provider image exceeded the artifact budget".into(),
        ));
    }
    fs::write(output, bytes)?;
    Ok(())
}

fn supported_openai_size(width: u32, height: u32) -> &'static str {
    if width > height {
        "1536x1024"
    } else if height > width {
        "1024x1536"
    } else {
        "1024x1024"
    }
}

fn execute_command_provider(
    provider: &ProviderConfig,
    plan: &DirectPlan,
    action: &GenerationAction,
    objective: &ObjectiveConfig,
    output: &Path,
) -> Result<()> {
    let (program, args) = provider
        .command
        .split_first()
        .ok_or_else(|| BrandiError::Invalid("command provider is empty".into()))?;
    if program.contains(['/', '\\']) {
        return Err(BrandiError::Invalid(
            "command provider program must resolve from PATH".into(),
        ));
    }
    let request = serde_json::json!({"schema_version":"brandi-provider-request-v1","action":action,"objective":objective,"brief_section":plan.section_value,"palette":plan.palette,"sources":plan.sources,"output":output});
    let result = run_bounded_with_input(
        Command::new(program).args(args),
        Some(&serde_json::to_vec(&request)?),
        PROVIDER_TIMEOUT,
        MAX_ARTIFACT_BYTES * 2,
    )
    .map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            BrandiError::NotFound(format!("provider adapter '{program}' is not installed"))
        } else {
            BrandiError::Io(error)
        }
    })?;
    if result.timed_out || result.truncated || !result.status.success() {
        return Err(BrandiError::Invalid(format!(
            "provider adapter '{}' failed: {}",
            provider.id,
            String::from_utf8_lossy(&result.stderr).trim()
        )));
    }
    let response: serde_json::Value = serde_json::from_slice(&result.stdout)?;
    if let Some(encoded) = response
        .get("image_base64")
        .and_then(|value| value.as_str())
    {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| {
                BrandiError::Invalid(format!("adapter returned invalid base64: {error}"))
            })?;
        fs::write(output, bytes)?;
    }
    if !output.is_file() {
        return Err(BrandiError::Invalid(format!(
            "provider adapter '{}' did not create the requested output",
            provider.id
        )));
    }
    Ok(())
}

fn review_batch(
    root: &Path,
    config: &DirectConfig,
    plan: &DirectPlan,
    candidates: &[CandidateEvidence],
) -> Result<BatchReview> {
    let reviewer = config.reviewer.as_ref();
    match reviewer
        .map(|item| item.kind)
        .unwrap_or(ReviewerKind::Deterministic)
    {
        ReviewerKind::OpenaiResponses => {
            review_openai(root, reviewer.expect("reviewer exists"), plan, candidates).or_else(
                |error| {
                    let mut fallback = deterministic_review(plan, candidates);
                    fallback.limitations.push(format!(
                        "vision reviewer unavailable; deterministic fallback used: {error}"
                    ));
                    Ok(fallback)
                },
            )
        }
        ReviewerKind::Deterministic => Ok(deterministic_review(plan, candidates)),
    }
}

fn deterministic_review(plan: &DirectPlan, candidates: &[CandidateEvidence]) -> BatchReview {
    let mut assessments = candidates
        .iter()
        .filter(|candidate| candidate.status == "generated")
        .map(|candidate| {
            let technical = candidate
                .path
                .as_ref()
                .and_then(|path| image::image_dimensions(path).ok())
                .map(|(width, height)| if width > 0 && height > 0 { 1.0 } else { 0.5 })
                .unwrap_or_else(|| {
                    if candidate.path.as_ref().and_then(|path| path.extension())
                        == Some(OsStr::new("svg"))
                    {
                        0.8
                    } else {
                        0.2
                    }
                });
            let evidence = if candidate.sha256.is_some() { 1.0 } else { 0.0 };
            let quality = 45.0 + technical * 30.0 + evidence * 10.0;
            let confidence = (0.35
                + technical * 0.2
                + evidence * 0.15
                + if plan.sources.is_empty() {
                    0.0
                } else {
                    0.1_f64
                })
            .min(0.8);
            AssetAssessment {
                candidate_id: candidate.action_id.clone(),
                rank: 0,
                quality_score: quality,
                confidence,
                constraint_scores: BTreeMap::from([
                    ("technical_fitness".into(), technical * 100.0),
                    ("evidence_completeness".into(), evidence * 100.0),
                ]),
                strengths: vec!["bounded artifact with content digest".into()],
                risks: vec![
                    "semantic and aesthetic claims require a configured vision reviewer".into(),
                ],
                recommendation: "eligible for model review".into(),
            }
        })
        .collect::<Vec<_>>();
    assessments.sort_by(|a, b| {
        b.quality_score
            .total_cmp(&a.quality_score)
            .then_with(|| a.candidate_id.cmp(&b.candidate_id))
    });
    for (index, item) in assessments.iter_mut().enumerate() {
        item.rank = index + 1;
    }
    BatchReview {
        provider: "deterministic".into(),
        model: "asset-constraints-v1".into(),
        reviewer_confidence: assessments.iter().map(|item| item.confidence).sum::<f64>()
            / assessments.len().max(1) as f64,
        assessments,
        limitations: vec![
            "No image-to-text model was configured; scores cover technical evidence only.".into(),
        ],
    }
}

fn review_openai(
    root: &Path,
    reviewer: &ReviewerConfig,
    plan: &DirectPlan,
    candidates: &[CandidateEvidence],
) -> Result<BatchReview> {
    let key_env = if reviewer.api_key_env.is_empty() {
        "OPENAI_API_KEY"
    } else {
        &reviewer.api_key_env
    };
    let api_key = std::env::var(key_env).map_err(|_| {
        BrandiError::NotFound(format!("reviewer requires environment variable {key_env}"))
    })?;
    let mut content = vec![
        serde_json::json!({"type":"input_text","text":format!("You are an independent brand asset critic. Analyze every image as one batch, compare options for the same objective, and return strict JSON. Distinguish quality_score from confidence. Penalize invented claims. Plan and rubric: {}", serde_json::to_string(plan)?)}),
    ];
    for candidate in candidates.iter().filter(|item| item.status == "generated") {
        let Some(path) = &candidate.path else {
            continue;
        };
        let bytes = fs::read(path)?;
        let mime = if path.extension() == Some(OsStr::new("svg")) {
            "image/svg+xml"
        } else {
            "image/png"
        };
        content.push(serde_json::json!({"type":"input_text","text":format!("candidate_id={}",candidate.action_id)}));
        content.push(serde_json::json!({"type":"input_image","image_url":format!("data:{mime};base64,{}",base64::engine::general_purpose::STANDARD.encode(bytes))}));
    }
    let body = serde_json::json!({"model":reviewer.model,"input":[{"role":"user","content":content}],"text":{"format":{"type":"json_schema","name":"brand_asset_review","strict":true,"schema":review_schema()}}});
    let endpoint = format!("{}/responses", reviewer.base_url.trim_end_matches('/'));
    let config = curl_json_config(&endpoint, &api_key, &body);
    let result = run_bounded_with_input(
        Command::new("curl")
            .args([
                "-sS",
                "--fail-with-body",
                "--connect-timeout",
                "10",
                "--max-time",
                "180",
                "--max-filesize",
                "8388608",
                "-X",
                "POST",
                "-K",
                "-",
            ])
            .current_dir(root),
        Some(config.as_bytes()),
        PROVIDER_TIMEOUT,
        8 * 1024 * 1024,
    )?;
    if result.timed_out || result.truncated || !result.status.success() {
        return Err(BrandiError::Network(
            String::from_utf8_lossy(&result.stderr).trim().into(),
        ));
    }
    let response: serde_json::Value = serde_json::from_slice(&result.stdout)?;
    let text = response
        .get("output_text")
        .and_then(|value| value.as_str())
        .or_else(|| {
            response.get("output")?.as_array()?.iter().find_map(|item| {
                item.get("content")?
                    .as_array()?
                    .iter()
                    .find_map(|part| part.get("text")?.as_str())
            })
        })
        .ok_or_else(|| BrandiError::Network("reviewer returned no structured text".into()))?;
    let mut review: BatchReview = serde_json::from_str(text)?;
    review.provider = reviewer.id.clone();
    review.model = reviewer.model.clone();
    validate_review(candidates, &review)?;
    Ok(review)
}

fn review_schema() -> serde_json::Value {
    serde_json::json!({"type":"object","additionalProperties":false,"required":["provider","model","assessments","reviewer_confidence","limitations"],"properties":{"provider":{"type":"string"},"model":{"type":"string"},"reviewer_confidence":{"type":"number","minimum":0,"maximum":1},"limitations":{"type":"array","items":{"type":"string"}},"assessments":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["candidate_id","rank","quality_score","confidence","constraint_scores","strengths","risks","recommendation"],"properties":{"candidate_id":{"type":"string"},"rank":{"type":"integer","minimum":1},"quality_score":{"type":"number","minimum":0,"maximum":100},"confidence":{"type":"number","minimum":0,"maximum":1},"constraint_scores":{"type":"object","additionalProperties":{"type":"number","minimum":0,"maximum":100}},"strengths":{"type":"array","items":{"type":"string"}},"risks":{"type":"array","items":{"type":"string"}},"recommendation":{"type":"string"}}}}}})
}

fn validate_review(candidates: &[CandidateEvidence], review: &BatchReview) -> Result<()> {
    let generated = candidates
        .iter()
        .filter(|item| item.status == "generated")
        .map(|item| item.action_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    if review.assessments.len() != generated.len()
        || review.assessments.iter().any(|item| {
            !generated.contains(item.candidate_id.as_str())
                || !(0.0..=100.0).contains(&item.quality_score)
                || !(0.0..=1.0).contains(&item.confidence)
        })
    {
        return Err(BrandiError::Invalid(
            "reviewer did not assess every generated candidate exactly once with bounded scores"
                .into(),
        ));
    }
    let assessed = review
        .assessments
        .iter()
        .map(|item| item.candidate_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    if assessed.len() != generated.len() {
        return Err(BrandiError::Invalid(
            "reviewer returned duplicate candidate assessments".into(),
        ));
    }
    Ok(())
}

fn promote_winners(
    root: &Path,
    plan: &DirectPlan,
    candidates: &[CandidateEvidence],
    review: &BatchReview,
) -> Result<BTreeMap<String, String>> {
    let mut selected = BTreeMap::new();
    for objective in &plan.objectives {
        let winner = review
            .assessments
            .iter()
            .filter_map(|assessment| {
                let candidate = candidates.iter().find(|item| {
                    item.action_id == assessment.candidate_id
                        && item.objective == objective.id
                        && item.status == "generated"
                })?;
                Some((assessment, candidate))
            })
            .max_by(|(left, _), (right, _)| {
                left.quality_score
                    .total_cmp(&right.quality_score)
                    .then_with(|| right.candidate_id.cmp(&left.candidate_id))
            });
        let Some((assessment, candidate)) = winner else {
            continue;
        };
        let source = candidate
            .path
            .as_ref()
            .expect("generated candidate has path");
        let mut relative = objective.output.clone();
        if source.extension() != relative.extension() {
            relative.set_extension(source.extension().unwrap_or_else(|| OsStr::new("bin")));
        }
        confined(&relative, "selected output")?;
        let destination = root.join(&relative);
        if destination.exists() {
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, &destination)?;
        selected.insert(
            objective.id.clone(),
            format!(
                "{} (score {:.1}, confidence {:.2})",
                relative.display(),
                assessment.quality_score,
                assessment.confidence
            ),
        );
    }
    Ok(selected)
}

fn require_postflight_confirmation(plan: &DirectPlan, supplied: Option<&str>) -> Result<()> {
    if let Some(required) = &plan.postflight.confirmation {
        if supplied != Some(required.as_str()) {
            return Err(BrandiError::Invalid(format!("post-flight mode {:?} requires --confirm-postflight {required}; run --preflight-only first and review the evidence", plan.postflight.mode)));
        }
    }
    Ok(())
}

fn begin_postflight(root: &Path, plan: &DirectPlan) -> Result<PostflightEvidence> {
    let mut evidence = PostflightEvidence {
        requested: plan.postflight.mode,
        ..PostflightEvidence::default()
    };
    if matches!(
        plan.postflight.mode,
        KaptaindMode::Aoc | KaptaindMode::AocPush
    ) {
        let result = run_bounded(
            Command::new("kaptaind-cli").args([
                "-r",
                root.to_string_lossy().as_ref(),
                "aoc",
                "start",
                &format!("brandi direct {}", plan.plan_id),
            ]),
            Duration::from_secs(15),
            MAX_SUBPROCESS_OUTPUT_BYTES,
        )?;
        if !result.status.success() {
            return Err(BrandiError::Invalid(format!(
                "Kaptaind AoC could not start: {}",
                String::from_utf8_lossy(&result.stderr).trim()
            )));
        }
        evidence.aoc_started = true;
        evidence
            .messages
            .push("Kaptaind Aim of Change started before asset mutation".into());
    }
    Ok(evidence)
}

fn finish_postflight(
    root: &Path,
    plan: &DirectPlan,
    evidence: &mut PostflightEvidence,
) -> Result<()> {
    if !matches!(plan.postflight.mode, KaptaindMode::None) {
        let result = run_bounded(
            Command::new("kaptaind-cli").args(["-r", root.to_string_lossy().as_ref(), "analyze"]),
            Duration::from_secs(60),
            MAX_SUBPROCESS_OUTPUT_BYTES,
        )?;
        evidence.analyzed = result.status.success();
        evidence
            .messages
            .push(String::from_utf8_lossy(&result.stdout).trim().to_string());
        if !result.status.success() {
            return Err(BrandiError::Invalid(
                "Kaptaind analysis failed; post-flight stopped before ship/push".into(),
            ));
        }
    }
    if matches!(
        plan.postflight.mode,
        KaptaindMode::Aoc | KaptaindMode::AocPush
    ) {
        let result = run_bounded(
            Command::new("kaptaind-cli").args([
                "-r",
                root.to_string_lossy().as_ref(),
                "aoc",
                "ship",
            ]),
            Duration::from_secs(60),
            MAX_SUBPROCESS_OUTPUT_BYTES,
        )?;
        if !result.status.success() {
            return Err(BrandiError::Invalid(format!(
                "Kaptaind AoC ship failed: {}",
                String::from_utf8_lossy(&result.stderr).trim()
            )));
        }
        evidence.aoc_shipped = true;
        evidence
            .messages
            .push("Kaptaind Aim of Change shipped; daemon-owned commit policy applied".into());
    }
    if plan.postflight.mode == KaptaindMode::AocPush {
        let status = run_bounded(
            Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(root),
            Duration::from_secs(10),
            MAX_SUBPROCESS_OUTPUT_BYTES,
        )?;
        if !status.status.success() || !status.stdout.is_empty() {
            return Err(BrandiError::Invalid(
                "push refused: Kaptaind has not produced a clean committed worktree".into(),
            ));
        }
        let push = run_bounded(
            Command::new("git").arg("push").current_dir(root),
            Duration::from_secs(120),
            MAX_SUBPROCESS_OUTPUT_BYTES,
        )?;
        if !push.status.success() {
            return Err(BrandiError::Invalid(format!(
                "post-flight push failed: {}",
                String::from_utf8_lossy(&push.stderr).trim()
            )));
        }
        evidence.pushed = true;
    }
    Ok(())
}

fn confined(path: &Path, label: &str) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err(BrandiError::Invalid(format!(
            "{label} must stay within the project: {}",
            path.display()
        )));
    }
    Ok(())
}

fn digest_file(path: &Path) -> Result<String> {
    Ok(digest_bytes(&fs::read(path)?))
}
fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn rdf_text(value: &str) -> String {
    value.replace(['[', ']', '\n', '\r'], " ")
}
fn escape_curl(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
fn curl_json_config(endpoint: &str, api_key: &str, body: &serde_json::Value) -> String {
    format!("url = \"{}\"\nheader = \"Content-Type: application/json\"\nheader = \"Authorization: Bearer {}\"\ndata-binary = \"{}\"\n", escape_curl(endpoint), escape_curl(api_key), escape_curl(&body.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scaffold() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        Brief::scaffold(root.path()).unwrap();
        Guidelines::scaffold(root.path()).unwrap();
        root
    }

    fn options() -> DirectOptions {
        DirectOptions {
            section: "identity.product".into(),
            runs: Some(3),
            strategy: Some(Strategy::Sequential),
            routes: Vec::new(),
            reviewer: None,
            max_concurrency: Some(2),
            max_cost_usd: Some(0.0),
            sources: Vec::new(),
            preflight_only: true,
            kaptaind: KaptaindMode::None,
            confirm_postflight: None,
        }
    }

    #[test]
    fn planner_is_deterministic_and_encodes_dependencies() {
        let root = scaffold();
        let (_, first) = plan(root.path(), &options()).unwrap();
        let (_, second) = plan(root.path(), &options()).unwrap();
        assert_eq!(first.plan_id, second.plan_id);
        assert_eq!(first.actions.len(), 6);
        assert!(first
            .actions
            .iter()
            .filter(|item| item.sequence > 0)
            .all(|item| item.parent_action.is_some()));
    }

    #[test]
    fn preflight_writes_exact_srgb_and_rugid_scene() {
        let root = scaffold();
        let outcome = run(root.path(), &options()).unwrap();
        let svg = fs::read_to_string(outcome.preflight.svg).unwrap();
        let rdf = fs::read_to_string(outcome.preflight.rdf).unwrap();
        assert!(svg.contains("color-interpolation=\"sRGB\""));
        assert!(svg.contains("#FF2DAA"));
        assert!(rdf.contains("window:::title[Brandi direct"));
        assert_eq!(outcome.generated, 0);
    }

    #[test]
    fn vasilis_requirement_reflects_guideline_config_not_hardcoded_literals() {
        let first_root = scaffold();
        std::fs::write(
            first_root.path().join(".brandi/visual.yaml"),
            "visual_language:\n  geometry: organic\n  complexity: intricate\n  density: dense\n  contrast: low\n  corner_language: rounded\n  lighting: dramatic\n  depth: high\n  texture: tactile\n  illustration_style: hand-drawn\n",
        )
        .unwrap();
        std::fs::write(
            first_root.path().join(".brandi/prohibited.yaml"),
            "visual_motifs: [\"gradients\", \"photorealistic people\"]\n",
        )
        .unwrap();
        let (_, first_plan) = plan(first_root.path(), &options()).unwrap();
        let first_action = &first_plan.actions[0];
        let first_objective = first_plan
            .objectives
            .iter()
            .find(|item| item.id == first_action.objective)
            .unwrap();
        let first_requirement = vasilis_requirement(&first_plan, first_action, first_objective);

        let second_root = scaffold(); // default visual_language/visual_motifs
        let (_, second_plan) = plan(second_root.path(), &options()).unwrap();
        let second_action = &second_plan.actions[0];
        let second_objective = second_plan
            .objectives
            .iter()
            .find(|item| item.id == second_action.objective)
            .unwrap();
        let second_requirement = vasilis_requirement(&second_plan, second_action, second_objective);

        let first_language = &first_requirement["brand_context"]["visual_language"];
        assert_eq!(first_language["geometry"]["value"], "organic");
        assert_eq!(first_language["geometry"]["source"], "observed");
        assert_eq!(first_language["illustration_style"]["value"], "hand-drawn");
        assert_eq!(
            first_requirement["brand_context"]["forbidden_motifs"],
            serde_json::json!(["gradients", "photorealistic people"])
        );

        let second_language = &second_requirement["brand_context"]["visual_language"];
        assert_eq!(second_language["geometry"]["value"], "geometric");
        assert_eq!(
            second_requirement["brand_context"]["forbidden_motifs"],
            serde_json::json!([])
        );

        // The two configs must produce genuinely different requirement JSON:
        // this is the proof the values are no longer hardcoded literals.
        assert_ne!(
            first_requirement["brand_context"]["visual_language"],
            second_requirement["brand_context"]["visual_language"]
        );
        assert_ne!(
            first_requirement["brand_context"]["forbidden_motifs"],
            second_requirement["brand_context"]["forbidden_motifs"]
        );
    }

    #[test]
    fn budget_and_path_traversal_fail_closed() {
        let root = scaffold();
        let config = root.path().join(CONFIG_PATH);
        fs::write(&config, "runs: 1\nmax_concurrency: 1\nproviders:\n  - id: paid\n    kind: openai_images\n    base_url: https://example.invalid/v1\n    models: [image]\n    cost_per_run_usd: 2.0\nobjectives:\n  - id: logo\n    description: logo\n    width: 512\n    height: 512\n    output: ../escape.png\n").unwrap();
        assert!(plan(root.path(), &options()).is_err());
    }
}
