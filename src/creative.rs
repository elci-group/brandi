//! Asset generation and revision workflows for `direct` and `reshoot`.

use crate::assets::{self, AssetClass};
use crate::brief::Brief;
use crate::error::{BrandiError, Result};
use crate::guidelines::{parse_hex_color, Dimensions, Guidelines};
use crate::process::{run_bounded, DEFAULT_SUBPROCESS_TIMEOUT, MAX_SUBPROCESS_OUTPUT_BYTES};
use crate::propose;
use crate::tape::{self, TapeRequest};
use crate::types::SurfaceKind;
use image::ImageFormat;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use walkdir::{DirEntry, WalkDir};

const MAX_PORTFOLIO_DEPTH: usize = 64;
const MAX_PORTFOLIO_ENTRIES: usize = 100_000;
const MAX_PORTFOLIO_PROJECTS: usize = 1_000;

#[derive(Debug, Clone, Copy)]
pub enum CreativeMode {
    Direct,
    Reshoot,
}

#[derive(Debug, Serialize)]
pub struct CreativeReport {
    pub mode: &'static str,
    pub portfolio: bool,
    pub root: PathBuf,
    pub projects: Vec<ProjectReport>,
}

#[derive(Debug, Serialize)]
pub struct ProjectReport {
    pub project: PathBuf,
    pub actions: Vec<String>,
    pub cache_hit: bool,
    pub error: Option<String>,
}

#[derive(Debug)]
struct ProjectRun {
    actions: Vec<String>,
    cache_hit: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct PadagoniaCreativeCache {
    schema_version: String,
    project_id: String,
    project_root: PathBuf,
    evidence_digest: String,
    direct_fingerprint: Option<String>,
    reshoot_fingerprint: Option<String>,
    reshoot_files: Vec<CachedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedFile {
    path: PathBuf,
    kind: String,
    bytes: u64,
    digest: String,
}

#[derive(Debug)]
struct PadagoniaKnowledge {
    project_id: String,
    evidence_digest: String,
}

impl CreativeReport {
    pub fn failed(&self) -> usize {
        self.projects
            .iter()
            .filter(|item| item.error.is_some())
            .count()
    }

    pub fn render_human(&self) -> String {
        let mut out = format!(
            "Brandi {} — {} project{}\n",
            self.mode,
            self.projects.len(),
            if self.projects.len() == 1 { "" } else { "s" }
        );
        for project in &self.projects {
            out.push_str(&format!("\n{}\n", project.project.display()));
            if let Some(error) = &project.error {
                out.push_str(&format!("  failed: {error}\n"));
                continue;
            }
            if project.cache_hit {
                out.push_str("  Padagonia cache hit; unchanged work skipped\n");
            }
            if project.actions.is_empty() {
                out.push_str("  already complete; no changes\n");
            } else {
                for action in &project.actions {
                    out.push_str(&format!("  {action}\n"));
                }
            }
        }
        out.push_str(&format!(
            "\nCompleted: {}  Failed: {}\n",
            self.projects.len().saturating_sub(self.failed()),
            self.failed()
        ));
        out
    }
}

/// Run one creative command against one project or every compliant project
/// below a bounded portfolio root. In portfolio mode an omitted path means
/// the user's home; otherwise an omitted path means the current directory.
pub fn run(mode: CreativeMode, path: Option<&Path>, portfolio: bool) -> Result<CreativeReport> {
    let root = resolve_root(path, portfolio)?;
    let projects = if portfolio {
        discover_projects(&root)?
    } else {
        vec![root.clone()]
    };
    if portfolio && projects.is_empty() {
        return Err(BrandiError::NotFound(format!(
            "no Brandi-compliant projects found beneath {}",
            root.display()
        )));
    }

    let mut reports = Vec::with_capacity(projects.len());
    for project in projects {
        let result = if portfolio {
            run_cached_project(mode, &project)
        } else {
            match mode {
                CreativeMode::Direct => direct_project(&project).map(|actions| ProjectRun {
                    actions,
                    cache_hit: false,
                }),
                CreativeMode::Reshoot => {
                    reshoot_project(&project, &HashSet::new()).map(|actions| ProjectRun {
                        actions,
                        cache_hit: false,
                    })
                }
            }
        };
        reports.push(match result {
            Ok(run) => ProjectReport {
                project,
                actions: run.actions,
                cache_hit: run.cache_hit,
                error: None,
            },
            Err(error) => ProjectReport {
                project,
                actions: Vec::new(),
                cache_hit: false,
                error: Some(error.to_string()),
            },
        });
    }
    Ok(CreativeReport {
        mode: match mode {
            CreativeMode::Direct => "direct",
            CreativeMode::Reshoot => "reshoot",
        },
        portfolio,
        root,
        projects: reports,
    })
}

pub(crate) fn resolve_root(path: Option<&Path>, portfolio: bool) -> Result<PathBuf> {
    let root = match path {
        Some(path) => path.to_path_buf(),
        None if portfolio => std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| BrandiError::NotFound("HOME is not set; pass --path".into()))?,
        None => PathBuf::from("."),
    };
    if !root.is_dir() {
        return Err(BrandiError::NotFound(format!(
            "project or portfolio root is not a directory: {}",
            root.display()
        )));
    }
    Ok(root)
}

pub fn discover_projects(root: &Path) -> Result<Vec<PathBuf>> {
    let mut projects = Vec::new();
    let mut visited = 0usize;
    for entry in WalkDir::new(root)
        .max_depth(MAX_PORTFOLIO_DEPTH)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| portfolio_entry(root, entry))
    {
        let entry = entry.map_err(|error| {
            BrandiError::Io(
                error
                    .into_io_error()
                    .unwrap_or_else(|| std::io::Error::other("portfolio traversal failed")),
            )
        })?;
        visited += 1;
        if visited > MAX_PORTFOLIO_ENTRIES {
            return Err(BrandiError::Invalid(format!(
                "portfolio traversal exceeded {MAX_PORTFOLIO_ENTRIES} entries"
            )));
        }
        if !entry.file_type().is_dir() || !entry.path().join(".brandi").is_dir() {
            continue;
        }
        if Brief::validate(entry.path()).is_ok()
            && Brief::load(entry.path()).is_ok()
            && Guidelines::validate(entry.path()).is_ok()
            && Guidelines::load(entry.path()).is_ok()
        {
            projects.push(entry.path().to_path_buf());
            if projects.len() > MAX_PORTFOLIO_PROJECTS {
                return Err(BrandiError::Invalid(format!(
                    "portfolio contains more than {MAX_PORTFOLIO_PROJECTS} compliant projects"
                )));
            }
        }
    }
    projects.sort();
    projects.dedup();
    Ok(projects)
}

fn portfolio_entry(root: &Path, entry: &DirEntry) -> bool {
    if entry.path() == root {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    if name.starts_with('.') {
        return false;
    }
    !matches!(
        name.as_ref(),
        "target" | "node_modules" | "vendor" | "dist" | "build" | "__pycache__"
    )
}

fn run_cached_project(mode: CreativeMode, root: &Path) -> Result<ProjectRun> {
    let knowledge = padagonia_knowledge(root)?;
    let current_files = relevant_inventory(root)?;
    let current_fingerprint = project_fingerprint(root, &knowledge, &current_files)?;
    let mut cache = load_creative_cache(root).unwrap_or_default();
    let cached_fingerprint = match mode {
        CreativeMode::Direct => cache.direct_fingerprint.as_deref(),
        CreativeMode::Reshoot => cache.reshoot_fingerprint.as_deref(),
    };
    if cache.schema_version == "brandi-padagonia-creative-v1"
        && cache.project_id == knowledge.project_id
        && cache.evidence_digest == knowledge.evidence_digest
        && cached_fingerprint == Some(current_fingerprint.as_str())
    {
        return Ok(ProjectRun {
            actions: Vec::new(),
            cache_hit: true,
        });
    }

    let mut actions = Vec::new();
    if !cache.project_root.as_os_str().is_empty() && cache.project_root != root {
        actions.push(format!(
            "remapped cached project root: {} -> {}",
            cache.project_root.display(),
            root.display()
        ));
    }
    let (unchanged, relocations) = if matches!(mode, CreativeMode::Reshoot) {
        remap_cached_files(root, &cache.reshoot_files, &current_files)
    } else {
        (HashSet::new(), Vec::new())
    };
    actions.extend(relocations);
    let mut mode_actions = match mode {
        CreativeMode::Direct => direct_project(root)?,
        CreativeMode::Reshoot => reshoot_project(root, &unchanged)?,
    };
    actions.append(&mut mode_actions);

    let updated_files = relevant_inventory(root)?;
    let updated_fingerprint = project_fingerprint(root, &knowledge, &updated_files)?;
    cache.schema_version = "brandi-padagonia-creative-v1".into();
    cache.project_id = knowledge.project_id;
    cache.project_root = root.to_path_buf();
    cache.evidence_digest = knowledge.evidence_digest;
    match mode {
        CreativeMode::Direct => cache.direct_fingerprint = Some(updated_fingerprint),
        CreativeMode::Reshoot => {
            cache.reshoot_fingerprint = Some(updated_fingerprint);
            cache.reshoot_files = updated_files;
        }
    }
    if let Err(error) = save_creative_cache(root, &cache) {
        actions.push(format!("Padagonia cache update skipped: {error}"));
    }
    Ok(ProjectRun {
        actions,
        cache_hit: false,
    })
}

fn cache_path(root: &Path) -> PathBuf {
    root.join(".brandi/state/padagonia-creative-cache.json")
}

fn load_creative_cache(root: &Path) -> Option<PadagoniaCreativeCache> {
    let bytes = fs::read(cache_path(root)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn save_creative_cache(root: &Path, cache: &PadagoniaCreativeCache) -> Result<()> {
    let path = cache_path(root);
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if !metadata.file_type().is_file() {
            return Err(BrandiError::Invalid(format!(
                "refusing to replace non-regular cache {}",
                path.display()
            )));
        }
    }
    let parent = path
        .parent()
        .ok_or_else(|| BrandiError::Invalid("cache path has no parent".into()))?;
    fs::create_dir_all(parent)?;
    let temporary = temporary_path(&path, "json")?;
    fs::write(&temporary, serde_json::to_vec_pretty(cache)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn padagonia_knowledge(root: &Path) -> Result<PadagoniaKnowledge> {
    let project_id = crate::automation::PromotionConfig::load(root)?.project_id;
    let outbox = root.join(".brandi/state/padagonia-outbox.jsonl");
    let latest = fs::read_to_string(outbox)
        .ok()
        .and_then(|contents| {
            contents
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "no-padagonia-evidence".into());
    let mut digest = Sha256::new();
    digest.update(latest.as_bytes());
    Ok(PadagoniaKnowledge {
        project_id,
        evidence_digest: format!("sha256:{:x}", digest.finalize()),
    })
}

fn relevant_inventory(root: &Path) -> Result<Vec<CachedFile>> {
    let mut files = Vec::new();
    let mut visited = 0usize;
    let walker = WalkDir::new(root)
        .max_depth(MAX_PORTFOLIO_DEPTH)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.path() == root
                || !crate::surface::is_ignored(
                    entry.path().strip_prefix(root).unwrap_or(entry.path()),
                )
        });
    for entry in walker {
        let entry = entry.map_err(|error| {
            BrandiError::Io(
                error
                    .into_io_error()
                    .unwrap_or_else(|| std::io::Error::other("portfolio inventory failed")),
            )
        })?;
        visited += 1;
        if visited > MAX_PORTFOLIO_ENTRIES {
            return Err(BrandiError::Invalid(format!(
                "portfolio inventory exceeded {MAX_PORTFOLIO_ENTRIES} entries"
            )));
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let extension = path
            .extension()
            .and_then(OsStr::to_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let kind = if matches!(
            extension.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg"
        ) {
            "visual"
        } else if matches!(extension.as_str(), "css" | "scss") {
            "style"
        } else if matches!(
            crate::surface::classify(path, root),
            Some(SurfaceKind::RepoDoc | SurfaceKind::Doc)
        ) {
            "document"
        } else if extension == "tape" {
            "tape"
        } else {
            continue;
        };
        let metadata = fs::metadata(path)?;
        files.push(CachedFile {
            path: path.strip_prefix(root).unwrap_or(path).to_path_buf(),
            kind: kind.into(),
            bytes: metadata.len(),
            digest: sha256_file(path)?,
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn project_fingerprint(
    root: &Path,
    knowledge: &PadagoniaKnowledge,
    files: &[CachedFile],
) -> Result<String> {
    let mut digest = Sha256::new();
    digest.update(knowledge.project_id.as_bytes());
    digest.update(knowledge.evidence_digest.as_bytes());
    for name in [
        "identity.yaml",
        "voice.yaml",
        "visual.yaml",
        "audience.yaml",
        "prohibited.yaml",
        "rules.yaml",
    ] {
        let path = root.join(".brandi").join(name);
        if path.is_file() {
            digest.update(name.as_bytes());
            digest.update(fs::read(path)?);
        }
    }
    for file in files {
        digest.update(file.path.to_string_lossy().as_bytes());
        digest.update(file.kind.as_bytes());
        digest.update(file.bytes.to_le_bytes());
        digest.update(file.digest.as_bytes());
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn remap_cached_files(
    root: &Path,
    cached: &[CachedFile],
    current: &[CachedFile],
) -> (HashSet<PathBuf>, Vec<String>) {
    let by_path: BTreeMap<&Path, &CachedFile> = current
        .iter()
        .map(|file| (file.path.as_path(), file))
        .collect();
    let mut unchanged = HashSet::new();
    let mut claimed = HashSet::new();
    let mut actions = Vec::new();
    for old in cached {
        if let Some(now) = by_path.get(old.path.as_path()) {
            if old.digest == now.digest {
                unchanged.insert(root.join(&now.path));
                claimed.insert(now.path.clone());
            }
            continue;
        }
        let matches = current
            .iter()
            .filter(|now| {
                !claimed.contains(&now.path)
                    && old.kind == now.kind
                    && old.bytes == now.bytes
                    && old.digest == now.digest
            })
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            let now = matches[0];
            unchanged.insert(root.join(&now.path));
            claimed.insert(now.path.clone());
            actions.push(format!(
                "remapped cached file: {} -> {}",
                old.path.display(),
                now.path.display()
            ));
        }
    }
    (unchanged, actions)
}

fn sha256_file(path: &Path) -> Result<String> {
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

fn direct_project(root: &Path) -> Result<Vec<String>> {
    Brief::validate(root)?;
    Guidelines::validate(root)?;
    let brief = Brief::load(root)?;
    let guidelines = Guidelines::load(root)?;
    let detected = assets::detect_assets(root)?;
    let mut actions = Vec::new();

    let targets = [
        (
            AssetClass::SocialCard,
            "social card",
            &guidelines.visual.assets.social_card,
            &guidelines.visual.assets.social_card_path,
        ),
        (
            AssetClass::Thumbnail,
            "thumbnail",
            &guidelines.visual.assets.thumbnail,
            &guidelines.visual.assets.thumbnail_path,
        ),
    ];
    for (class, label, dimensions, relative) in targets {
        if detected.iter().any(|asset| asset.class == class) {
            continue;
        }
        let output = confined_output(root, relative, "visual asset")?;
        if output.extension() != Some(OsStr::new("svg")) {
            return Err(BrandiError::Invalid(format!(
                "direct output {} must use .svg because Vasilis is the deterministic SVG provider",
                relative.display()
            )));
        }
        generate_visual(&brief, &guidelines, label, dimensions, &output)?;
        actions.push(format!("generated {label}: {}", relative.display()));
    }

    let wants_demo = brief
        .audience
        .narratives
        .iter()
        .flat_map(|narrative| narrative.formats.iter())
        .any(|format| matches!(format.as_str(), "demo_video" | "demo" | "video"));
    if wants_demo && !has_demo_asset(root)? {
        let slug = slugify(&brief.identity.product.name);
        let relative = PathBuf::from(format!("demos/{slug}-demo.tape"));
        let draft = tape::generate(
            root,
            TapeRequest {
                goal: format!(
                    "Show how {} delivers {}",
                    brief.identity.product.name, brief.identity.product.mission
                ),
                preset: "product-tour".into(),
                slug: format!("{slug}-demo"),
                output: format!("demos/{slug}-demo.gif"),
            },
        )?;
        let validation = tape::save_validated(root, &draft, &relative, false)?;
        if !validation.valid {
            return Err(BrandiError::Invalid(
                "VHS rejected the generated demo tape".into(),
            ));
        }
        actions.push(format!("generated VHS demo plan: {}", relative.display()));
    }
    Ok(actions)
}

fn generate_visual(
    brief: &Brief,
    guidelines: &Guidelines,
    label: &str,
    dimensions: &Dimensions,
    output: &Path,
) -> Result<()> {
    if output.exists() {
        return Ok(());
    }
    let parent = output
        .parent()
        .ok_or_else(|| BrandiError::Invalid("visual output has no parent directory".into()))?;
    fs::create_dir_all(parent)?;
    let requirement = visual_requirement(brief, guidelines, label, dimensions);
    let requirement_path = temporary_path(output, "requirement.json")?;
    fs::write(&requirement_path, serde_json::to_vec_pretty(&requirement)?)?;
    let temporary = temporary_path(output, "svg")?;
    let result = run_bounded(
        Command::new("vasilis")
            .arg("generate")
            .arg("--requirement")
            .arg(&requirement_path)
            .arg("--output")
            .arg(&temporary),
        DEFAULT_SUBPROCESS_TIMEOUT,
        MAX_SUBPROCESS_OUTPUT_BYTES,
    );
    let _ = fs::remove_file(&requirement_path);
    let result = result.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            BrandiError::NotFound(
                "vasilis is required for `brandi direct`; install it on PATH".into(),
            )
        } else {
            BrandiError::Io(error)
        }
    })?;
    if result.timed_out || !result.status.success() {
        let _ = fs::remove_file(&temporary);
        return Err(BrandiError::Invalid(format!(
            "Vasilis could not generate {}: {}",
            output.display(),
            String::from_utf8_lossy(&result.stderr).trim()
        )));
    }
    if let Err(error) = fs::hard_link(&temporary, output) {
        let _ = fs::remove_file(&temporary);
        return if error.kind() == std::io::ErrorKind::AlreadyExists {
            Err(BrandiError::Invalid(format!(
                "refusing to overwrite asset created concurrently: {}",
                output.display()
            )))
        } else {
            Err(error.into())
        };
    }
    fs::remove_file(&temporary)?;
    Ok(())
}

fn visual_requirement(
    brief: &Brief,
    guidelines: &Guidelines,
    label: &str,
    dimensions: &Dimensions,
) -> serde_json::Value {
    let colors = std::iter::once(&guidelines.visual.palette.primary)
        .chain(guidelines.visual.palette.secondary.iter())
        .chain(guidelines.visual.palette.neutrals.iter())
        .cloned()
        .collect::<Vec<_>>();
    let product_slug = slugify(&brief.identity.product.name);
    // "observed": these values were read from a config file (.brandi/visual.yaml)
    // a human authored, not requested live nor inferred.
    let sourced = |value: &str| serde_json::json!({"value": value, "source": "observed"});
    let visual_language = &guidelines.visual.visual_language;
    serde_json::json!({
        "id": format!("{product_slug}-{}", label.replace(' ', "-")),
        "revision": 1,
        "namespace": {
            "tenant": "local",
            "project": product_slug,
            "application": "brandi-direct"
        },
        "source": "brandi",
        "semantic_role": format!("Present {} as the branded {label}", brief.identity.product.name),
        "asset_type": if label == "social card" { "social_preview" } else { "illustration" },
        "surface": {
            "name": format!("brand.{}", label.replace(' ', "_")),
            "width": dimensions.width,
            "height": dimensions.height,
            "theme": "any"
        },
        "purpose": format!("{} {} — {}", brief.identity.product.tagline, brief.identity.product.mission, label),
        "constraints": {
            "required_format": "svg",
            "max_bytes": u64::from(guidelines.visual.assets.max_file_kb) * 1024,
            "max_pixels": u64::from(dimensions.width) * u64::from(dimensions.height),
            "transparent_background": false
        },
        "priority": "normal",
        "brand_context": {
            "id": format!("{product_slug}-brand"),
            "revision": 1,
            "palette": colors,
            "visual_language": {
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
            "forbidden_motifs": guidelines.prohibited.visual_motifs
        },
        "required_variants": ["master"],
        "target_context": crate::direct::target_context(&brief.audience)
    })
}

fn has_demo_asset(root: &Path) -> Result<bool> {
    let demos = root.join("demos");
    if !demos.is_dir() {
        return Ok(false);
    }
    for entry in fs::read_dir(demos)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let extension = entry
            .path()
            .extension()
            .and_then(OsStr::to_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if matches!(extension.as_str(), "gif" | "mp4" | "webm" | "mov" | "tape") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn reshoot_project(root: &Path, unchanged: &HashSet<PathBuf>) -> Result<Vec<String>> {
    Brief::validate(root)?;
    Guidelines::validate(root)?;
    let brief = Brief::load(root)?;
    let guidelines = Guidelines::load(root)?;
    let mut actions = revise_text_surfaces(root, &brief, &guidelines, unchanged)?;
    let assets = assets::detect_assets(root)?;
    for asset in assets {
        let path = root.join(&asset.path);
        if unchanged.contains(&path) {
            continue;
        }
        if revise_image(&path, &guidelines)? {
            actions.push(format!("revised visual asset: {}", asset.path));
        }
    }
    Ok(actions)
}

fn revise_text_surfaces(
    root: &Path,
    brief: &Brief,
    guidelines: &Guidelines,
    unchanged: &HashSet<PathBuf>,
) -> Result<Vec<String>> {
    let proposals = propose::propose(brief, guidelines, root, usize::MAX)?;
    let mut by_path: BTreeMap<PathBuf, Vec<_>> = BTreeMap::new();
    for proposal in proposals {
        if unchanged.contains(&proposal.path) {
            continue;
        }
        let is_document = matches!(proposal.kind, SurfaceKind::RepoDoc | SurfaceKind::Doc);
        let is_stylesheet = proposal.kind == SurfaceKind::Style
            && proposal
                .path
                .extension()
                .and_then(OsStr::to_str)
                .is_some_and(|extension| matches!(extension, "css" | "scss"));
        if (is_document || is_stylesheet)
            && proposal.start_byte.is_some()
            && proposal.end_byte.is_some()
        {
            by_path
                .entry(proposal.path.clone())
                .or_default()
                .push(proposal);
        }
    }

    let mut actions = Vec::new();
    for (path, mut proposals) in by_path {
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_file() {
            return Err(BrandiError::Invalid(format!(
                "refusing to revise non-regular text asset {}",
                path.display()
            )));
        }
        let original = fs::read_to_string(&path)?;
        proposals.sort_by_key(|proposal| std::cmp::Reverse(proposal.start_byte.unwrap_or(0)));
        let mut revised = original.clone();
        let mut occupied = HashSet::new();
        let mut count = 0usize;
        for proposal in proposals {
            let (start, end) = (
                proposal.start_byte.unwrap_or(0),
                proposal.end_byte.unwrap_or(0),
            );
            if start > end
                || end > original.len()
                || !original.is_char_boundary(start)
                || !original.is_char_boundary(end)
            {
                continue;
            }
            if (start..end).any(|offset| occupied.contains(&offset)) {
                continue;
            }
            revised.replace_range(start..end, &proposal.revised);
            occupied.extend(start..end);
            count += 1;
        }
        if revised != original {
            atomic_write(&path, revised.as_bytes(), &metadata)?;
            let relative = path.strip_prefix(root).unwrap_or(&path);
            actions.push(format!(
                "revised {count} non-code text surface{} in {}",
                if count == 1 { "" } else { "s" },
                relative.display()
            ));
        }
    }
    Ok(actions)
}

fn revise_image(path: &Path, guidelines: &Guidelines) -> Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(BrandiError::Invalid(format!(
            "refusing to revise non-regular visual asset {}",
            path.display()
        )));
    }
    let palette = guidelines.visual.palette.all_colors();
    if palette.is_empty() {
        return Err(BrandiError::Invalid(
            "visual palette has no valid colors".into(),
        ));
    }
    let extension = path
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "svg" {
        let original = fs::read_to_string(path)?;
        let color = Regex::new(r"(?i)#[0-9a-f]{6}\b")
            .map_err(|error| BrandiError::Invalid(error.to_string()))?;
        let revised = color
            .replace_all(&original, |captures: &regex::Captures| {
                nearest_color(captures.get(0).unwrap().as_str(), &palette)
            })
            .into_owned();
        if revised != original {
            atomic_write(path, revised.as_bytes(), &metadata)?;
            return Ok(true);
        }
        return Ok(false);
    }

    let format = ImageFormat::from_extension(&extension).ok_or_else(|| {
        BrandiError::Invalid(format!("unsupported visual asset format: {extension}"))
    })?;
    let image = image::open(path)?;
    let mut pixels = image.to_rgba8();
    let mut changed = false;
    for pixel in pixels.pixels_mut() {
        if pixel[3] == 0 {
            continue;
        }
        let nearest = palette
            .iter()
            .min_by_key(|color| color_distance((pixel[0], pixel[1], pixel[2]), **color))
            .copied()
            .unwrap();
        let next = [nearest.0, nearest.1, nearest.2, pixel[3]];
        changed |= pixel.0 != next;
        pixel.0 = next;
    }
    if changed {
        let temporary = temporary_path(path, &extension)?;
        pixels.save_with_format(&temporary, format)?;
        fs::set_permissions(&temporary, metadata.permissions())?;
        fs::rename(&temporary, path)?;
    }
    Ok(changed)
}

fn nearest_color(raw: &str, palette: &[(u8, u8, u8)]) -> String {
    let source = parse_hex_color(raw).unwrap_or(palette[0]);
    let nearest = palette
        .iter()
        .min_by_key(|color| color_distance(source, **color))
        .copied()
        .unwrap_or(palette[0]);
    format!("#{:02X}{:02X}{:02X}", nearest.0, nearest.1, nearest.2)
}

fn color_distance(left: (u8, u8, u8), right: (u8, u8, u8)) -> u32 {
    u32::from(left.0.abs_diff(right.0)).pow(2)
        + u32::from(left.1.abs_diff(right.1)).pow(2)
        + u32::from(left.2.abs_diff(right.2)).pow(2)
}

fn confined_output(root: &Path, relative: &Path, label: &str) -> Result<PathBuf> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(BrandiError::Invalid(format!(
            "{label} path must stay within the project: {}",
            relative.display()
        )));
    }
    Ok(root.join(relative))
}

fn temporary_path(path: &Path, extension: &str) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| BrandiError::Invalid("output has no parent directory".into()))?;
    let stem = path.file_stem().and_then(OsStr::to_str).unwrap_or("asset");
    for attempt in 0..32u8 {
        let candidate = parent.join(format!(
            ".{stem}.brandi-{}-{attempt}.{extension}",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => {
                fs::remove_file(&candidate)?;
                return Ok(candidate);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(BrandiError::Io(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not reserve a temporary asset path",
    )))
}

fn atomic_write(path: &Path, bytes: &[u8], metadata: &fs::Metadata) -> Result<()> {
    let extension = path.extension().and_then(OsStr::to_str).unwrap_or("tmp");
    let temporary = temporary_path(path, extension)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::set_permissions(&temporary, metadata.permissions())?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn slugify(value: &str) -> String {
    let slug = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "brand".into()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portfolio_discovers_only_valid_projects_and_skips_hidden_projects() {
        let temporary = tempfile::tempdir().unwrap();
        let valid = temporary.path().join("valid");
        let hidden = temporary.path().join(".hidden");
        let broken = temporary.path().join("broken");
        fs::create_dir_all(&valid).unwrap();
        fs::create_dir_all(&hidden).unwrap();
        fs::create_dir_all(broken.join(".brandi")).unwrap();
        Brief::scaffold(&valid).unwrap();
        Guidelines::scaffold(&valid).unwrap();
        Brief::scaffold(&hidden).unwrap();
        Guidelines::scaffold(&hidden).unwrap();

        assert_eq!(discover_projects(temporary.path()).unwrap(), vec![valid]);
    }

    #[test]
    fn reshoot_changes_docs_and_images_but_not_source_strings() {
        let temporary = tempfile::tempdir().unwrap();
        Brief::scaffold(temporary.path()).unwrap();
        Guidelines::scaffold(temporary.path()).unwrap();
        fs::write(
            temporary.path().join("README.md"),
            "# Sample\n\nThis awesome tool helps teams.\n",
        )
        .unwrap();
        fs::write(
            temporary.path().join("main.rs"),
            "fn main() { println!(\"This awesome tool helps teams.\"); }\n",
        )
        .unwrap();
        let image_path = temporary.path().join("logo.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 255, 0, 255]))
            .save(&image_path)
            .unwrap();

        let actions = reshoot_project(temporary.path(), &HashSet::new()).unwrap();
        assert!(fs::read_to_string(temporary.path().join("README.md"))
            .unwrap()
            .contains("useful"));
        assert!(fs::read_to_string(temporary.path().join("main.rs"))
            .unwrap()
            .contains("awesome"));
        assert_eq!(
            image::open(image_path)
                .unwrap()
                .to_rgba8()
                .get_pixel(0, 0)
                .0,
            [111, 91, 104, 255]
        );
        assert!(!actions.is_empty());
    }

    #[test]
    fn visual_requirement_reflects_guideline_config_not_hardcoded_literals() {
        let temporary = tempfile::tempdir().unwrap();
        Brief::scaffold(temporary.path()).unwrap();
        Guidelines::scaffold(temporary.path()).unwrap();
        let brief = Brief::load(temporary.path()).unwrap();
        let default_guidelines = Guidelines::load(temporary.path()).unwrap();
        let dimensions = Dimensions {
            width: 1200,
            height: 630,
        };
        let default_requirement =
            visual_requirement(&brief, &default_guidelines, "social card", &dimensions);

        fs::write(
            temporary.path().join(".brandi/visual.yaml"),
            "visual_language:\n  geometry: organic\n  complexity: intricate\n  density: dense\n  contrast: low\n  corner_language: rounded\n  lighting: dramatic\n  depth: high\n  texture: tactile\n  illustration_style: hand-drawn\n",
        )
        .unwrap();
        fs::write(
            temporary.path().join(".brandi/prohibited.yaml"),
            "visual_motifs: [\"gradients\", \"photorealistic people\"]\n",
        )
        .unwrap();
        let custom_guidelines = Guidelines::load(temporary.path()).unwrap();
        let custom_requirement =
            visual_requirement(&brief, &custom_guidelines, "social card", &dimensions);

        let custom_language = &custom_requirement["brand_context"]["visual_language"];
        assert_eq!(custom_language["geometry"]["value"], "organic");
        assert_eq!(custom_language["geometry"]["source"], "observed");
        assert_eq!(custom_language["illustration_style"]["value"], "hand-drawn");
        assert_eq!(
            custom_requirement["brand_context"]["forbidden_motifs"],
            serde_json::json!(["gradients", "photorealistic people"])
        );

        let default_language = &default_requirement["brand_context"]["visual_language"];
        assert_eq!(default_language["geometry"]["value"], "geometric");
        assert_eq!(
            default_requirement["brand_context"]["forbidden_motifs"],
            serde_json::json!([])
        );

        assert_ne!(
            custom_requirement["brand_context"]["visual_language"],
            default_requirement["brand_context"]["visual_language"]
        );
        assert_ne!(
            custom_requirement["brand_context"]["forbidden_motifs"],
            default_requirement["brand_context"]["forbidden_motifs"]
        );
    }

    #[test]
    fn configured_outputs_cannot_escape_the_project() {
        let temporary = tempfile::tempdir().unwrap();
        assert!(confined_output(temporary.path(), Path::new("../outside.svg"), "asset").is_err());
        assert!(confined_output(temporary.path(), Path::new("assets/card.svg"), "asset").is_ok());
    }
}
