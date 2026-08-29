//! Surface discovery: finds user-facing surfaces (docs, UI/CLI strings,
//! styles) in a project tree and classifies them.

use crate::error::{BrandiError, Result};
use crate::types::{
    ExposureAudience, ExposureProfile, ExposureReach, ExposureVisibility, ScanDiagnostic,
    ScanResult, Surface, SurfaceDomain, SurfaceKind, SurfaceProvenance, SurfaceSignificance,
    SurfaceSubtype,
};
use regex::Regex;
use std::cell::RefCell;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use tree_sitter::{Language, Node, Parser};
use walkdir::WalkDir;

pub const EXTRACTION_SCHEMA_VERSION: &str = "brandi-extraction-v4";

/// Path components that are never user-facing surfaces: VCS internals, build
/// output, dependencies, IDE config, and brandi's own brief/guidelines directory.
const IGNORED_COMPONENTS: [&str; 16] = [
    ".git",
    "target",
    "node_modules",
    ".brandi",
    "dist",
    "build",
    ".idea",
    ".vscode",
    "__pycache__",
    "vendor",
    ".next",
    "coverage",
    "testdata",
    "fixtures",
    "benches",
    "memory",
];

/// Hard scan budgets prevent a single repository or generated file from
/// consuming unbounded memory or filesystem traversal time.
pub const MAX_SOURCE_FILE_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_SCAN_FILES: usize = 100_000;
pub const MAX_SCAN_DEPTH: usize = 64;

/// Files larger or longer than these thresholds skip the Tree-sitter AST
/// parser and fall back to the regex extractor. This prevents pathological
/// parse times on generated/vendored megabyte-scale sources.
pub const MAX_AST_SOURCE_BYTES: u64 = 256 * 1024;
pub const MAX_AST_SOURCE_LINES: usize = 5000;

/// Well-known repo documents, compared case-insensitively against the
/// basename of files sitting directly under the project root.
const REPO_DOC_NAMES: [&str; 9] = [
    "readme.md",
    "contributing.md",
    "changelog.md",
    "changes.md",
    "license",
    "license.md",
    "license.txt",
    "code_of_conduct.md",
    "security.md",
];

/// Double-quoted string literals of at least 4 chars without escapes or
/// newlines inside; the capture group is the literal body.
fn ui_string_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#""([^"\\\n]{4,})""#).expect("static UI-string regex must compile")
    })
}

/// Pure lowercase identifier / path / format-key shape, e.g.
/// `"application/json"`, `"some.key"`, `"camel_case"`.
fn identifier_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^[a-z0-9_\-./:{}%]+$").expect("static identifier regex must compile")
    })
}

/// A `#rrggbb` hex color; `\b` keeps 8-digit `#rrggbbaa` values from matching.
fn hex_color_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"#[0-9a-fA-F]{6}\b").expect("static hex-color regex must compile")
    })
}

/// Returns true if a project-relative path should be excluded from scans
/// (e.g. `.git/`, `target/`, `node_modules/`, lockfiles).
pub fn is_ignored(rel_path: &Path) -> bool {
    rel_path.components().any(|component| match component {
        Component::Normal(name) => name
            .to_str()
            .is_some_and(|name| IGNORED_COMPONENTS.contains(&name)),
        _ => false,
    })
}

/// Load simple path patterns from `<root>/.brandiignore`, one per line
/// (blank lines and `#` comments skipped, a trailing `/` stripped). A
/// project's own brief/guidelines scaffolding or test fixtures that
/// intentionally quote prohibited example words — the default
/// `prohibited.yaml` template embedded in `guidelines.rs` is exactly this
/// case — have no other way to avoid tripping brandi's own lint rules
/// against itself; missing or
/// unreadable is just "no patterns", not an error.
fn load_ignore_patterns(root: &Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(root.join(".brandiignore")) else {
        return Vec::new();
    };
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.trim_end_matches('/').to_string())
        .collect()
}

/// True when `rel_path` matches one of `patterns`: an exact relative-path
/// match (`tui/brandi_test.go`), or the pattern names any path component
/// (a bare directory name like `fixtures` ignores that whole subtree).
fn matches_ignore_pattern(rel_path: &Path, patterns: &[String]) -> bool {
    if patterns.is_empty() {
        return false;
    }
    let rel_str = rel_path.to_string_lossy();
    patterns.iter().any(|pattern| {
        rel_str == pattern.as_str()
            || rel_str.starts_with(&format!("{pattern}/"))
            || rel_path
                .components()
                .any(|c| c.as_os_str().to_str() == Some(pattern.as_str()))
    })
}

/// Classify a file into a `SurfaceKind` from its path relative to the project
/// root; returns `None` for files that are not user-facing surfaces.
pub fn classify(path: &Path, root: &Path) -> Option<SurfaceKind> {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let file_name = rel.file_name()?.to_string_lossy().to_lowercase();
    let ext = rel
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    // RepoDoc: well-known documents directly under the project root...
    let directly_under_root = rel
        .parent()
        .is_some_and(|parent| parent.as_os_str().is_empty());
    if directly_under_root && REPO_DOC_NAMES.contains(&file_name.as_str()) {
        return Some(SurfaceKind::RepoDoc);
    }
    // ...plus Markdown files under a top-level `.github/` directory
    // (issue / PR templates), which are repo documents too.
    let under_github = matches!(
        rel.components().next(),
        Some(Component::Normal(first)) if first == ".github"
    );
    if under_github && ext == "md" {
        return Some(SurfaceKind::RepoDoc);
    }

    // Doc: any other Markdown / reStructuredText file (including `docs/`).
    if matches!(ext.as_str(), "md" | "markdown" | "rst") {
        return Some(SurfaceKind::Doc);
    }
    // UiString: source files that may hold user-facing string literals.
    if matches!(
        ext.as_str(),
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go"
    ) {
        return Some(SurfaceKind::UiString);
    }
    // Style: stylesheets.
    if matches!(ext.as_str(), "css" | "scss") {
        return Some(SurfaceKind::Style);
    }
    None
}

/// Walk `root`, preserving every exclusion or failure as a machine-readable
/// diagnostic. Only parse/read failures affect completeness; intentional
/// policy exclusions remain auditable without making the scan fail.
pub fn scan_project(root: &Path) -> Result<ScanResult> {
    let mut surfaces = Vec::new();
    let mut diagnostics = Vec::new();
    let patterns = load_ignore_patterns(root);
    let walker = WalkDir::new(root).max_depth(MAX_SCAN_DEPTH).into_iter();
    let mut scanned_files = 0usize;
    let mut successfully_scanned_files = 0usize;
    let excluded_paths = RefCell::new(Vec::<PathBuf>::new());
    for entry_result in walker.filter_entry(|entry| {
        let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
        let keep = !is_ignored(rel) && !matches_ignore_pattern(rel, &patterns);
        if !keep {
            excluded_paths.borrow_mut().push(rel.to_path_buf());
        }
        keep
    }) {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(error) => {
                diagnostics.push(ScanDiagnostic {
                    path: error.path().unwrap_or(root).to_path_buf(),
                    code: "walk-error".into(),
                    message: error.to_string(),
                    affects_completeness: true,
                });
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        scanned_files += 1;
        if scanned_files > MAX_SCAN_FILES {
            return Err(BrandiError::Invalid(format!(
                "scan exceeded the {MAX_SCAN_FILES} file safety limit"
            )));
        }
        let path = entry.path();
        let Some(kind) = classify(path, root) else {
            continue;
        };
        let rel = path.strip_prefix(root).unwrap_or(path);
        if is_test_source(path) && kind != SurfaceKind::UiString {
            diagnostics.push(skip_diagnostic(
                rel,
                "test-nonassertion-corpus",
                "non-source test corpus excluded; source assertion strings remain eligible",
            ));
            continue;
        }
        if is_generated_source(path) {
            diagnostics.push(skip_diagnostic(
                rel,
                "generated-code",
                "generated source excluded by policy",
            ));
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) => {
                diagnostics.push(failure_diagnostic(rel, "metadata-error", error.to_string()));
                continue;
            }
        };
        if metadata.len() > MAX_SOURCE_FILE_BYTES {
            diagnostics.push(failure_diagnostic(
                rel,
                "oversized-source",
                format!("file exceeds the {MAX_SOURCE_FILE_BYTES} byte source limit"),
            ));
            continue;
        }
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) => {
                diagnostics.push(failure_diagnostic(rel, "read-error", error.to_string()));
                continue;
            }
        };
        if kind == SurfaceKind::UiString && is_generated_content(&content) {
            diagnostics.push(skip_diagnostic(
                rel,
                "generated-code",
                "generated source excluded by content header",
            ));
            continue;
        }
        let (mut extracted, parse_error) = surfaces_in_text(path, kind, &content);
        apply_significance(&mut extracted, &content);
        successfully_scanned_files += 1;
        surfaces.append(&mut extracted);
        if let Some(message) = parse_error {
            diagnostics.push(failure_diagnostic(rel, "parse-error", message));
        }
    }
    for path in excluded_paths.into_inner() {
        diagnostics.push(exclusion_diagnostic(&path, &patterns));
    }
    surfaces.extend(crate::social::account_surfaces(root)?);
    diagnostics.sort_by(|left, right| left.path.cmp(&right.path).then(left.code.cmp(&right.code)));
    surfaces.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.start_byte.cmp(&right.start_byte))
            .then(left.kind.to_string().cmp(&right.kind.to_string()))
    });
    Ok(ScanResult {
        schema_version: EXTRACTION_SCHEMA_VERSION.into(),
        complete: !diagnostics.iter().any(|item| item.affects_completeness),
        surfaces,
        diagnostics,
        files_considered: scanned_files,
        files_scanned: successfully_scanned_files,
    })
}

fn exclusion_diagnostic(path: &Path, patterns: &[String]) -> ScanDiagnostic {
    if matches_ignore_pattern(path, patterns) {
        return skip_diagnostic(
            path,
            "project-policy-exclusion",
            "excluded by project .brandiignore policy",
        );
    }
    let component = path.components().find_map(|component| {
        let value = component.as_os_str().to_str()?;
        IGNORED_COMPONENTS.contains(&value).then_some(value)
    });
    let (code, reason) = match component {
        Some("target" | "dist" | "build" | ".next" | "coverage" | "__pycache__") => (
            "generated-build-artifacts",
            "generated build artefacts excluded by default policy",
        ),
        Some("node_modules" | "vendor") => (
            "dependency-corpus",
            "dependency or vendored corpus excluded by default policy",
        ),
        Some(".git") => (
            "vcs-internals",
            "version-control internals excluded by default policy",
        ),
        Some(".brandi") => (
            "brandi-configuration",
            "Brandi's own brief and guideline machinery excluded by default policy",
        ),
        Some("memory") => (
            "internal-memory",
            "internal memory excluded by default policy",
        ),
        Some("testdata" | "fixtures" | "benches") => (
            "test-fixture",
            "test fixtures and benchmark corpora excluded by default policy",
        ),
        Some(".idea" | ".vscode") => (
            "editor-metadata",
            "editor metadata excluded by default policy",
        ),
        _ => (
            "policy-exclusion",
            "path excluded by default scanner policy",
        ),
    };
    skip_diagnostic(path, code, reason)
}

/// Compatibility wrapper for callers that require complete evidence.
pub fn scan_surfaces(root: &Path) -> Result<Vec<Surface>> {
    let scan = scan_project(root)?;
    if !scan.complete {
        return Err(BrandiError::Invalid(format!(
            "scan incomplete: {} input diagnostics; run `brandi scan --format json` for details",
            scan.diagnostics
                .iter()
                .filter(|item| item.affects_completeness)
                .count()
        )));
    }
    Ok(scan.surfaces)
}

fn skip_diagnostic(path: &Path, code: &str, message: impl Into<String>) -> ScanDiagnostic {
    ScanDiagnostic {
        path: path.to_path_buf(),
        code: code.into(),
        message: message.into(),
        affects_completeness: false,
    }
}

fn failure_diagnostic(path: &Path, code: &str, message: impl Into<String>) -> ScanDiagnostic {
    ScanDiagnostic {
        path: path.to_path_buf(),
        code: code.into(),
        message: message.into(),
        affects_completeness: true,
    }
}

fn is_generated_source(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    name.contains(".generated.")
        || name.ends_with("_generated.go")
        || name.ends_with(".g.rs")
        || name.ends_with(".min.js")
}

/// Content heuristic for generated source files that the filename-based
/// [`is_generated_source`] cannot catch (e.g. vendored Unicode tables with a
/// "generated by" header). Only the leading 1 KiB is inspected so normal
/// prose mentioning "generated" is not skipped.
fn is_generated_content(content: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(generated by|auto-?generated|machine generated|automatically generated)\b",
        )
        .expect("generated-content regex must compile")
    });
    let sample_len = content.floor_char_boundary(1024.min(content.len()));
    re.is_match(&content[..sample_len])
}

fn is_test_source(path: &Path) -> bool {
    let file = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|value| matches!(value, "test" | "tests" | "spec" | "specs"))
    }) || file.contains("_test.")
        || file.contains(".test.")
        || file.contains(".spec.")
}

fn apply_significance(surfaces: &mut [Surface], content: &str) {
    for surface in surfaces {
        surface.significance =
            classify_significance(&surface.path, surface.kind, &surface.context, content);
        surface.exposure = surface.significance.exposure.score;
    }
}

/// Describe what a detected surface means to brand analysis independently of
/// the detector's confidence that the surface exists.
pub fn classify_significance(
    path: &Path,
    kind: SurfaceKind,
    context: &str,
    content: &str,
) -> SurfaceSignificance {
    let file = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let path_text = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let generated = is_generated_content(content)
        || content
            .get(..content.floor_char_boundary(1024.min(content.len())))
            .is_some_and(|sample| sample.to_ascii_lowercase().contains("managed by poka"));
    let agent_instruction = matches!(file.as_str(), "claude.md" | "agents.md")
        || file == "copilot-instructions.md"
        || path_text.contains("/.claude/");
    let governance = matches!(
        file.as_str(),
        "contributing.md" | "security.md" | "code_of_conduct.md"
    );
    let provenance = if path_text.contains("/vendor/") || path_text.contains("/node_modules/") {
        SurfaceProvenance::Vendored
    } else if generated {
        SurfaceProvenance::Generated
    } else {
        SurfaceProvenance::Authored
    };

    let (domain, subtype, audience, reach, visibility, relevance, authority) =
        if context == "test_assertion" {
            (
                SurfaceDomain::Semantic,
                SurfaceSubtype::TestAssertion,
                ExposureAudience::Developer,
                ExposureReach::Repository,
                ExposureVisibility::Indirect,
                65,
                65,
            )
        } else if agent_instruction {
            (
                SurfaceDomain::Operational,
                SurfaceSubtype::AgentInstruction,
                ExposureAudience::Agent,
                ExposureReach::Repository,
                ExposureVisibility::Indirect,
                20,
                85,
            )
        } else if governance {
            (
                SurfaceDomain::Operational,
                SurfaceSubtype::Governance,
                ExposureAudience::Contributor,
                ExposureReach::Repository,
                ExposureVisibility::Direct,
                35,
                90,
            )
        } else if generated && matches!(kind, SurfaceKind::Doc | SurfaceKind::RepoDoc) {
            (
                SurfaceDomain::Operational,
                SurfaceSubtype::GeneratedDocumentation,
                ExposureAudience::Developer,
                ExposureReach::Repository,
                ExposureVisibility::Indirect,
                25,
                60,
            )
        } else {
            match kind {
                SurfaceKind::UiString => (
                    SurfaceDomain::Semantic,
                    SurfaceSubtype::UiCopy,
                    ExposureAudience::User,
                    ExposureReach::Product,
                    ExposureVisibility::Direct,
                    95,
                    90,
                ),
                SurfaceKind::Style => (
                    SurfaceDomain::Semantic,
                    SurfaceSubtype::VisualStyle,
                    ExposureAudience::User,
                    ExposureReach::Product,
                    ExposureVisibility::Direct,
                    90,
                    85,
                ),
                SurfaceKind::Social => (
                    SurfaceDomain::Semantic,
                    SurfaceSubtype::SocialProfile,
                    ExposureAudience::Public,
                    ExposureReach::Product,
                    ExposureVisibility::Direct,
                    95,
                    90,
                ),
                SurfaceKind::RepoDoc if file.starts_with("readme") => (
                    SurfaceDomain::Semantic,
                    SurfaceSubtype::ProductDocumentation,
                    ExposureAudience::Public,
                    ExposureReach::Product,
                    ExposureVisibility::Direct,
                    100,
                    100,
                ),
                SurfaceKind::RepoDoc => (
                    SurfaceDomain::Semantic,
                    SurfaceSubtype::RepositoryDocumentation,
                    ExposureAudience::Contributor,
                    ExposureReach::Repository,
                    ExposureVisibility::Direct,
                    70,
                    85,
                ),
                SurfaceKind::Doc => (
                    SurfaceDomain::Semantic,
                    SurfaceSubtype::ProductDocumentation,
                    ExposureAudience::Developer,
                    ExposureReach::Repository,
                    ExposureVisibility::Indirect,
                    70,
                    70,
                ),
            }
        };
    let score = match audience {
        ExposureAudience::Public | ExposureAudience::User => 5,
        ExposureAudience::Contributor => 4,
        ExposureAudience::Agent | ExposureAudience::Developer => 3,
        ExposureAudience::None => 1,
    };
    let provenance_factor = match provenance {
        SurfaceProvenance::Authored => 100u16,
        SurfaceProvenance::Generated => 25,
        SurfaceProvenance::Inherited => 50,
        SurfaceProvenance::Vendored => 0,
        SurfaceProvenance::Mirrored => 40,
    };
    let semantic_weight = ((u16::from(relevance) * provenance_factor) / 100) as u8;
    SurfaceSignificance {
        domain,
        subtype,
        provenance,
        exposure: ExposureProfile {
            audience,
            reach,
            visibility,
            score,
        },
        relevance,
        authority,
        semantic_weight,
    }
}

/// Extract the surfaces contained in a single file (`path` may be absolute or
/// relative to `root`).
///
/// * Files that classify to `None` yield an empty vec, not an error.
/// * A missing file is a `BrandiError::NotFound`.
/// * An existing but unreadable/binary file yields an empty vec (the same
///   "never fail a scan" policy as [`scan_surfaces`]).
pub fn scan_file(root: &Path, path: &Path) -> Result<Vec<Surface>> {
    let full_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let Some(kind) = classify(&full_path, root) else {
        return Ok(Vec::new());
    };
    if is_test_source(&full_path) && kind != SurfaceKind::UiString {
        return Ok(Vec::new());
    }
    if full_path
        .metadata()
        .is_ok_and(|metadata| metadata.len() > MAX_SOURCE_FILE_BYTES)
    {
        return Err(BrandiError::Invalid(format!(
            "{} exceeds the {} byte source-file safety limit",
            full_path.display(),
            MAX_SOURCE_FILE_BYTES
        )));
    }
    let content = match std::fs::read_to_string(&full_path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(BrandiError::NotFound(format!("{}", full_path.display())));
        }
        Err(_) => return Ok(Vec::new()),
    };
    let (mut surfaces, parse_error) = surfaces_in_text(&full_path, kind, &content);
    apply_significance(&mut surfaces, &content);
    if let Some(message) = parse_error {
        return Err(BrandiError::Invalid(format!(
            "{} could not be parsed completely: {message}",
            full_path.display()
        )));
    }
    Ok(surfaces)
}

/// Pull user-facing string literals out of source `content`;
/// returns (1-based line, literal) pairs.
///
/// Heuristics (deliberately simple — this is a candidate extractor, the
/// voice/terminology rules do the real judging):
///
/// * Candidates are double-quoted literals of >= 4 chars with no escapes or
///   newlines inside (`"([^"\\\n]{4,})"`). Single-quoted strings are ignored:
///   in Rust/Go they are chars, and in TS/JS/Python they skew towards
///   code-ish tokens.
/// * KEEP only literals that contain at least one space — user-facing copy
///   is virtually always multi-word, while single-word literals are usually
///   identifiers, keys, or enum tags.
/// * REJECT literals that look machine-facing:
///   - pure lowercase identifier/path/format-key shapes
///     (`^[a-z0-9_\-./:{}%]+$`, e.g. `"application/json"`, `"some.key"`);
///   - URLs (containing `http://` or `https://`);
///   - anything containing `::` (module paths like `"fmt::Error"`);
///   - pure format strings (two or more `{}` placeholders, e.g. `"{}: {}"`);
///   - paths, i.e. literals starting with `.` or `/`;
///   - literals with no ASCII letters at all (digits/punctuation only).
///
/// Extract user-facing string literals from `content` with byte spans.
///
/// Returns `(1-based line, start_byte, end_byte, literal text)` tuples.
pub fn extract_ui_string_spans(content: &str) -> Vec<(usize, usize, usize, String)> {
    let re = ui_string_regex();
    let mut out = Vec::new();
    let mut line = 1usize;
    let mut scanned = 0usize;
    for caps in re.captures_iter(content) {
        let Some(m) = caps.get(0) else {
            continue;
        };
        line += count_newlines(&content[scanned..m.start()]);
        scanned = m.start();
        let Some(literal) = caps.get(1).map(|group| group.as_str()) else {
            continue;
        };
        if is_user_facing_candidate(literal) {
            out.push((line, m.start(), m.end(), literal.to_string()));
        }
    }
    out
}

pub fn extract_ui_strings(content: &str) -> Vec<(usize, String)> {
    extract_ui_string_spans(content)
        .into_iter()
        .map(|(line, _, _, text)| (line, text))
        .collect()
}

/// The KEEP/REJECT filter behind [`extract_ui_strings`]; see its docs for the
/// heuristics.
fn is_user_facing_candidate(literal: &str) -> bool {
    if literal.len() < 4 || !literal.contains(' ') {
        return false;
    }
    if identifier_regex().is_match(literal) {
        return false;
    }
    if literal.contains("http://") || literal.contains("https://") {
        return false;
    }
    if literal.contains("::") {
        return false;
    }
    if literal.matches("{}").count() >= 2 {
        return false;
    }
    if literal.starts_with('.') || literal.starts_with('/') {
        return false;
    }
    if !literal.bytes().any(|b| b.is_ascii_alphabetic()) {
        return false;
    }
    true
}

/// Emit every surface contained in one already-read text file.
///
/// * RepoDoc/Doc: one surface per line (1-based), with fenced code block
///   bodies blanked via [`strip_fenced_code`] — code examples are not
///   prose. Line numbers still match the original file.
/// * UiString sources: one surface per extracted string literal.
/// * Style sources (css/scss): no line surfaces at all.
/// * Every kind additionally emits one Style surface per `#rrggbb`
///   occurrence in the ORIGINAL content, fences included — colors inside
///   CSS/markdown code examples are legitimately checked — so the palette
///   rule sees colors in markdown and code too. For stylesheets those hex
///   surfaces are the only emission, keeping every Style surface's text a
///   single hex literal.
fn surfaces_in_text(
    path: &Path,
    kind: SurfaceKind,
    content: &str,
) -> (Vec<Surface>, Option<String>) {
    let mut surfaces = Vec::new();
    let mut parse_error = None;
    match kind {
        SurfaceKind::RepoDoc | SurfaceKind::Doc => {
            let stripped = strip_fenced_code(content);
            let mut offset = 0usize;
            for (index, text) in stripped.lines().enumerate() {
                if text.trim().is_empty() || fence_run(text).is_some() {
                    offset += text.len() + 1;
                    continue;
                }
                surfaces.push(Surface {
                    kind,
                    path: path.to_path_buf(),
                    line: index + 1,
                    text: text.to_string(),
                    context: "documentation".into(),
                    confidence: 100,
                    exposure: if kind == SurfaceKind::RepoDoc { 5 } else { 3 },
                    significance: SurfaceSignificance::default(),
                    reason: "prose outside fenced code in a documentation surface".into(),
                    start_byte: offset,
                    end_byte: offset + text.len(),
                });
                offset += text.len() + 1;
            }
        }
        SurfaceKind::UiString => {
            let large = content.len() > MAX_AST_SOURCE_BYTES as usize
                || count_newlines(content) + 1 > MAX_AST_SOURCE_LINES;
            if large {
                if is_test_source(path) {
                    return (surfaces, None);
                }
                for (line, start_byte, end_byte, text) in extract_ui_string_spans(content) {
                    surfaces.push(Surface {
                        kind: SurfaceKind::UiString,
                        path: path.to_path_buf(),
                        line,
                        text,
                        context: "large-source-regex".into(),
                        confidence: 70,
                        exposure: 3,
                        significance: SurfaceSignificance::default(),
                        reason: "large source file: regex fallback instead of AST parse".into(),
                        start_byte,
                        end_byte,
                    });
                }
            } else {
                match extract_ast_surfaces(path, content) {
                    Ok(extracted) => surfaces.extend(extracted),
                    Err(message) => parse_error = Some(message),
                }
            }
        }
        SurfaceKind::Style => {}  // stylesheets only contribute hex colors
        SurfaceKind::Social => {} // social surfaces are injected from account metadata
    }
    surfaces.extend(hex_color_surfaces(path, content));
    (surfaces, parse_error)
}

/// Extract one in-memory corpus sample through the same policy and adapters
/// as a repository scan.
pub fn extract_content(path: &Path, content: &str) -> std::result::Result<Vec<Surface>, String> {
    if is_ignored(path) || is_generated_source(path) {
        return Ok(Vec::new());
    }
    let kind = classify(path, Path::new(""))
        .ok_or_else(|| "sample path is not a supported surface".to_string())?;
    if is_test_source(path) && kind != SurfaceKind::UiString {
        return Ok(Vec::new());
    }
    let (mut surfaces, error) = surfaces_in_text(path, kind, content);
    apply_significance(&mut surfaces, content);
    error.map_or(Ok(surfaces), Err)
}

/// One Style surface per `#rrggbb` occurrence in `content`.
fn hex_color_surfaces(path: &Path, content: &str) -> Vec<Surface> {
    let re = hex_color_regex();
    let mut surfaces = Vec::new();
    let mut line = 1usize;
    let mut scanned = 0usize;
    for m in re.find_iter(content) {
        line += count_newlines(&content[scanned..m.start()]);
        scanned = m.start();
        surfaces.push(Surface {
            kind: SurfaceKind::Style,
            path: path.to_path_buf(),
            line,
            text: m.as_str().to_string(),
            context: "color-token".into(),
            confidence: 100,
            exposure: 2,
            significance: SurfaceSignificance::default(),
            reason: "six-digit color token is governed by the brand palette".into(),
            start_byte: m.start(),
            end_byte: m.end(),
        });
    }
    surfaces
}

/// Extract outward-facing strings with a language-specific Tree-sitter
/// adapter. Unsupported source extensions are rejected unless a future
/// explicit fallback policy opts them in.
pub fn extract_ast_surfaces(
    path: &Path,
    content: &str,
) -> std::result::Result<Vec<Surface>, String> {
    let (adapter, language) = language_adapter(path)
        .ok_or_else(|| "no AST adapter is registered for this source language".to_string())?;
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|error| format!("failed to initialize {adapter} parser: {error}"))?;
    let tree = parser
        .parse(content, None)
        .ok_or_else(|| format!("{adapter} parser returned no syntax tree"))?;
    if tree.root_node().has_error() {
        return Err(format!("{adapter} syntax tree contains parse errors"));
    }

    let bytes = content.as_bytes();
    let mut output = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        let mut cursor = node.walk();
        let children: Vec<Node<'_>> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
        if !is_string_node(adapter, node.kind()) {
            continue;
        }
        let raw = node
            .utf8_text(bytes)
            .map_err(|error| format!("invalid UTF-8 span from {adapter} parser: {error}"))?;
        let Some(text) = decoded_literal(node.kind(), raw) else {
            continue;
        };
        let Some((context, confidence, exposure, reason)) = classify_ast_context(node, bytes)
        else {
            continue;
        };
        if is_test_source(path) && context != "test_assertion" {
            continue;
        }
        if !is_ast_candidate(&text, context) {
            continue;
        }
        output.push(Surface {
            kind: SurfaceKind::UiString,
            path: path.to_path_buf(),
            line: node.start_position().row + 1,
            text,
            context: context.into(),
            confidence,
            exposure,
            significance: SurfaceSignificance::default(),
            reason: format!("{adapter} AST: {reason}"),
            start_byte: node.start_byte(),
            end_byte: node.end_byte(),
        });
    }
    Ok(output)
}

fn language_adapter(path: &Path) -> Option<(&'static str, Language)> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "rs" => Some(("rust", tree_sitter_rust::LANGUAGE.into())),
        "go" => Some(("go", tree_sitter_go::LANGUAGE.into())),
        "js" | "jsx" => Some(("javascript", tree_sitter_javascript::LANGUAGE.into())),
        "ts" => Some((
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        )),
        "tsx" => Some(("tsx", tree_sitter_typescript::LANGUAGE_TSX.into())),
        "py" => Some(("python", tree_sitter_python::LANGUAGE.into())),
        _ => None,
    }
}

fn is_string_node(adapter: &str, kind: &str) -> bool {
    match adapter {
        "rust" => matches!(kind, "string_literal" | "raw_string_literal"),
        "go" => matches!(kind, "interpreted_string_literal" | "raw_string_literal"),
        "javascript" | "typescript" | "tsx" => {
            matches!(kind, "string" | "template_string" | "jsx_text")
        }
        "python" => matches!(kind, "string" | "concatenated_string"),
        _ => false,
    }
}

fn decoded_literal(kind: &str, raw: &str) -> Option<String> {
    if kind == "jsx_text" {
        let value = raw.trim();
        return (!value.is_empty()).then(|| value.to_string());
    }
    let first = raw.find(['\'', '"', '`'])?;
    let quote = raw.as_bytes()[first] as char;
    let last = raw.rfind(quote)?;
    if last <= first {
        return None;
    }
    let value = &raw[first + 1..last];
    if value.contains("\\n") || value.contains('\n') {
        return None;
    }
    Some(value.replace("\\\"", "\"").replace("\\'", "'"))
}

fn classify_ast_context(
    node: Node<'_>,
    source: &[u8],
) -> Option<(&'static str, u8, u8, &'static str)> {
    let mut current = Some(node);
    for _ in 0..8 {
        let ancestor = current?;
        let kind = ancestor.kind();
        let text = ancestor.utf8_text(source).unwrap_or_default();
        let prefix = text
            .chars()
            .take(180)
            .collect::<String>()
            .to_ascii_lowercase();
        if matches!(
            kind,
            "assert_statement" | "macro_invocation" | "call_expression" | "call"
        ) && contains_any(
            &prefix,
            &[
                "assert", "expect(", "expect.", "require.", "require(", "should.",
            ],
        ) {
            return Some((
                "test_assertion",
                95,
                2,
                "text is part of a test assertion or expected user-facing value",
            ));
        }
        if kind.contains("jsx") {
            return Some(("template", 100, 5, "text appears in a JSX/TSX template"));
        }
        if kind == "attribute_item" && prefix.contains("about") {
            return Some(("cli", 100, 5, "text configures command-line help"));
        }
        if matches!(kind, "call_expression" | "macro_invocation" | "call") {
            if contains_any(
                &prefix,
                &[
                    "logger", "logging", "log.", "tracing", "eprintln", "console.",
                ],
            ) {
                return Some(("logging", 95, 3, "text is passed to a logging API"));
            }
            if contains_any(
                &prefix,
                &["println", "print(", "fmt.print", "click.echo", "clap"],
            ) {
                return Some(("cli", 95, 4, "text is passed to a command-line output API"));
            }
            if contains_any(
                &prefix,
                &[
                    "alert",
                    "notify",
                    "toast",
                    "render",
                    "dialog",
                    "message",
                    "label",
                    "title",
                    "placeholder",
                ],
            ) {
                return Some(("ui", 90, 5, "text is passed to a user-interface API"));
            }
            if contains_any(&prefix, &["gettext", "i18n", "translate", " t("]) {
                return Some((
                    "localization",
                    95,
                    5,
                    "text is passed to a localization API",
                ));
            }
        }
        if matches!(kind, "raise_statement" | "panic_expression") {
            return Some((
                "error",
                90,
                4,
                "text is emitted on a user-visible failure path",
            ));
        }
        if matches!(
            kind,
            "let_declaration" | "variable_declarator" | "assignment" | "short_var_declaration"
        ) && contains_any(
            &prefix,
            &[
                "message",
                "label",
                "title",
                "description",
                "placeholder",
                "help_text",
            ],
        ) {
            return Some((
                "ui",
                80,
                3,
                "text is assigned to a user-interface named field",
            ));
        }
        current = ancestor.parent();
    }
    None
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn is_ast_candidate(literal: &str, context: &str) -> bool {
    if literal.len() < 2 || !literal.bytes().any(|byte| byte.is_ascii_alphabetic()) {
        return false;
    }
    if identifier_regex().is_match(literal)
        || literal.contains("http://")
        || literal.contains("https://")
        || literal.contains("::")
        || literal.starts_with('.')
        || literal.starts_with('/')
    {
        return false;
    }
    literal.contains(' ') || matches!(context, "template" | "ui" | "localization")
}

/// Newlines in `s` — used to derive 1-based line numbers for regex matches.
fn count_newlines(s: &str) -> usize {
    s.bytes().filter(|&b| b == b'\n').count()
}

/// Blank out the contents of fenced code blocks, line by line.
///
/// Every line inside a fenced code block is replaced with an empty string
/// (its newline is kept); the fence marker lines themselves are kept (they
/// are markdown, not code). The output has exactly as many lines as the
/// input, so line numbers computed over the result still match the
/// original file.
///
/// Both ``` and ~~~ fences are recognized, with optional language tags. A
/// fence opens on a run of the same fence character of length >= 3 and
/// closes on a run of the same character at least as long as the opener;
/// an unclosed fence blanks everything to the end of input.
///
/// Limitation: indented (4-space) code blocks are NOT recognized — their
/// lines pass through as ordinary text.
pub fn strip_fenced_code(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut open: Option<(char, usize)> = None;
    for chunk in content.split_inclusive('\n') {
        let line = chunk.strip_suffix('\n').unwrap_or(chunk);
        let run = fence_run(line);
        match open {
            Some((fence_char, fence_len)) => {
                // Only a matching closer ends the block; the closing fence
                // line itself is kept. Everything else inside is blanked.
                if let Some((c, n)) = run {
                    if c == fence_char && n >= fence_len {
                        open = None;
                        out.push_str(chunk);
                        continue;
                    }
                }
                if chunk.ends_with('\n') {
                    out.push('\n');
                }
            }
            None => {
                // An opener starts a block; the opening fence line is kept.
                if run.is_some() {
                    open = run;
                }
                out.push_str(chunk);
            }
        }
    }
    out
}

/// The fence run starting `line`, if any: (fence character, run length) for
/// a leading run of >= 3 backticks or tildes (leading whitespace allowed).
fn fence_run(line: &str) -> Option<(char, usize)> {
    let mut chars = line.trim_start().chars();
    let c = chars.next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = 1 + chars.take_while(|&next| next == c).count();
    (n >= 3).then_some((c, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignore_list_matches_any_component() {
        let ignored = [
            ".git/config",
            "target/debug/brandi",
            "node_modules/react/index.js",
            ".brandi/identity.yaml",
            "dist/bundle.js",
            "build/out.o",
            ".idea/workspace.xml",
            ".vscode/settings.json",
            "__pycache__/mod.cpython-311.pyc",
            "vendor/github.com/x/y.go",
            ".next/cache/webpack",
            "coverage/lcov.info",
            // nested components are ignored anywhere in the path
            "src/target/generated.rs",
        ];
        for p in ignored {
            assert!(is_ignored(Path::new(p)), "should ignore {p}");
        }
        // whole components only: "builder.rs" must not trip the "build" entry
        let kept = [
            "src/main.rs",
            "docs/guide.md",
            "README.md",
            "src/builder.rs",
        ];
        for p in kept {
            assert!(!is_ignored(Path::new(p)), "should not ignore {p}");
        }
    }

    #[test]
    fn classify_matrix() {
        let root = Path::new("/proj");
        // RepoDoc: well-known names directly under root (case-insensitive).
        assert_eq!(
            classify(Path::new("/proj/README.md"), root),
            Some(SurfaceKind::RepoDoc)
        );
        assert_eq!(
            classify(Path::new("/proj/Readme.MD"), root),
            Some(SurfaceKind::RepoDoc)
        );
        assert_eq!(
            classify(Path::new("/proj/LICENSE"), root),
            Some(SurfaceKind::RepoDoc)
        );
        assert_eq!(
            classify(Path::new("/proj/CHANGELOG.md"), root),
            Some(SurfaceKind::RepoDoc)
        );
        // RepoDoc: any Markdown under a top-level .github/ directory.
        assert_eq!(
            classify(Path::new("/proj/.github/ISSUE_TEMPLATE/bug.md"), root),
            Some(SurfaceKind::RepoDoc)
        );
        assert_eq!(
            classify(Path::new("/proj/.github/PULL_REQUEST_TEMPLATE.md"), root),
            Some(SurfaceKind::RepoDoc)
        );
        // Doc: other Markdown / rst, including docs/ and nested READMEs.
        assert_eq!(
            classify(Path::new("/proj/docs/guide.md"), root),
            Some(SurfaceKind::Doc)
        );
        assert_eq!(
            classify(Path::new("/proj/docs/README.md"), root),
            Some(SurfaceKind::Doc)
        );
        assert_eq!(
            classify(Path::new("/proj/notes.rst"), root),
            Some(SurfaceKind::Doc)
        );
        // UiString: source files.
        assert_eq!(
            classify(Path::new("/proj/src/foo.rs"), root),
            Some(SurfaceKind::UiString)
        );
        assert_eq!(
            classify(Path::new("/proj/web/app.tsx"), root),
            Some(SurfaceKind::UiString)
        );
        assert_eq!(
            classify(Path::new("/proj/tool.py"), root),
            Some(SurfaceKind::UiString)
        );
        // Style: stylesheets.
        assert_eq!(
            classify(Path::new("/proj/styles/main.css"), root),
            Some(SurfaceKind::Style)
        );
        assert_eq!(
            classify(Path::new("/proj/theme.scss"), root),
            Some(SurfaceKind::Style)
        );
        // Not surfaces.
        assert_eq!(classify(Path::new("/proj/Cargo.toml"), root), None);
        assert_eq!(classify(Path::new("/proj/logo.png"), root), None);
        assert_eq!(
            classify(Path::new("/proj/.github/workflows/ci.yml"), root),
            None
        );
        // Relative root / path combinations classify the same way.
        assert_eq!(
            classify(Path::new("./README.md"), Path::new(".")),
            Some(SurfaceKind::RepoDoc)
        );
    }

    #[test]
    fn extract_accepts_user_facing_copy() {
        let content = r#"fn main() {
    println!("Your session expired. Reconnect to continue.");
    let msg = "Dependency gap detected";
    let hint = format!("Could not open {}", path);
}
"#;
        let found = extract_ui_strings(content);
        assert_eq!(
            found,
            vec![
                (
                    2,
                    "Your session expired. Reconnect to continue.".to_string()
                ),
                (3, "Dependency gap detected".to_string()),
                // a single {} placeholder in real copy is still kept
                (4, "Could not open {}".to_string()),
            ]
        );
    }

    #[test]
    fn extract_rejects_machine_facing_literals() {
        let content = r#"
const P: &str = "/usr/lib/x";
let u = "https://a.io/b";
let w = "see https://a.io/b now";
let i = "camelCaseIdentifier";
let e = "fmt::Error";
let f = "{}: {}";
let k = "application/json";
let n = "1234 5678";
let d = ". hidden file";
"#;
        assert!(
            extract_ui_strings(content).is_empty(),
            "all of these should be rejected: {:?}",
            extract_ui_strings(content)
        );
    }

    #[test]
    fn extract_tracks_line_numbers() {
        let content =
            "// line 1\nlet a = \"First string here\";\n\nlet b = \"Second string here\";\n";
        let found = extract_ui_strings(content);
        assert_eq!(
            found,
            vec![
                (2, "First string here".to_string()),
                (4, "Second string here".to_string()),
            ]
        );
    }

    #[test]
    fn extract_spans_track_byte_offsets() {
        let content =
            "// line 1\nlet a = \"First string here\";\n\nlet b = \"Second string here\";\n";
        let found = extract_ui_string_spans(content);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, 2);
        assert_eq!(&content[found[0].1..found[0].2], "\"First string here\"");
        assert_eq!(found[0].3, "First string here");
        assert_eq!(found[1].0, 4);
        assert_eq!(&content[found[1].1..found[1].2], "\"Second string here\"");
    }

    #[test]
    fn generated_content_header_detects_vendored_tables() {
        let header = "// NOTE: The following code was generated by \"scripts/unicode.py\"\nconst X: u8 = 0;\n";
        assert!(is_generated_content(header));
        let prose = "// This module talks about generated code in the docs\n";
        assert!(!is_generated_content(prose));
    }

    #[test]
    fn hex_surfaces_only_match_rrggbb() {
        let content = "# Heading\n#ff0000 and #fff and #ff0000aa\n#00FF00\n";
        let surfaces = hex_color_surfaces(Path::new("doc.md"), content);
        let texts: Vec<&str> = surfaces.iter().map(|s| s.text.as_str()).collect();
        // headings, 3-digit and 8-digit hex are not #rrggbb colors
        assert_eq!(texts, ["#ff0000", "#00FF00"]);
        assert_eq!(surfaces[0].line, 2);
        assert_eq!(surfaces[1].line, 3);
        assert!(surfaces.iter().all(|s| s.kind == SurfaceKind::Style));
    }

    #[test]
    fn scan_surfaces_walks_fixture_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        std::fs::write(
            root.join("README.md"),
            "# Acme\n\nBrand color is #ff0000 everywhere.\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("styles")).unwrap();
        std::fs::write(
            root.join("styles/main.css"),
            ":root {\n  --primary: #0E7C7B;\n  --accent: #FF0000;\n}\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            "fn main() {\n    println!(\"Welcome to Acme CLI\");\n}\n",
        )
        .unwrap();
        // Files under ignored trees that WOULD classify if they leaked.
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("target/notes.md"), "# generated\n#123456\n").unwrap();
        std::fs::create_dir_all(root.join(".brandi")).unwrap();
        std::fs::write(root.join(".brandi/draft.md"), "# secret #abcdef\n").unwrap();

        let surfaces = scan_surfaces(root).unwrap();

        // README.md: two non-empty RepoDoc surfaces + one Style hex surface.
        let readme: Vec<&Surface> = surfaces
            .iter()
            .filter(|s| s.path == root.join("README.md"))
            .collect();
        assert_eq!(
            readme
                .iter()
                .filter(|s| s.kind == SurfaceKind::RepoDoc)
                .count(),
            2
        );
        let line3 = readme
            .iter()
            .find(|s| s.kind == SurfaceKind::RepoDoc && s.line == 3)
            .unwrap();
        assert_eq!(line3.text, "Brand color is #ff0000 everywhere.");
        let readme_hex: Vec<&&Surface> = readme
            .iter()
            .filter(|s| s.kind == SurfaceKind::Style)
            .collect();
        assert_eq!(readme_hex.len(), 1);
        assert_eq!(readme_hex[0].text, "#ff0000");
        assert_eq!(readme_hex[0].line, 3);

        // styles/main.css: exactly two Style surfaces, no line surfaces.
        let css: Vec<&Surface> = surfaces
            .iter()
            .filter(|s| s.path == root.join("styles/main.css"))
            .collect();
        assert_eq!(css.len(), 2);
        assert!(css.iter().all(|s| s.kind == SurfaceKind::Style));
        let primary = css.iter().find(|s| s.text == "#0E7C7B").unwrap();
        assert_eq!(primary.line, 2);
        let accent = css.iter().find(|s| s.text == "#FF0000").unwrap();
        assert_eq!(accent.line, 3);

        // src/main.rs: one UiString surface with its line and literal.
        let rs: Vec<&Surface> = surfaces
            .iter()
            .filter(|s| s.path == root.join("src/main.rs"))
            .collect();
        assert_eq!(rs.len(), 1);
        assert_eq!(rs[0].kind, SurfaceKind::UiString);
        assert_eq!(rs[0].line, 2);
        assert_eq!(rs[0].text, "Welcome to Acme CLI");

        // Ignored trees contribute nothing.
        assert!(surfaces
            .iter()
            .all(|s| !s.path.starts_with(root.join("target"))));
        assert!(surfaces
            .iter()
            .all(|s| !s.path.starts_with(root.join(".brandi"))));
    }

    #[test]
    fn matches_ignore_pattern_handles_exact_paths_and_bare_components() {
        let patterns = vec!["src/guidelines.rs".to_string(), "fixtures".to_string()];
        assert!(matches_ignore_pattern(
            Path::new("src/guidelines.rs"),
            &patterns
        ));
        assert!(matches_ignore_pattern(
            Path::new("test/fixtures/sample.md"),
            &patterns
        ));
        assert!(!matches_ignore_pattern(
            Path::new("src/guidelines2.rs"),
            &patterns
        ));
        assert!(!matches_ignore_pattern(Path::new("src/main.rs"), &patterns));
        assert!(!matches_ignore_pattern(Path::new("src/guidelines.rs"), &[]));
    }

    #[test]
    fn brandiignore_excludes_matching_paths_from_scan_surfaces() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join(".brandiignore"), "# comment\nsrc/guidelines.rs\n").unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/guidelines.rs"),
            "fn f() { println!(\"this is amazing\"); }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            "fn main() { println!(\"Welcome to Acme CLI\"); }\n",
        )
        .unwrap();
        let surfaces = scan_surfaces(root).unwrap();
        assert!(surfaces
            .iter()
            .all(|s| s.path != root.join("src/guidelines.rs")));
        assert!(surfaces.iter().any(|s| s.path == root.join("src/main.rs")));
    }

    #[test]
    fn scan_file_classifies_reads_and_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("README.md"), "# Acme\nuses #ff0000\n").unwrap();

        // Absolute path: 2 line surfaces + 1 hex surface.
        let surfaces = scan_file(root, &root.join("README.md")).unwrap();
        assert_eq!(surfaces.len(), 3);
        // Path relative to root resolves against root.
        let surfaces = scan_file(root, Path::new("README.md")).unwrap();
        assert_eq!(surfaces.len(), 3);

        // Unclassifiable extension -> empty vec, not an error.
        std::fs::write(root.join("logo.png"), "not really a png").unwrap();
        assert!(scan_file(root, &root.join("logo.png")).unwrap().is_empty());

        // Binary content in a classified file -> empty vec, not an error.
        std::fs::write(root.join("binary.md"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
        assert!(scan_file(root, &root.join("binary.md")).unwrap().is_empty());

        // Missing classified file -> NotFound naming the file.
        match scan_file(root, &root.join("nope.rs")) {
            Err(BrandiError::NotFound(msg)) => assert!(msg.contains("nope.rs"), "got: {msg}"),
            other => panic!("expected BrandiError::NotFound, got {other:?}"),
        }
    }

    #[test]
    fn strip_fenced_code_blanks_bodies_and_keeps_line_numbers() {
        let content =
            "intro\n```rust\nlet x = 1;\nlet y = 2;\n```\nafter\n~~~\ntilde body\n~~~~\nend\n";
        let stripped = strip_fenced_code(content);
        let lines: Vec<&str> = stripped.lines().collect();
        // Fence marker lines (with language tags) are kept; bodies blanked.
        assert_eq!(
            lines,
            vec!["intro", "```rust", "", "", "```", "after", "~~~", "", "~~~~", "end"]
        );
        // Same number of lines as the input: line numbers are preserved.
        assert_eq!(stripped.lines().count(), content.lines().count());
    }

    #[test]
    fn strip_fenced_code_requires_matching_char_and_sufficient_length() {
        // A shorter run or the other fence character does not close a block.
        let content = "```\n~~\n``\nbody\n```\nout\n";
        let stripped = strip_fenced_code(content);
        let lines: Vec<&str> = stripped.lines().collect();
        assert_eq!(lines, vec!["```", "", "", "", "```", "out"]);
    }

    #[test]
    fn strip_fenced_code_handles_unclosed_fences_and_indented_code() {
        // An unclosed fence blanks everything to the end of input.
        let content = "a\n```\nb\nc\n";
        let stripped = strip_fenced_code(content);
        let lines: Vec<&str> = stripped.lines().collect();
        assert_eq!(lines, vec!["a", "```", "", ""]);
        // Indented (4-space) code blocks are out of scope: lines pass through.
        let content = "a\n    code line\nb\n";
        assert_eq!(strip_fenced_code(content), content);
    }

    #[test]
    fn doc_surfaces_skip_fence_bodies_but_keep_hex_colors() {
        let content = "# Doc\n\n```css\nbody { color: #ff0000; }\n```\n\nvisible #00ff00 text\n";
        let (surfaces, parse_error) =
            surfaces_in_text(Path::new("README.md"), SurfaceKind::RepoDoc, content);
        assert!(parse_error.is_none());
        // No RepoDoc surface carries the fenced code; the line still exists,
        // blanked, at its original line number.
        let docs: Vec<&Surface> = surfaces
            .iter()
            .filter(|s| s.kind == SurfaceKind::RepoDoc)
            .collect();
        assert!(
            docs.iter().all(|s| !s.text.contains("color: #")),
            "got: {docs:?}"
        );
        assert!(docs.iter().all(|s| s.line != 4));
        let line7 = docs.iter().find(|s| s.line == 7).unwrap();
        assert_eq!(line7.text, "visible #00ff00 text");
        // Hex colors are extracted from the ORIGINAL content, fences included.
        let hex: Vec<&Surface> = surfaces
            .iter()
            .filter(|s| s.kind == SurfaceKind::Style)
            .collect();
        assert_eq!(hex.len(), 2);
        assert_eq!(hex[0].text, "#ff0000");
        assert_eq!(hex[0].line, 4);
        assert_eq!(hex[1].text, "#00ff00");
        assert_eq!(hex[1].line, 7);
    }

    #[test]
    fn scan_file_rejects_oversized_sources_before_reading() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("oversized.md");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_SOURCE_FILE_BYTES + 1).unwrap();
        let error = scan_file(tmp.path(), &path).unwrap_err().to_string();
        assert!(error.contains("source-file safety limit"));
    }

    #[test]
    fn ast_adapters_preserve_context_span_and_reason() {
        let cases = [
            ("src/main.rs", "fn main(){ println!(\"Welcome home\"); }"),
            (
                "cmd/main.go",
                "package main\nfunc main(){ fmt.Println(\"Welcome home\") }",
            ),
            ("web/app.js", "alert(\"Welcome home\");"),
            ("web/app.ts", "translate(\"Welcome home\");"),
            ("web/app.tsx", "const App=()=> <p>Welcome home</p>;"),
            ("tools/app.py", "print(\"Welcome home\")\n"),
        ];
        for (path, content) in cases {
            let surfaces = extract_content(Path::new(path), content).unwrap();
            assert_eq!(surfaces.len(), 1, "{path}: {surfaces:?}");
            let surface = &surfaces[0];
            assert_eq!(surface.text, "Welcome home");
            assert!(surface.confidence >= 90);
            assert!(surface.end_byte > surface.start_byte);
            assert!(surface.reason.contains("AST"));
        }
    }

    #[test]
    fn generated_agent_instructions_are_detected_but_low_weight() {
        let content = "This file is managed by poka.\n- Never commit secrets.\n";
        let surfaces = extract_content(Path::new(".claude/CLAUDE.md"), content).unwrap();
        assert!(!surfaces.is_empty());
        let significance = &surfaces[0].significance;
        assert_eq!(significance.domain, SurfaceDomain::Operational);
        assert_eq!(significance.subtype, SurfaceSubtype::AgentInstruction);
        assert_eq!(significance.provenance, SurfaceProvenance::Generated);
        assert_eq!(significance.exposure.audience, ExposureAudience::Agent);
        assert!(significance.semantic_weight <= 25);
    }

    #[test]
    fn readme_is_high_weight_public_product_documentation() {
        let surfaces = extract_content(
            Path::new("README.md"),
            "Brandi gives projects a coherent visual identity.\n",
        )
        .unwrap();
        let significance = &surfaces[0].significance;
        assert_eq!(significance.domain, SurfaceDomain::Semantic);
        assert_eq!(significance.exposure.audience, ExposureAudience::Public);
        assert_eq!(significance.semantic_weight, 100);
    }

    #[test]
    fn tests_contribute_assertion_strings_but_not_debug_output() {
        let content = r#"
#[test]
fn renders_copy() {
    assert_eq!(render(), "Your account is ready");
    println!("temporary debug output");
}
"#;
        let surfaces = extract_content(Path::new("tests/copy_test.rs"), content).unwrap();
        assert_eq!(surfaces.len(), 1, "{surfaces:?}");
        assert_eq!(surfaces[0].text, "Your account is ready");
        assert_eq!(surfaces[0].context, "test_assertion");
        assert_eq!(
            surfaces[0].significance.subtype,
            SurfaceSubtype::TestAssertion
        );
    }

    #[test]
    fn test_documents_do_not_become_brand_documentation() {
        let surfaces = extract_content(
            Path::new("tests/fixtures/expected.md"),
            "This is machinery, not product documentation.\n",
        )
        .unwrap();
        assert!(surfaces.is_empty());
    }

    #[test]
    fn parse_failures_make_project_scans_incomplete() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/broken.rs"), "fn main( {").unwrap();
        let scan = scan_project(root.path()).unwrap();
        assert!(!scan.complete);
        assert!(scan
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "parse-error"));
        assert!(scan_surfaces(root.path()).is_err());
    }
}
