//! Evidence-rich `brandi scan` pipeline.
//!
//! Surface extraction remains deterministic and in-process. Bound contributes
//! bounded, disposable source snapshots as independent structural evidence;
//! brief/guideline rules turn extracted surfaces into findings; and an
//! installed Dreamseq makes ancestor `.dreams/*.dreams` files visible as
//! read-only delivery targets.

use crate::brief::Brief;
use crate::error::{BrandiError, Result};
use crate::guidelines::Guidelines;
use crate::output::{self, OutputEvent, Severity as OutputSeverity};
use crate::process;
use crate::types::{
    ExposureAudience, Finding, ScanDiagnostic, Surface, SurfaceDomain, SurfaceProvenance,
};
use form3::table::{ContentArrangement, Table, TableStyle};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use walkdir::WalkDir;

pub const SCAN_SCHEMA_VERSION: &str = "brandi-scan-v6";
const PHASES: u64 = 5;
const BOUND_TOKEN_LIMIT_PER_FILE: usize = 512;
const BOUND_SIZE_LIMIT_PER_FILE: u64 = crate::surface::MAX_SOURCE_FILE_BYTES;
const MAX_BOUND_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_BOUND_SCOPES: usize = 24;
const MAX_DREAM_FILES: usize = 256;
const PROJECT_ROOT_MARKERS: [&str; 8] = [
    ".git",
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "Package.swift",
];
static SNAPSHOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
pub struct ScanMilestone {
    pub id: String,
    pub label: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoundScopeEvidence {
    pub extension: String,
    pub directory: PathBuf,
    pub files: usize,
    pub source_bytes: u64,
    pub snapshot_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoundEvidence {
    pub executable: PathBuf,
    pub token_limit_per_file: usize,
    pub size_limit_per_file: u64,
    pub scopes: Vec<BoundScopeEvidence>,
    pub files: usize,
    pub source_bytes: u64,
    pub snapshot_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextSource {
    pub path: PathBuf,
    pub available: bool,
    pub optional: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrandContext {
    pub brief: Vec<ContextSource>,
    pub guidelines: Vec<ContextSource>,
    pub findings_evaluated: bool,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DreamContext {
    pub installed: bool,
    pub executable: Option<PathBuf>,
    pub searched_from: PathBuf,
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectTarget {
    Configured(PathBuf),
    Uninitialized(PathBuf),
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryKind {
    Brief,
    Guideline,
    Dream,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindingDelivery {
    pub kind: DeliveryKind,
    pub target: PathBuf,
    pub available: bool,
    /// Stable indexes into [`ScanReport::findings`].
    pub finding_indices: Vec<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileSurfaceCount {
    pub path: PathBuf,
    pub surfaces: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SurfaceAggregation {
    pub total: usize,
    pub analysable: usize,
    pub operational: usize,
    pub low_value: usize,
    pub by_kind: BTreeMap<String, usize>,
    pub by_domain: BTreeMap<String, usize>,
    pub by_provenance: BTreeMap<String, usize>,
    pub by_audience: BTreeMap<String, usize>,
    pub top_files: Vec<FileSurfaceCount>,
    pub low_value_files: Vec<FileSurfaceCount>,
}

/// Versioned scan output. The extraction fields stay at the top level for
/// compatibility with consumers of the former `ScanResult` JSON.
#[derive(Debug, Clone, Serialize)]
pub struct ScanReport {
    pub schema_version: String,
    pub extraction_schema_version: String,
    pub root: PathBuf,
    pub analysis_complete: bool,
    pub complete: bool,
    pub surfaces: Vec<Surface>,
    pub aggregation: SurfaceAggregation,
    pub diagnostics: Vec<ScanDiagnostic>,
    pub files_considered: usize,
    pub files_scanned: usize,
    pub milestones: Vec<ScanMilestone>,
    pub bound: BoundEvidence,
    pub context: BrandContext,
    pub dreams: DreamContext,
    pub findings: Vec<Finding>,
    pub deliveries: Vec<FindingDelivery>,
}

struct MilestoneTracker {
    completed: Vec<ScanMilestone>,
    current: u64,
    event_sequence: u64,
}

impl MilestoneTracker {
    fn new() -> Result<Self> {
        output::event(&OutputEvent::CommandStarted {
            command: "scan".into(),
            sequence: 0,
        })?;
        Ok(Self {
            completed: Vec::new(),
            current: 0,
            event_sequence: 1,
        })
    }

    fn phase<T>(
        &mut self,
        id: &str,
        label: &str,
        work: impl FnOnce() -> Result<T>,
        detail: impl FnOnce(&T) -> String,
    ) -> Result<T> {
        self.current += 1;
        output::event(&OutputEvent::PhaseStarted {
            id: id.into(),
            label: label.into(),
            sequence: self.event_sequence,
        })?;
        self.event_sequence += 1;
        output::milestone(
            self.current,
            PHASES,
            label,
            Some("started"),
            OutputSeverity::Info,
        )?;
        let progress = output::ProgressGuard::start(label);
        let result = work();
        progress.finish();
        match result {
            Ok(value) => {
                let detail = detail(&value);
                output::event(&OutputEvent::Progressed {
                    phase: id.into(),
                    current: 1,
                    total: Some(1),
                    sequence: self.event_sequence,
                })?;
                self.event_sequence += 1;
                output::milestone(
                    self.current,
                    PHASES,
                    label,
                    Some(&detail),
                    OutputSeverity::Success,
                )?;
                self.completed.push(ScanMilestone {
                    id: id.into(),
                    label: label.into(),
                    status: "complete".into(),
                    detail,
                });
                Ok(value)
            }
            Err(error) => {
                output::milestone(
                    self.current,
                    PHASES,
                    label,
                    Some(&error.to_string()),
                    OutputSeverity::Error,
                )?;
                Err(error)
            }
        }
    }

    fn finish(&mut self, summary: String) -> Result<()> {
        output::event(&OutputEvent::CommandFinished {
            status: "ok".into(),
            summary,
            sequence: self.event_sequence,
        })?;
        Ok(())
    }
}

pub fn run(requested_root: Option<&Path>) -> Result<ScanReport> {
    let mut tracker = MilestoneTracker::new()?;
    let root = tracker.phase(
        "scope",
        "Resolve scan scope",
        || resolve_project_root(requested_root),
        |root| root.display().to_string(),
    )?;
    let extraction = tracker.phase(
        "surfaces",
        "Discover user-facing surfaces",
        || crate::surface::scan_project(&root),
        |scan| {
            format!(
                "{} surfaces in {} files",
                scan.surfaces.len(),
                scan.files_scanned
            )
        },
    )?;
    let bound_executable = find_executable("bound").ok_or_else(|| {
        BrandiError::NotFound(
            "bound is required by `brandi scan`; install it and ensure it is on PATH".into(),
        )
    })?;
    let bound = tracker.phase(
        "bound",
        "Capture bounded source evidence with Bound",
        || capture_bound(&root, &extraction.surfaces, &bound_executable),
        |bound| {
            format!(
                "{} files · {} bundles · {} snapshot bytes",
                bound.files,
                bound.scopes.len(),
                bound.snapshot_bytes
            )
        },
    )?;
    let brand = tracker.phase(
        "brand-context",
        "Evaluate brief and guideline findings",
        || evaluate_brand_context(&root, &extraction.surfaces),
        |brand| {
            if brand.context.findings_evaluated {
                format!("{} findings evaluated", brand.findings.len())
            } else {
                format!(
                    "skipped · {} context diagnostics",
                    brand.context.diagnostics.len()
                )
            }
        },
    )?;
    let context = brand.context;
    let findings = brand.findings;
    let (dreams, deliveries) = tracker.phase(
        "delivery",
        "Route findings to brand and Dreamseq context",
        || {
            let dreams = discover_dream_context(&root)?;
            let deliveries = build_deliveries(&context, &dreams, &findings);
            Ok((dreams, deliveries))
        },
        |(dreams, deliveries)| {
            format!(
                "{} targets · {} dream files{}",
                deliveries.len(),
                dreams.files.len(),
                if dreams.installed {
                    ""
                } else {
                    " (Dreamseq not installed)"
                }
            )
        },
    )?;

    let analysis_complete = extraction.complete && context.findings_evaluated;
    let aggregation = aggregate_surfaces(&root, &extraction.surfaces);
    let summary = format!(
        "{} surfaces, {} findings, {} delivery targets",
        extraction.surfaces.len(),
        findings.len(),
        deliveries.len()
    );
    tracker.finish(summary)?;
    Ok(ScanReport {
        schema_version: SCAN_SCHEMA_VERSION.into(),
        extraction_schema_version: extraction.schema_version,
        root,
        analysis_complete,
        complete: extraction.complete,
        surfaces: extraction.surfaces,
        aggregation,
        diagnostics: extraction.diagnostics,
        files_considered: extraction.files_considered,
        files_scanned: extraction.files_scanned,
        milestones: tracker.completed,
        bound,
        context,
        dreams,
        findings,
        deliveries,
    })
}

fn aggregate_surfaces(root: &Path, surfaces: &[Surface]) -> SurfaceAggregation {
    let mut by_kind = BTreeMap::new();
    let mut by_domain = BTreeMap::new();
    let mut by_provenance = BTreeMap::new();
    let mut by_audience = BTreeMap::new();
    let mut files: BTreeMap<PathBuf, usize> = BTreeMap::new();
    let mut low_value_files: BTreeMap<PathBuf, usize> = BTreeMap::new();
    let mut analysable = 0usize;
    let mut operational = 0usize;
    let mut low_value = 0usize;
    for surface in surfaces {
        let significance = &surface.significance;
        *by_kind.entry(surface.kind.to_string()).or_insert(0) += 1;
        *by_domain
            .entry(domain_name(significance.domain).into())
            .or_insert(0) += 1;
        *by_provenance
            .entry(provenance_name(significance.provenance).into())
            .or_insert(0) += 1;
        *by_audience
            .entry(audience_name(significance.exposure.audience).into())
            .or_insert(0) += 1;
        let path = surface
            .path
            .strip_prefix(root)
            .unwrap_or(&surface.path)
            .to_path_buf();
        *files.entry(path.clone()).or_insert(0) += 1;
        if significance.semantic_weight > 25 {
            analysable += 1;
        } else {
            low_value += 1;
            *low_value_files.entry(path).or_insert(0) += 1;
        }
        if significance.domain == SurfaceDomain::Operational {
            operational += 1;
        }
    }
    SurfaceAggregation {
        total: surfaces.len(),
        analysable,
        operational,
        low_value,
        by_kind,
        by_domain,
        by_provenance,
        by_audience,
        top_files: ranked_files(files),
        low_value_files: ranked_files(low_value_files),
    }
}

fn ranked_files(files: BTreeMap<PathBuf, usize>) -> Vec<FileSurfaceCount> {
    let mut files: Vec<_> = files
        .into_iter()
        .map(|(path, surfaces)| FileSurfaceCount { path, surfaces })
        .collect();
    files.sort_by(|left, right| {
        right
            .surfaces
            .cmp(&left.surfaces)
            .then(left.path.cmp(&right.path))
    });
    files.truncate(10);
    files
}

fn domain_name(value: SurfaceDomain) -> &'static str {
    match value {
        SurfaceDomain::Semantic => "semantic",
        SurfaceDomain::Operational => "operational",
    }
}

fn provenance_name(value: SurfaceProvenance) -> &'static str {
    match value {
        SurfaceProvenance::Authored => "authored",
        SurfaceProvenance::Generated => "generated",
        SurfaceProvenance::Inherited => "inherited",
        SurfaceProvenance::Vendored => "vendored",
        SurfaceProvenance::Mirrored => "mirrored",
    }
}

fn audience_name(value: ExposureAudience) -> &'static str {
    match value {
        ExposureAudience::Public => "public",
        ExposureAudience::User => "user",
        ExposureAudience::Contributor => "contributor",
        ExposureAudience::Agent => "agent",
        ExposureAudience::Developer => "developer",
        ExposureAudience::None => "none",
    }
}

fn resolve_project_root(path: Option<&Path>) -> Result<PathBuf> {
    match resolve_project_target(path)? {
        ProjectTarget::Configured(root) => Ok(root),
        ProjectTarget::Uninitialized(root) => Err(BrandiError::NotFound(format!(
            "target has no .brandi configuration: {}",
            root.display()
        ))),
    }
}

pub fn resolve_project_target(path: Option<&Path>) -> Result<ProjectTarget> {
    let cwd = std::env::current_dir()?;
    resolve_project_target_from(path, &cwd)
}

fn resolve_project_target_from(path: Option<&Path>, cwd: &Path) -> Result<ProjectTarget> {
    match path {
        Some(path) => {
            let root = path.canonicalize().map_err(|_| {
                BrandiError::NotFound(format!("scan path does not exist: {}", path.display()))
            })?;
            if !root.is_dir() {
                return Err(BrandiError::Invalid(format!(
                    "explicit target path is not a directory: {}",
                    path.display()
                )));
            }
            if root.join(".brandi").is_dir() {
                return Ok(ProjectTarget::Configured(root));
            }
            if root
                .parent()
                .into_iter()
                .flat_map(Path::ancestors)
                .any(|ancestor| ancestor.join(".brandi").is_dir())
            {
                return Err(BrandiError::Invalid(format!(
                    "explicit target path must be a top-level project root, not a nested path: {}",
                    path.display()
                )));
            }
            Ok(ProjectTarget::Uninitialized(root))
        }
        None => {
            let cwd = cwd.canonicalize()?;
            if let Some(root) = cwd
                .ancestors()
                .find(|candidate| candidate.join(".brandi").is_dir())
                .map(Path::to_path_buf)
            {
                return Ok(ProjectTarget::Configured(root));
            }
            let root = cwd
                .ancestors()
                .find(|candidate| is_project_root(candidate))
                .unwrap_or(&cwd)
                .to_path_buf();
            Ok(ProjectTarget::Uninitialized(root))
        }
    }
}

fn is_project_root(path: &Path) -> bool {
    PROJECT_ROOT_MARKERS
        .iter()
        .any(|marker| path.join(marker).exists())
}

struct BrandEvaluation {
    context: BrandContext,
    findings: Vec<Finding>,
}

fn evaluate_brand_context(root: &Path, surfaces: &[Surface]) -> Result<BrandEvaluation> {
    let brief_sources = ["identity.yaml", "audience.yaml"]
        .into_iter()
        .map(|name| context_source(root, name, false))
        .collect();
    let guideline_sources = [
        ("voice.yaml", false),
        ("visual.yaml", false),
        ("prohibited.yaml", false),
        ("rules.yaml", true),
    ]
    .into_iter()
    .map(|(name, optional)| context_source(root, name, optional))
    .collect();
    let mut diagnostics = Vec::new();
    let brief = match Brief::load(root) {
        Ok(brief) => Some(brief),
        Err(error) => {
            diagnostics.push(format!("brief unavailable: {error}"));
            None
        }
    };
    let guidelines = match Guidelines::load(root) {
        Ok(guidelines) => Some(guidelines),
        Err(error) => {
            diagnostics.push(format!("guidelines unavailable: {error}"));
            None
        }
    };
    let findings = match (&brief, &guidelines) {
        (Some(brief), Some(guidelines)) => {
            crate::rules::run_rules_on_surfaces(brief, guidelines, root, surfaces)
        }
        _ => Vec::new(),
    };
    let findings_evaluated = brief.is_some() && guidelines.is_some();
    Ok(BrandEvaluation {
        context: BrandContext {
            brief: brief_sources,
            guidelines: guideline_sources,
            findings_evaluated,
            diagnostics,
        },
        findings,
    })
}

fn context_source(root: &Path, name: &str, optional: bool) -> ContextSource {
    let path = root.join(".brandi").join(name);
    ContextSource {
        available: path.is_file(),
        path,
        optional,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BundleScope {
    extension: String,
    directory: PathBuf,
}

fn capture_bound(root: &Path, surfaces: &[Surface], executable: &Path) -> Result<BoundEvidence> {
    let scopes = bundle_scopes(root, surfaces);
    let mut evidence = Vec::with_capacity(scopes.len());
    for scope in scopes {
        evidence.push(run_bound_scope(executable, &scope)?);
    }
    Ok(BoundEvidence {
        executable: executable.to_path_buf(),
        token_limit_per_file: BOUND_TOKEN_LIMIT_PER_FILE,
        size_limit_per_file: BOUND_SIZE_LIMIT_PER_FILE,
        files: evidence.iter().map(|scope| scope.files).sum(),
        source_bytes: evidence.iter().map(|scope| scope.source_bytes).sum(),
        snapshot_bytes: evidence.iter().map(|scope| scope.snapshot_bytes).sum(),
        scopes: evidence,
    })
}

fn bundle_scopes(root: &Path, surfaces: &[Surface]) -> Vec<BundleScope> {
    let allowed: BTreeSet<&str> = [
        "md", "markdown", "rst", "rs", "ts", "tsx", "js", "jsx", "py", "go", "css", "scss",
    ]
    .into_iter()
    .collect();
    let mut scopes = BTreeSet::new();
    for surface in surfaces {
        let path = if surface.path.is_absolute() {
            surface.path.clone()
        } else {
            root.join(&surface.path)
        };
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        let extension = extension.to_ascii_lowercase();
        if !allowed.contains(extension.as_str()) {
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let directory = match relative.components().next() {
            Some(first) if relative.components().count() > 1 => root.join(first.as_os_str()),
            _ => root.to_path_buf(),
        };
        scopes.insert(BundleScope {
            extension,
            directory,
        });
    }
    if scopes.is_empty() {
        scopes.insert(BundleScope {
            extension: "md".into(),
            directory: root.to_path_buf(),
        });
    }
    let root_extensions: BTreeSet<String> = scopes
        .iter()
        .filter(|scope| scope.directory == root)
        .map(|scope| scope.extension.clone())
        .collect();
    scopes
        .into_iter()
        .filter(|scope| {
            scope.directory == root || !root_extensions.contains(scope.extension.as_str())
        })
        .take(MAX_BOUND_SCOPES)
        .collect()
}

struct TempSnapshot(PathBuf);

impl TempSnapshot {
    fn new() -> Self {
        let sequence = SNAPSHOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "brandi-bound-{}-{sequence}.json",
            std::process::id()
        )))
    }
}

impl Drop for TempSnapshot {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn run_bound_scope(executable: &Path, scope: &BundleScope) -> Result<BoundScopeEvidence> {
    let snapshot = TempSnapshot::new();
    let filter = format!("[.{}]", scope.extension);
    let mut result = run_bound_process(executable, scope, &filter, &snapshot.0)?;
    // Bound uses an internal cache and older builds can occasionally return
    // success without materialising `--out` when concurrent scans overlap.
    // Retry that one narrow condition once; every attempt remains bounded and
    // uses the same private temp destination.
    if result.status.success() && !snapshot.0.is_file() {
        result = run_bound_process(executable, scope, &filter, &snapshot.0)?;
    }
    if result.timed_out {
        return Err(BrandiError::Invalid(format!(
            "Bound timed out while scanning {} files beneath {}",
            scope.extension,
            scope.directory.display()
        )));
    }
    if !result.status.success() {
        let detail = String::from_utf8_lossy(&result.stderr);
        let detail = detail.trim();
        return Err(BrandiError::Invalid(format!(
            "Bound failed for {} beneath {}{}",
            scope.extension,
            scope.directory.display(),
            if detail.is_empty() {
                String::new()
            } else {
                format!(": {}", output::sanitize_untrusted(detail))
            }
        )));
    }
    let metadata = fs::metadata(&snapshot.0)?;
    if metadata.len() > MAX_BOUND_SNAPSHOT_BYTES {
        return Err(BrandiError::Invalid(format!(
            "Bound snapshot exceeded the {MAX_BOUND_SNAPSHOT_BYTES} byte safety limit"
        )));
    }
    let document: serde_json::Value = serde_json::from_slice(&fs::read(&snapshot.0)?)?;
    let files = document
        .get("files")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| BrandiError::Invalid("Bound returned no files array".into()))?;
    let source_bytes = files
        .iter()
        .filter_map(|file| file.pointer("/metadata/size_bytes")?.as_u64())
        .sum();
    Ok(BoundScopeEvidence {
        extension: scope.extension.clone(),
        directory: scope.directory.clone(),
        files: files.len(),
        source_bytes,
        snapshot_bytes: metadata.len(),
    })
}

fn run_bound_process(
    executable: &Path,
    scope: &BundleScope,
    filter: &str,
    destination: &Path,
) -> Result<process::BoundedOutput> {
    Ok(process::run_bounded(
        Command::new(executable)
            .arg(filter)
            .args([
                "--json",
                "--meta",
                "--tree",
                "--token-limit",
                &BOUND_TOKEN_LIMIT_PER_FILE.to_string(),
                "--size-limit",
                &BOUND_SIZE_LIMIT_PER_FILE.to_string(),
                "--depth-limit",
                &crate::surface::MAX_SCAN_DEPTH.to_string(),
                "--out",
            ])
            .arg(destination)
            .current_dir(&scope.directory),
        Duration::from_secs(45),
        256 * 1024,
    )?)
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 && is_executable(candidate) {
        return candidate.canonicalize().ok();
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find(|path| is_executable(path))
        .and_then(|path| path.canonicalize().ok())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn discover_dream_context(project_root: &Path) -> Result<DreamContext> {
    let executable = find_executable("dreamseq");
    let searched_from = project_root.canonicalize()?;
    let mut files = BTreeSet::new();
    if executable.is_some() {
        for ancestor in searched_from.ancestors() {
            let directory = ancestor.join(".dreams");
            if !directory.is_dir() {
                continue;
            }
            for entry in WalkDir::new(&directory)
                .max_depth(4)
                .follow_links(false)
                .into_iter()
                .filter_map(std::result::Result::ok)
            {
                if entry.file_type().is_file()
                    && entry.path().extension().and_then(|value| value.to_str()) == Some("dreams")
                    && dream_belongs_to_project(entry.path(), &searched_from)
                {
                    files.insert(entry.path().canonicalize()?);
                    if files.len() >= MAX_DREAM_FILES {
                        break;
                    }
                }
            }
            if files.len() >= MAX_DREAM_FILES {
                break;
            }
        }
    }
    Ok(DreamContext {
        installed: executable.is_some(),
        executable,
        searched_from,
        files: files.into_iter().collect(),
    })
}

fn dream_belongs_to_project(path: &Path, project_root: &Path) -> bool {
    dream_references_project(path, project_root)
}

fn dream_references_project(path: &Path, project_root: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if metadata.len() > crate::surface::MAX_SOURCE_FILE_BYTES {
        return false;
    }
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    let canonical = project_root.to_string_lossy();
    if content.contains(canonical.as_ref()) {
        return true;
    }
    let project_name = project_root.file_name().and_then(|value| value.to_str());
    serde_yaml::from_str::<serde_yaml::Value>(&content)
        .ok()
        .and_then(|document| document.get("project")?.as_str().map(str::to_owned))
        .is_some_and(|reference| {
            project_name.is_some_and(|name| reference == name)
                || path
                    .parent()
                    .unwrap_or(Path::new(""))
                    .join(&reference)
                    .canonicalize()
                    .is_ok_and(|referenced| referenced == project_root)
        })
}

fn build_deliveries(
    context: &BrandContext,
    dreams: &DreamContext,
    findings: &[Finding],
) -> Vec<FindingDelivery> {
    let mut routes: BTreeMap<PathBuf, (DeliveryKind, bool, Vec<usize>)> = BTreeMap::new();
    for source in &context.brief {
        routes.insert(
            source.path.clone(),
            (DeliveryKind::Brief, source.available, Vec::new()),
        );
    }
    for source in &context.guidelines {
        routes.insert(
            source.path.clone(),
            (DeliveryKind::Guideline, source.available, Vec::new()),
        );
    }
    for (index, finding) in findings.iter().enumerate() {
        for target in policy_targets(&context.brief, &context.guidelines, &finding.rule_id) {
            if let Some((_, _, indexes)) = routes.get_mut(&target) {
                indexes.push(index);
            }
        }
    }
    for dream in &dreams.files {
        routes.insert(
            dream.clone(),
            (DeliveryKind::Dream, true, (0..findings.len()).collect()),
        );
    }
    routes
        .into_iter()
        .map(
            |(target, (kind, available, finding_indices))| FindingDelivery {
                kind,
                target,
                available,
                finding_indices,
            },
        )
        .collect()
}

fn policy_targets(
    brief: &[ContextSource],
    guidelines: &[ContextSource],
    rule_id: &str,
) -> Vec<PathBuf> {
    let names: &[&str] = match rule_id {
        "terminology-variant"
        | "sentence-case-headings"
        | "readme-mission"
        | "readme-h1"
        | "terminology-ownership" => &["identity.yaml"],
        "prohibited-term" | "accessibility-language" | "inclusive-terminology" => {
            &["prohibited.yaml"]
        }
        "error-prefix"
        | "error-actionable"
        | "exclamation-limit"
        | "reading-level"
        | "localization-readiness" => &["voice.yaml"],
        "palette-adherence" | "readme-image-refs" => &["visual.yaml"],
        _ => &["rules.yaml"],
    };
    brief
        .iter()
        .chain(guidelines.iter())
        .filter(|source| {
            source
                .path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| names.contains(&name))
        })
        .map(|source| source.path.clone())
        .collect()
}

pub fn render_human(report: &ScanReport) -> String {
    // Default output is corpus-level evidence; `--verbose` opts into the
    // location-by-location debugging ledger below.
    let unicode = output::context().unicode;
    let verbose = output::context().verbose;
    let mut out = Vec::new();
    out.push(output::styled("Brandi scan", OutputSeverity::Info).to_string());
    out.push(output::styled(report.root.display().to_string(), OutputSeverity::Muted).to_string());
    out.push(String::new());

    let mut summary = Table::new();
    summary
        .set_style(if unicode {
            TableStyle::Rounded
        } else {
            TableStyle::Ascii
        })
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["Evidence", "Result"])
        .add_row(vec![
            "Bound",
            &format!(
                "{} files · {} bundles · {} bytes",
                report.bound.files,
                report.bound.scopes.len(),
                report.bound.snapshot_bytes
            ),
        ])
        .add_row(vec![
            "Surfaces",
            &format!(
                "{} across {} scanned files",
                report.surfaces.len(),
                report.files_scanned
            ),
        ])
        .add_row(vec![
            "Analysable",
            &format!(
                "{} · {} low-value · {} operational",
                report.aggregation.analysable,
                report.aggregation.low_value,
                report.aggregation.operational
            ),
        ])
        .add_row(vec!["Findings", &report.findings.len().to_string()])
        .add_row(vec![
            "Brand context",
            if report.context.findings_evaluated {
                "brief + guidelines loaded"
            } else {
                "incomplete; findings not evaluated"
            },
        ])
        .add_row(vec![
            "Dreamseq",
            &format!(
                "{} · {} dream files",
                if report.dreams.installed {
                    "installed"
                } else {
                    "not installed"
                },
                report.dreams.files.len()
            ),
        ]);
    out.push(summary.to_string().trim_end().to_string());

    out.push(String::new());
    out.push(output::styled("Surface significance", OutputSeverity::Info).to_string());
    let mut significance = Table::new();
    significance
        .set_style(if unicode {
            TableStyle::Square
        } else {
            TableStyle::Ascii
        })
        .set_header(vec!["Dimension", "Distribution"]);
    for (label, values) in [
        ("Type", &report.aggregation.by_kind),
        ("Domain", &report.aggregation.by_domain),
        ("Provenance", &report.aggregation.by_provenance),
        ("Audience", &report.aggregation.by_audience),
    ] {
        significance.add_row(vec![
            label.to_string(),
            values
                .iter()
                .map(|(name, count)| format!("{name} {count}"))
                .collect::<Vec<_>>()
                .join(" · "),
        ]);
    }
    out.push(significance.to_string().trim_end().to_string());

    if !report.aggregation.top_files.is_empty() {
        out.push(String::new());
        out.push(output::styled("Top surfaces", OutputSeverity::Info).to_string());
        for file in &report.aggregation.top_files {
            out.push(format!("{:>6}  {}", file.surfaces, file.path.display()));
        }
    }
    if !report.aggregation.low_value_files.is_empty() {
        out.push(String::new());
        out.push(output::styled("Low-value corpus", OutputSeverity::Muted).to_string());
        for file in &report.aggregation.low_value_files {
            out.push(format!("{:>6}  {}", file.surfaces, file.path.display()));
        }
    }

    out.push(String::new());
    out.push(output::styled("Finding delivery", OutputSeverity::Info).to_string());
    let mut deliveries = Table::new();
    deliveries
        .set_style(if unicode {
            TableStyle::Square
        } else {
            TableStyle::Ascii
        })
        .set_header(vec!["Target", "Kind", "Findings"]);
    for delivery in report.deliveries.iter().take(output::DEFAULT_LIST_CAP) {
        deliveries.add_row(vec![
            delivery.target.display().to_string(),
            format!("{:?}", delivery.kind).to_ascii_lowercase(),
            if delivery.available {
                delivery.finding_indices.len().to_string()
            } else {
                "unavailable".into()
            },
        ]);
    }
    out.push(deliveries.to_string().trim_end().to_string());
    if report.deliveries.len() > output::DEFAULT_LIST_CAP {
        out.push(format!(
            "… +{} more delivery targets (use --format json for all)",
            report.deliveries.len() - output::DEFAULT_LIST_CAP
        ));
    }

    if verbose {
        out.push(String::new());
        out.push(output::styled("Findings", OutputSeverity::Info).to_string());
        if report.findings.is_empty() {
            out.push(output::styled("No brand findings.", OutputSeverity::Success).to_string());
        } else {
            for finding in report.findings.iter().take(output::DEFAULT_LIST_CAP) {
                let severity = match finding.severity {
                    crate::types::Severity::Error => OutputSeverity::Error,
                    crate::types::Severity::Warning => OutputSeverity::Warning,
                    crate::types::Severity::Info => OutputSeverity::Info,
                };
                let location = finding.line.map_or_else(
                    || finding.path.display().to_string(),
                    |line| format!("{}:{line}", finding.path.display()),
                );
                out.push(
                    output::styled(
                        format!(
                            "{} {} [{}] {}",
                            severity_marker(severity, unicode),
                            location,
                            finding.rule_id,
                            output::sanitize_untrusted(&finding.message)
                        ),
                        severity,
                    )
                    .to_string(),
                );
                if let Some(suggestion) = &finding.suggestion {
                    out.push(format!(
                        "    {} {}",
                        if unicode { "→" } else { "->" },
                        output::sanitize_untrusted(suggestion)
                    ));
                }
            }
            if report.findings.len() > output::DEFAULT_LIST_CAP {
                out.push(format!(
                    "… +{} more findings (use --format json for all)",
                    report.findings.len() - output::DEFAULT_LIST_CAP
                ));
            }
        }

        out.push(String::new());
        out.push(output::styled("Discovered surfaces", OutputSeverity::Info).to_string());
        for surface in report.surfaces.iter().take(output::DEFAULT_LIST_CAP) {
            out.push(format!(
                "{}:{} [{}:{} confidence={} relevance={} weight={}] {}",
                surface.path.display(),
                surface.line,
                surface.kind,
                domain_name(surface.significance.domain),
                surface.confidence,
                surface.significance.relevance,
                surface.significance.semantic_weight,
                output::sanitize_untrusted(&surface.text)
            ));
        }
        if report.surfaces.len() > output::DEFAULT_LIST_CAP {
            out.push(format!(
                "… +{} more surfaces (use --format json for all)",
                report.surfaces.len() - output::DEFAULT_LIST_CAP
            ));
        }
    }
    for diagnostic in &report.context.diagnostics {
        out.push(
            output::styled(
                format!("warning: {}", output::sanitize_untrusted(diagnostic)),
                OutputSeverity::Warning,
            )
            .to_string(),
        );
    }
    out.join("\n")
}

fn severity_marker(severity: OutputSeverity, unicode: bool) -> &'static str {
    match (severity, unicode) {
        (OutputSeverity::Error, true) => "✕",
        (OutputSeverity::Warning, true) => "▲",
        (OutputSeverity::Info, true) => "●",
        (OutputSeverity::Error, false) => "x",
        (OutputSeverity::Warning, false) => "!",
        _ => "*",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn surface(root: &Path, relative: &str) -> Surface {
        Surface {
            kind: crate::types::SurfaceKind::Doc,
            path: root.join(relative),
            line: 1,
            text: "A public surface".into(),
            context: "test".into(),
            confidence: 100,
            exposure: 3,
            significance: crate::types::SurfaceSignificance::default(),
            reason: "test".into(),
            start_byte: 0,
            end_byte: 1,
        }
    }

    #[test]
    fn bundle_scopes_are_filtered_and_deduplicated() {
        let root = Path::new("/project");
        let scopes = bundle_scopes(
            root,
            &[
                surface(root, "README.md"),
                surface(root, "docs/guide.md"),
                surface(root, "docs/deeper/note.md"),
            ],
        );
        assert_eq!(
            scopes,
            vec![BundleScope {
                extension: "md".into(),
                directory: root.into()
            }]
        );
    }

    #[test]
    fn implicit_scope_finds_nearest_project_but_explicit_scope_requires_its_root() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        let nested = root.join("src/deep");
        fs::create_dir_all(root.join(".brandi")).unwrap();
        fs::create_dir_all(&nested).unwrap();
        let canonical_root = root.canonicalize().unwrap();

        assert_eq!(
            resolve_project_target_from(None, &nested).unwrap(),
            ProjectTarget::Configured(canonical_root.clone())
        );
        assert_eq!(
            resolve_project_target_from(Some(&root), &nested).unwrap(),
            ProjectTarget::Configured(canonical_root)
        );
        let error = resolve_project_target_from(Some(&nested), &nested)
            .unwrap_err()
            .to_string();
        assert!(error.contains("top-level project root"), "{error}");
    }

    #[test]
    fn implicit_uninitialized_scope_uses_the_nearest_project_marker() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        let nested = root.join("src/deep");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname='sample'\n").unwrap();
        assert_eq!(
            resolve_project_target_from(None, &nested).unwrap(),
            ProjectTarget::Uninitialized(root.canonicalize().unwrap())
        );
    }

    #[test]
    fn dream_scope_requires_a_target_reference_even_for_project_local_files() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("brandi");
        let local = root.join(".dreams/active.dreams");
        let local_matching = root.join(".dreams/matching.dreams");
        let matching = temporary.path().join("matching.dreams");
        let unrelated = temporary.path().join("unrelated.dreams");
        fs::create_dir_all(local.parent().unwrap()).unwrap();
        fs::write(&local, "project: other\n").unwrap();
        fs::write(&local_matching, "project: brandi\n").unwrap();
        fs::write(&matching, "project: brandi\n").unwrap();
        fs::write(&unrelated, "project: deliver\n").unwrap();

        assert!(!dream_belongs_to_project(&local, &root));
        assert!(dream_belongs_to_project(&local_matching, &root));
        assert!(dream_belongs_to_project(&matching, &root));
        assert!(!dream_belongs_to_project(&unrelated, &root));
    }

    #[test]
    fn dream_reference_can_use_the_canonical_project_path() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("project");
        fs::create_dir_all(&root).unwrap();
        let dream = temporary.path().join("path-reference.dreams");
        fs::write(&dream, format!("project: {}\n", root.display())).unwrap();
        assert!(dream_references_project(&dream, &root));
    }

    #[test]
    fn aggregation_separates_detected_analysable_and_operational_surfaces() {
        let root = Path::new("/project");
        let semantic = surface(root, "README.md");
        let mut operational = surface(root, ".claude/CLAUDE.md");
        operational.significance.domain = SurfaceDomain::Operational;
        operational.significance.provenance = SurfaceProvenance::Generated;
        operational.significance.exposure.audience = ExposureAudience::Agent;
        operational.significance.semantic_weight = 5;
        let aggregation = aggregate_surfaces(root, &[semantic, operational]);
        assert_eq!(aggregation.total, 2);
        assert_eq!(aggregation.analysable, 1);
        assert_eq!(aggregation.operational, 1);
        assert_eq!(aggregation.low_value, 1);
        assert_eq!(aggregation.by_provenance["generated"], 1);
        assert_eq!(
            aggregation.low_value_files[0].path,
            Path::new(".claude/CLAUDE.md")
        );
    }

    #[test]
    fn bound_snapshots_are_parsed_and_deleted() {
        let temporary = tempfile::tempdir().unwrap();
        let fake = temporary.path().join("bound");
        fs::write(
            &fake,
            "#!/bin/sh\nout=''\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = --out ]; then out=$2; shift 2; else shift; fi\ndone\nprintf '%s' '{\"tree\":\"README.md\",\"files\":[{\"metadata\":{\"size_bytes\":12},\"content\":\"hello\"}]}' > \"$out\"\n",
        )
        .unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
        let scope = BundleScope {
            extension: "md".into(),
            directory: temporary.path().into(),
        };
        let evidence = run_bound_scope(&fake, &scope).unwrap();
        assert_eq!(evidence.files, 1);
        assert_eq!(evidence.source_bytes, 12);
        assert!(evidence.snapshot_bytes > 0);
    }

    #[test]
    fn bound_retries_success_without_an_output_file_once() {
        let temporary = tempfile::tempdir().unwrap();
        let fake = temporary.path().join("bound");
        fs::write(
            &fake,
            "#!/bin/sh\nout=''\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = --out ]; then out=$2; shift 2; else shift; fi\ndone\nif [ ! -f .attempted ]; then touch .attempted; exit 0; fi\nprintf '%s' '{\"tree\":null,\"files\":[]}' > \"$out\"\n",
        )
        .unwrap();
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
        let scope = BundleScope {
            extension: "md".into(),
            directory: temporary.path().into(),
        };
        let evidence = run_bound_scope(&fake, &scope).unwrap();
        assert_eq!(evidence.files, 0);
        assert!(temporary.path().join(".attempted").is_file());
    }

    #[test]
    fn dream_files_are_delivery_targets_when_discovered() {
        let root = tempfile::tempdir().unwrap();
        let dream = root.path().join(".dreams/active.dreams");
        fs::create_dir_all(dream.parent().unwrap()).unwrap();
        fs::write(&dream, "dreams: []\n").unwrap();
        let context = BrandContext {
            brief: vec![context_source(root.path(), "identity.yaml", false)],
            guidelines: vec![context_source(root.path(), "voice.yaml", false)],
            findings_evaluated: true,
            diagnostics: Vec::new(),
        };
        let dreams = DreamContext {
            installed: true,
            executable: Some("/bin/dreamseq".into()),
            searched_from: root.path().into(),
            files: vec![dream.clone()],
        };
        let finding = Finding {
            rule_id: "error-prefix".into(),
            severity: crate::types::Severity::Warning,
            kind: None,
            path: root.path().join("README.md"),
            line: Some(1),
            message: "message".into(),
            suggestion: None,
            semantics: crate::types::FindingSemantics::default(),
        };
        let deliveries = build_deliveries(&context, &dreams, &[finding]);
        assert!(deliveries
            .iter()
            .any(|delivery| { delivery.target == dream && delivery.finding_indices == vec![0] }));
    }
}
