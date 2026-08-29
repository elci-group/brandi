//! Shared data types for surfaces, findings, scores, and lint reports.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

/// Severity of a lint finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// The kind of user-facing surface a piece of content belongs to.
///
/// `rename_all = "snake_case"` keeps JSON serialization ("ui_string")
/// consistent with the `Display` impl below, which is what
/// `Scores.by_kind` and `LintReport.surface_counts` keys already use —
/// without this, `Finding.kind` would serialize as default PascalCase
/// ("UiString") while those maps use snake_case for the same concept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind {
    /// Top-level repo documents (README, CONTRIBUTING, ...).
    RepoDoc,
    /// Documentation files (docs/, *.md deeper in the tree, ...).
    Doc,
    /// UI / CLI string literals found in source code.
    UiString,
    /// Style sources (CSS, theme files, color definitions).
    Style,
    /// A linked social profile whose public identity is both evidence and output.
    Social,
}

impl fmt::Display for SurfaceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SurfaceKind::RepoDoc => "repo_doc",
            SurfaceKind::Doc => "doc",
            SurfaceKind::UiString => "ui_string",
            SurfaceKind::Style => "style",
            SurfaceKind::Social => "social",
        };
        f.write_str(s)
    }
}

/// Whether a surface communicates the product's identity or operates the
/// repository around it. Detection and significance intentionally remain
/// separate: operational prose is still evidence, but it should not dominate
/// brand analysis merely because it is verbose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceDomain {
    Semantic,
    Operational,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceSubtype {
    ProductDocumentation,
    RepositoryDocumentation,
    UiCopy,
    VisualStyle,
    SocialProfile,
    AgentInstruction,
    Governance,
    GeneratedDocumentation,
    TestAssertion,
    Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceProvenance {
    Authored,
    Generated,
    Inherited,
    Vendored,
    Mirrored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExposureAudience {
    Public,
    User,
    Contributor,
    Agent,
    Developer,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExposureReach {
    Product,
    Repository,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExposureVisibility {
    Direct,
    Indirect,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExposureProfile {
    pub audience: ExposureAudience,
    pub reach: ExposureReach,
    pub visibility: ExposureVisibility,
    pub score: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceSignificance {
    pub domain: SurfaceDomain,
    pub subtype: SurfaceSubtype,
    pub provenance: SurfaceProvenance,
    pub exposure: ExposureProfile,
    /// How closely this evidence expresses product identity, from 0 to 100.
    pub relevance: u8,
    /// How authoritative the source is for its intended audience, 0 to 100.
    pub authority: u8,
    /// Downstream analytical weight after provenance is applied, 0 to 100.
    pub semantic_weight: u8,
}

impl Default for SurfaceSignificance {
    fn default() -> Self {
        Self {
            domain: SurfaceDomain::Semantic,
            subtype: SurfaceSubtype::ProductDocumentation,
            provenance: SurfaceProvenance::Authored,
            exposure: ExposureProfile {
                audience: ExposureAudience::Developer,
                reach: ExposureReach::File,
                visibility: ExposureVisibility::Internal,
                score: 3,
            },
            // Legacy/deserialized surfaces did not carry significance. Treat
            // them neutrally so adopting the richer schema does not silently
            // halve existing scores; extraction always replaces this profile.
            relevance: 100,
            authority: 100,
            semantic_weight: 100,
        }
    }
}

/// A single discovered surface: one unit of user-facing content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Surface {
    pub kind: SurfaceKind,
    pub path: PathBuf,
    pub line: usize,
    pub text: String,
    pub context: String,
    pub confidence: u8,
    pub exposure: u8,
    #[serde(default)]
    pub significance: SurfaceSignificance,
    pub reason: String,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// What an evaluated finding represents in the decision pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingClass {
    Violation,
    Opportunity,
    Observation,
}

/// The syntactic or visual role of the object a finding addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRole {
    Prose,
    Heading,
    UiCopy,
    ColorToken,
    SocialProfile,
    DocumentStructure,
    Typography,
}

/// Decision metadata attached after detection and before proposals are ranked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingSemantics {
    pub class: FindingClass,
    pub role: SemanticRole,
    pub domain: SurfaceDomain,
    pub provenance: SurfaceProvenance,
    pub audience: ExposureAudience,
    pub confidence: u8,
    pub relevance: u8,
    pub authority: u8,
    pub exposure: u8,
    pub impact: u8,
    /// Normalized multiplicative decision score (0-100), used for ranking.
    pub priority: u64,
}

impl Default for FindingSemantics {
    fn default() -> Self {
        Self {
            class: FindingClass::Violation,
            role: SemanticRole::Prose,
            domain: SurfaceDomain::Semantic,
            provenance: SurfaceProvenance::Authored,
            audience: ExposureAudience::Developer,
            confidence: 100,
            relevance: 100,
            authority: 100,
            exposure: 3,
            impact: 50,
            priority: 30,
        }
    }
}

/// A single rule violation (or observation) tied to a location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub rule_id: String,
    pub severity: Severity,
    pub kind: Option<SurfaceKind>,
    pub path: PathBuf,
    pub line: Option<usize>,
    pub message: String,
    pub suggestion: Option<String>,
    #[serde(default)]
    pub semantics: FindingSemantics,
}

/// A path that could not be scanned completely, or was deliberately skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanDiagnostic {
    pub path: PathBuf,
    pub code: String,
    pub message: String,
    pub affects_completeness: bool,
}

/// Versioned extraction result, including evidence about skipped inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanResult {
    pub schema_version: String,
    pub complete: bool,
    pub surfaces: Vec<Surface>,
    pub diagnostics: Vec<ScanDiagnostic>,
    pub files_considered: usize,
    pub files_scanned: usize,
}

/// Coherence scores computed from a set of findings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scores {
    pub model_version: String,
    pub overall: u32,
    pub by_kind: BTreeMap<String, u32>,
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
    pub normalized_density_per_1000: u32,
    pub weighted_penalty_units: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineDelta {
    pub baseline_path: PathBuf,
    pub new_findings: usize,
    pub resolved_findings: usize,
}

/// A full lint report for a project tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintReport {
    pub schema_version: String,
    pub root: PathBuf,
    pub timestamp: String,
    pub scores: Scores,
    pub surface_counts: BTreeMap<String, usize>,
    pub findings: Vec<Finding>,
    pub scan_complete: bool,
    pub scan_diagnostics: Vec<ScanDiagnostic>,
    pub baseline_delta: Option<BaselineDelta>,
}
