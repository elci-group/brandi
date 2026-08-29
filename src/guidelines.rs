//! Brand guidelines: the instructional half of the brand definition stored
//! in `.brandi/` — how the brand sounds (`voice.yaml`), how it looks
//! (`visual.yaml`), and what language it forbids (`prohibited.yaml`), plus
//! an optional `rules.yaml` with per-rule severity overrides.
//!
//! Every struct uses `#[serde(default)]` so partial files load cleanly.

use crate::error::{BrandiError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Default content for `.brandi/voice.yaml`.
const VOICE_YAML: &str = r#"traits: [precise, ambitious, human]
trait_signals:
  precise: [exactly, deterministic, verify]
  ambitious: [build, push, frontier]
  human: [you, your, we]
style:
  sentence_case_headings: true
  max_exclamation_marks: 0
  error_messages:
    forbid_prefixes: ["Error", "ERROR", "Fatal", "panic"]
    forbid_numeric_codes: true
    require_actionable: true
"#;

/// Default content for `.brandi/visual.yaml`.
const VISUAL_YAML: &str = r##"palette:
  primary: "#FF2DAA"
  secondary: ["#FF78C8", "#DCA7FF", "#FF4D6D", "#FFB020", "#58D5FF"]
  neutrals: ["#FFF7FC", "#160812", "#6F5B68"]
color_tolerance: 24
typography:
  style: technical_modern
  preferred_fonts: []
assets:
  social_card: { width: 1200, height: 630 }
  thumbnail: { width: 1280, height: 720 }
  social_card_path: assets/social-card.svg
  thumbnail_path: assets/thumbnail.svg
  min_whitespace: 0.25
  max_file_kb: 500
  coherence_min_palette: 50
visual_language:
  geometry: geometric
  complexity: restrained
  density: balanced
  contrast: high
  corner_language: consistent
  lighting: flat
  depth: low
  texture: minimal
  illustration_style: brand-specific
"##;

/// Default content for `.brandi/prohibited.yaml`.
const PROHIBITED_YAML: &str = r#"categories:
  corporate_jargon: ["synergy", "best-in-class", "seamless", "seamlessly", "cutting-edge", "world-class", "leverage our"]
  empty_hype: ["revolutionary", "game-changing", "game changer", "the future of", "awesome", "amazing", "incredible"]
  generic_ai_language: ["delve", "delving", "unlock the power", "supercharge", "elevate", "harness the", "in today's fast-paced"]
severity:
  corporate_jargon: warning
  empty_hype: error
  generic_ai_language: warning
visual_motifs: []
"#;

/// The instructional rules the brand must be executed against.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Guidelines {
    /// Project root the guidelines were loaded from (not part of the YAML files).
    pub root: PathBuf,
    pub voice: Voice,
    pub visual: Visual,
    pub prohibited: Prohibited,
    /// Per-rule severity overrides from the optional `rules.yaml`
    /// (an absent file means no overrides).
    pub rules_overrides: RuleOverrides,
}

/// How the brand sounds.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Voice {
    pub traits: Vec<String>,
    pub trait_signals: BTreeMap<String, Vec<String>>,
    pub style: VoiceStyle,
}

/// Mechanical style rules for written voice.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceStyle {
    pub sentence_case_headings: bool,
    pub max_exclamation_marks: usize,
    pub error_messages: ErrorMessageStyle,
}

/// Style rules specific to error messages.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ErrorMessageStyle {
    pub forbid_prefixes: Vec<String>,
    pub forbid_numeric_codes: bool,
    pub require_actionable: bool,
}

/// How the brand looks.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Visual {
    pub palette: Palette,
    pub color_tolerance: u8,
    pub typography: Typography,
    pub assets: AssetSpec,
    /// The nine visual-language axes handed to generation providers such as
    /// VASILIS's `brand_context.visual_language` (see `VisualLanguage`).
    pub visual_language: VisualLanguage,
}

/// Brand colors, hex `#rrggbb`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Palette {
    pub primary: String,
    pub secondary: Vec<String>,
    pub neutrals: Vec<String>,
}

impl Palette {
    /// All palette colors parsed to RGB; entries that are not valid
    /// `#rrggbb` are skipped.
    pub fn all_colors(&self) -> Vec<(u8, u8, u8)> {
        std::iter::once(&self.primary)
            .chain(self.secondary.iter())
            .chain(self.neutrals.iter())
            .filter_map(|s| parse_hex_color(s))
            .collect()
    }
}

/// Typography guidance.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Typography {
    pub style: String,
    pub preferred_fonts: Vec<String>,
}

/// Required dimensions/whitespace for brand assets, plus audit budgets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetSpec {
    pub social_card: Dimensions,
    pub thumbnail: Dimensions,
    /// Project-relative destinations used by `brandi direct`.
    pub social_card_path: PathBuf,
    pub thumbnail_path: PathBuf,
    pub min_whitespace: f32,
    /// File size budget per raster asset, in KiB (used by `assets audit`).
    pub max_file_kb: u32,
    /// Minimum palette-adherence percent for an asset to count as on-brand
    /// in coherence checks.
    pub coherence_min_palette: u32,
}

impl Default for AssetSpec {
    fn default() -> Self {
        AssetSpec {
            social_card: Dimensions {
                width: 1200,
                height: 630,
            },
            thumbnail: Dimensions {
                width: 1280,
                height: 720,
            },
            social_card_path: PathBuf::from("assets/social-card.svg"),
            thumbnail_path: PathBuf::from("assets/thumbnail.svg"),
            min_whitespace: 0.25,
            max_file_kb: 500,
            coherence_min_palette: 50,
        }
    }
}

/// Pixel dimensions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Dimensions {
    pub width: u32,
    pub height: u32,
}

/// The nine visual-language axes generation providers (VASILIS's
/// `brand_context.visual_language`, in particular) use to steer generated
/// imagery toward the brand's look. Each axis is a short descriptive value
/// (e.g. `geometry: "geometric"`); free-form so new axis values don't
/// require a Brandi release. Fields left unset in `.brandi/visual.yaml`
/// fall back to the defaults below rather than an empty string, so a
/// project that never configures this section still gets a coherent set
/// of values instead of blanks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VisualLanguage {
    pub geometry: String,
    pub complexity: String,
    pub density: String,
    pub contrast: String,
    pub corner_language: String,
    pub lighting: String,
    pub depth: String,
    pub texture: String,
    pub illustration_style: String,
}

impl Default for VisualLanguage {
    fn default() -> Self {
        VisualLanguage {
            geometry: "geometric".into(),
            complexity: "restrained".into(),
            density: "balanced".into(),
            contrast: "high".into(),
            corner_language: "consistent".into(),
            lighting: "flat".into(),
            depth: "low".into(),
            texture: "minimal".into(),
            illustration_style: "brand-specific".into(),
        }
    }
}

/// Language the brand forbids, with per-category severity
/// (severity values: "error" | "warning" | "info").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Prohibited {
    pub categories: BTreeMap<String, Vec<String>>,
    pub severity: BTreeMap<String, String>,
    /// Forbidden visual motifs (e.g. "gradients", "photorealistic people"),
    /// distinct from the forbidden terminology in `categories`/`severity`.
    /// Not consumed by any text-lint rule; fed straight into generation
    /// providers as VASILIS's `brand_context.forbidden_motifs`.
    pub visual_motifs: Vec<String>,
}

/// The rule ids that `.brandi/rules.yaml` overrides may name. This mirrors
/// `rules::rule_catalog()` — rules.rs owns the catalog and guidelines.rs
/// cannot reference it without a module cycle; a crate-internal test
/// asserts the two stay in sync.
pub const KNOWN_RULE_IDS: &[&str] = &[
    "prohibited-term",
    "terminology-variant",
    "error-prefix",
    "error-actionable",
    "exclamation-limit",
    "sentence-case-headings",
    "readme-mission",
    "readme-image-refs",
    "palette-adherence",
    "readme-h1",
    "accessibility-language",
    "inclusive-terminology",
    "reading-level",
    "localization-readiness",
    "terminology-ownership",
];

/// Per-rule severity overrides from the optional `.brandi/rules.yaml`:
/// `off` disables a rule, `info`/`warning`/`error` replace its severity.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RuleOverrides {
    pub overrides: BTreeMap<String, String>,
}

/// Parse a `#rrggbb` hex color (case-insensitive) into RGB.
/// Returns `None` for anything else.
pub fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let hex = s.trim().strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}

impl Guidelines {
    /// Load the guidelines from `<project_root>/.brandi/`.
    ///
    /// A missing guidelines file is a `BrandiError::Invalid` naming the
    /// file, except for the optional `rules.yaml`, which defaults to no
    /// overrides; YAML parse errors are wrapped in `BrandiError::Yaml`.
    pub fn load(project_root: &Path) -> Result<Guidelines> {
        let dir = project_root.join(".brandi");
        Ok(Guidelines {
            root: project_root.to_path_buf(),
            voice: load_yaml(&dir.join("voice.yaml"))?,
            visual: load_yaml(&dir.join("visual.yaml"))?,
            prohibited: load_yaml(&dir.join("prohibited.yaml"))?,
            rules_overrides: load_optional_yaml(&dir.join("rules.yaml"))?,
        })
    }

    /// Validate the guidelines under `<project_root>/.brandi/`.
    ///
    /// Returns human-readable warning strings (an empty vec means clean).
    /// Hard problems — missing files, bad YAML, palette entries that are not
    /// valid `#rrggbb`, unknown values in `prohibited.severity` — are returned
    /// as `Err(BrandiError::Invalid)` with all problems joined.
    pub fn validate(project_root: &Path) -> Result<Vec<String>> {
        let dir = project_root.join(".brandi");
        let mut problems: Vec<String> = Vec::new();

        let _voice = check_yaml::<Voice>(&dir.join("voice.yaml"), &mut problems);
        let visual = check_yaml::<Visual>(&dir.join("visual.yaml"), &mut problems);
        let prohibited = check_yaml::<Prohibited>(&dir.join("prohibited.yaml"), &mut problems);

        // rules.yaml is optional: validated only when present. Every key
        // must be a known rule id, every value off|info|warning|error.
        let rules_path = dir.join("rules.yaml");
        if rules_path.exists() {
            if let Some(overrides) = check_yaml::<RuleOverrides>(&rules_path, &mut problems) {
                for (rule_id, value) in &overrides.overrides {
                    if !KNOWN_RULE_IDS.contains(&rule_id.as_str()) {
                        problems.push(format!("rules: unknown rule id '{rule_id}' in rules.yaml"));
                    }
                    if !matches!(value.as_str(), "off" | "info" | "warning" | "error") {
                        problems.push(format!(
                            "rules: unknown severity override '{value}' for rule '{rule_id}' (expected off|info|warning|error)"
                        ));
                    }
                }
            }
        }

        if !problems.is_empty() {
            return Err(BrandiError::Invalid(problems.join("; ")));
        }

        // All required files parsed (any failure above recorded a problem).
        let visual = visual.unwrap_or_default();
        let prohibited = prohibited.unwrap_or_default();

        // Palette entries must be valid #rrggbb (empty = unset, skipped).
        for entry in std::iter::once(&visual.palette.primary)
            .chain(visual.palette.secondary.iter())
            .chain(visual.palette.neutrals.iter())
            .filter(|e| !e.trim().is_empty())
        {
            if parse_hex_color(entry).is_none() {
                problems.push(format!(
                    "visual: palette entry '{entry}' is not a valid #rrggbb color"
                ));
            }
        }

        for (label, path) in [
            ("social_card_path", &visual.assets.social_card_path),
            ("thumbnail_path", &visual.assets.thumbnail_path),
        ] {
            if path.as_os_str().is_empty()
                || path.is_absolute()
                || path.components().any(|component| {
                    !matches!(
                        component,
                        std::path::Component::Normal(_) | std::path::Component::CurDir
                    )
                })
                || path.extension().and_then(|value| value.to_str()) != Some("svg")
            {
                problems.push(format!(
                    "visual: assets.{label} must be a project-relative .svg path"
                ));
            }
        }
        for (label, dimensions) in [
            ("social_card", &visual.assets.social_card),
            ("thumbnail", &visual.assets.thumbnail),
        ] {
            if dimensions.width == 0 || dimensions.height == 0 {
                problems.push(format!(
                    "visual: assets.{label} dimensions must be greater than zero"
                ));
            }
        }

        // Severity values must be known.
        for (category, value) in &prohibited.severity {
            if !matches!(value.as_str(), "error" | "warning" | "info") {
                problems.push(format!(
                    "prohibited: unknown severity value '{value}' for category '{category}' (expected error|warning|info)"
                ));
            }
        }

        if !problems.is_empty() {
            return Err(BrandiError::Invalid(problems.join("; ")));
        }

        // Soft problems become warnings.
        let mut warnings: Vec<String> = Vec::new();
        if visual.palette.primary.trim().is_empty() {
            warnings.push("visual: palette.primary is not set".to_string());
        }
        for category in prohibited.severity.keys() {
            if !prohibited.categories.contains_key(category) {
                warnings.push(format!(
                    "prohibited: severity entry '{category}' has no matching category"
                ));
            }
        }
        Ok(warnings)
    }

    /// Create `.brandi/` (plus `assets/` with a `.gitkeep`) under
    /// `project_root` and write the guidelines' default YAML files.
    ///
    /// Idempotent: existing files are never overwritten. Returns the list of
    /// files actually created.
    pub fn scaffold(project_root: &Path) -> Result<Vec<PathBuf>> {
        let dir = project_root.join(".brandi");
        let assets_dir = dir.join("assets");
        std::fs::create_dir_all(&assets_dir)?;

        let mut created: Vec<PathBuf> = Vec::new();

        let gitkeep = assets_dir.join(".gitkeep");
        if !gitkeep.exists() {
            std::fs::write(&gitkeep, "")?;
            created.push(gitkeep);
        }

        let defaults: [(&str, &str); 3] = [
            ("voice.yaml", VOICE_YAML),
            ("visual.yaml", VISUAL_YAML),
            ("prohibited.yaml", PROHIBITED_YAML),
        ];
        for (name, content) in defaults {
            let path = dir.join(name);
            if !path.exists() {
                std::fs::write(&path, content)?;
                created.push(path);
            }
        }
        Ok(created)
    }
}

/// The message for a missing/unreadable guidelines file, with the
/// actionable next step brandi's own `error-actionable` rule expects of
/// everyone else's error messages: `brandi init` is the one command that
/// actually fixes this, so name it instead of just naming the missing file.
fn missing_guidelines_file_message(path: &Path) -> String {
    // No "; " inside this message: `Guidelines::validate` joins multiple
    // problems with "; ", and `commands::guidelines_validate` splits on that
    // same separator to print one problem per line — a literal "; " in an
    // individual problem's own text would corrupt that split.
    format!(
        "missing guidelines file: {} (run `brandi init` to scaffold .brandi/)",
        path.display()
    )
}

/// Read and parse one guidelines YAML file for `load`.
fn load_yaml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let content = std::fs::read_to_string(path)
        .map_err(|_| BrandiError::Invalid(missing_guidelines_file_message(path)))?;
    Ok(serde_yaml::from_str(&content)?)
}

/// Read and parse an optional guidelines YAML file for `load`: a missing
/// file yields the default instead of an error.
fn load_optional_yaml<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(serde_yaml::from_str(&content)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(_) => Err(BrandiError::Invalid(missing_guidelines_file_message(path))),
    }
}

/// Read and parse one guidelines YAML file for `validate`, recording
/// problems instead of failing fast. Returns `None` when the file is
/// missing or invalid.
fn check_yaml<T: serde::de::DeserializeOwned>(
    path: &Path,
    problems: &mut Vec<String>,
) -> Option<T> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => {
            problems.push(missing_guidelines_file_message(path));
            return None;
        }
    };
    match serde_yaml::from_str(&content) {
        Ok(v) => Some(v),
        Err(e) => {
            problems.push(format!("invalid YAML in {}: {e}", path.display()));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three required guidelines files, in canonical order.
    const GUIDELINES_FILES: [&str; 3] = ["voice.yaml", "visual.yaml", "prohibited.yaml"];

    #[test]
    fn parse_hex_color_accepts_valid_rrggbb() {
        assert_eq!(parse_hex_color("#0E7C7B"), Some((0x0E, 0x7C, 0x7B)));
        assert_eq!(parse_hex_color("#ffffff"), Some((255, 255, 255)));
        assert_eq!(parse_hex_color("#Ff00aA"), Some((255, 0, 170)));
        assert_eq!(parse_hex_color("#000000"), Some((0, 0, 0)));
    }

    #[test]
    fn parse_hex_color_rejects_invalid() {
        assert_eq!(parse_hex_color("0E7C7B"), None); // missing '#'
        assert_eq!(parse_hex_color("#fff"), None); // too short
        assert_eq!(parse_hex_color("#0E7C7BFF"), None); // too long
        assert_eq!(parse_hex_color("#gggggg"), None); // not hex
        assert_eq!(parse_hex_color(""), None);
        assert_eq!(parse_hex_color("#"), None);
    }

    #[test]
    fn all_colors_skips_unparseable_entries() {
        let palette = Palette {
            primary: "#0E7C7B".to_string(),
            secondary: vec!["#1F2937".to_string(), "not-a-color".to_string()],
            neutrals: vec!["#FFFFFF".to_string(), "".to_string()],
        };
        assert_eq!(
            palette.all_colors(),
            vec![(0x0E, 0x7C, 0x7B), (0x1F, 0x29, 0x37), (255, 255, 255)]
        );
    }

    #[test]
    fn scaffold_creates_all_files_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let created = Guidelines::scaffold(tmp.path()).unwrap();
        assert_eq!(created.len(), 4); // three YAML files + assets/.gitkeep
        for name in GUIDELINES_FILES {
            assert!(
                tmp.path().join(".brandi").join(name).exists(),
                "missing {name}"
            );
        }
        assert!(tmp.path().join(".brandi/assets/.gitkeep").exists());

        // Modify one file; a second scaffold must not overwrite anything.
        let custom = "palette:\n  primary: \"#000000\"\n";
        std::fs::write(tmp.path().join(".brandi/visual.yaml"), custom).unwrap();
        let created_again = Guidelines::scaffold(tmp.path()).unwrap();
        assert!(created_again.is_empty());
        assert_eq!(
            std::fs::read_to_string(tmp.path().join(".brandi/visual.yaml")).unwrap(),
            custom
        );
    }

    #[test]
    fn load_applies_defaults_for_partial_files() {
        let tmp = tempfile::tempdir().unwrap();
        Guidelines::scaffold(tmp.path()).unwrap();
        // Partial voice file: everything but sentence_case_headings falls back to defaults.
        std::fs::write(
            tmp.path().join(".brandi/voice.yaml"),
            "style:\n  sentence_case_headings: false\n",
        )
        .unwrap();
        let guidelines = Guidelines::load(tmp.path()).unwrap();
        assert!(!guidelines.voice.style.sentence_case_headings);
        assert!(guidelines.voice.traits.is_empty());
        assert_eq!(guidelines.root, tmp.path());
        // Untouched scaffolded files keep their default values.
        assert_eq!(guidelines.visual.palette.primary, "#FF2DAA");
        assert_eq!(guidelines.visual.color_tolerance, 24);
    }

    #[test]
    fn visual_language_and_visual_motifs_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        Guidelines::scaffold(tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(".brandi/visual.yaml"),
            "visual_language:\n  geometry: organic\n  complexity: intricate\n  density: dense\n  contrast: low\n  corner_language: rounded\n  lighting: dramatic\n  depth: high\n  texture: tactile\n  illustration_style: hand-drawn\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(".brandi/prohibited.yaml"),
            "visual_motifs: [\"gradients\", \"photorealistic people\"]\n",
        )
        .unwrap();

        let guidelines = Guidelines::load(tmp.path()).unwrap();
        assert_eq!(guidelines.visual.visual_language.geometry, "organic");
        assert_eq!(guidelines.visual.visual_language.complexity, "intricate");
        assert_eq!(guidelines.visual.visual_language.density, "dense");
        assert_eq!(guidelines.visual.visual_language.contrast, "low");
        assert_eq!(guidelines.visual.visual_language.corner_language, "rounded");
        assert_eq!(guidelines.visual.visual_language.lighting, "dramatic");
        assert_eq!(guidelines.visual.visual_language.depth, "high");
        assert_eq!(guidelines.visual.visual_language.texture, "tactile");
        assert_eq!(
            guidelines.visual.visual_language.illustration_style,
            "hand-drawn"
        );
        assert_eq!(
            guidelines.prohibited.visual_motifs,
            vec!["gradients".to_string(), "photorealistic people".to_string()]
        );

        // Round-trip through serde_yaml: struct -> YAML -> struct.
        let visual_yaml = serde_yaml::to_string(&guidelines.visual).unwrap();
        let reparsed_visual: Visual = serde_yaml::from_str(&visual_yaml).unwrap();
        assert_eq!(
            reparsed_visual.visual_language.geometry,
            guidelines.visual.visual_language.geometry
        );
        assert_eq!(
            reparsed_visual.visual_language.illustration_style,
            guidelines.visual.visual_language.illustration_style
        );

        let prohibited_yaml = serde_yaml::to_string(&guidelines.prohibited).unwrap();
        let reparsed_prohibited: Prohibited = serde_yaml::from_str(&prohibited_yaml).unwrap();
        assert_eq!(
            reparsed_prohibited.visual_motifs,
            guidelines.prohibited.visual_motifs
        );
    }

    #[test]
    fn visual_language_and_visual_motifs_default_when_unset() {
        let tmp = tempfile::tempdir().unwrap();
        Guidelines::scaffold(tmp.path()).unwrap();
        // Scaffolded visual.yaml already sets visual_language explicitly;
        // strip it to confirm the struct default still fills in coherent
        // (non-empty) values rather than blanks.
        std::fs::write(
            tmp.path().join(".brandi/visual.yaml"),
            "palette:\n  primary: \"#000000\"\n",
        )
        .unwrap();
        let guidelines = Guidelines::load(tmp.path()).unwrap();
        assert_eq!(guidelines.visual.visual_language.geometry, "geometric");
        assert_eq!(guidelines.visual.visual_language.contrast, "high");
        assert!(guidelines.prohibited.visual_motifs.is_empty());
    }

    #[test]
    fn load_missing_file_names_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        match Guidelines::load(tmp.path()) {
            Err(BrandiError::Invalid(msg)) => {
                assert!(msg.contains("voice.yaml"), "got: {msg}");
                // Actionable next step, not just a bare fact — this is the
                // one error a brand-new user is most likely to hit first.
                assert!(msg.contains("brandi init"), "got: {msg}");
            }
            other => panic!("expected BrandiError::Invalid, got {other:?}"),
        }
    }

    #[test]
    fn validate_scaffolded_guidelines_is_clean() {
        let tmp = tempfile::tempdir().unwrap();
        Guidelines::scaffold(tmp.path()).unwrap();
        let warnings = Guidelines::validate(tmp.path()).unwrap();
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    #[test]
    fn validate_flags_bad_palette_and_severity() {
        let tmp = tempfile::tempdir().unwrap();
        Guidelines::scaffold(tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(".brandi/visual.yaml"),
            "palette:\n  primary: \"teal-ish\"\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(".brandi/prohibited.yaml"),
            "categories:\n  hype: [awesome]\nseverity:\n  hype: fatal\n",
        )
        .unwrap();
        match Guidelines::validate(tmp.path()) {
            Err(BrandiError::Invalid(msg)) => {
                assert!(msg.contains("teal-ish"), "got: {msg}");
                assert!(msg.contains("fatal"), "got: {msg}");
            }
            other => panic!("expected BrandiError::Invalid, got {other:?}"),
        }
    }

    #[test]
    fn known_rule_ids_match_the_rule_catalog() {
        let catalog: std::collections::BTreeSet<&str> = crate::rules::rule_catalog()
            .iter()
            .map(|(id, _)| *id)
            .collect();
        let known: std::collections::BTreeSet<&str> = KNOWN_RULE_IDS.iter().copied().collect();
        assert_eq!(known, catalog);
    }

    #[test]
    fn load_without_rules_yaml_defaults_to_no_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        Guidelines::scaffold(tmp.path()).unwrap();
        let guidelines = Guidelines::load(tmp.path()).unwrap();
        assert!(guidelines.rules_overrides.overrides.is_empty());
        // Scaffold never creates rules.yaml.
        assert!(!tmp.path().join(".brandi/rules.yaml").exists());
    }

    #[test]
    fn load_reads_rules_yaml_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        Guidelines::scaffold(tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(".brandi/rules.yaml"),
            "overrides:\n  error-actionable: off\n  sentence-case-headings: info\n",
        )
        .unwrap();
        let guidelines = Guidelines::load(tmp.path()).unwrap();
        assert_eq!(
            guidelines.rules_overrides.overrides["error-actionable"],
            "off"
        );
        assert_eq!(
            guidelines.rules_overrides.overrides["sentence-case-headings"],
            "info"
        );
    }

    #[test]
    fn validate_accepts_valid_rules_yaml() {
        let tmp = tempfile::tempdir().unwrap();
        Guidelines::scaffold(tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(".brandi/rules.yaml"),
            "overrides:\n  error-actionable: off\n  prohibited-term: error\n",
        )
        .unwrap();
        let warnings = Guidelines::validate(tmp.path()).unwrap();
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    #[test]
    fn validate_rejects_bad_rule_id_and_bad_severity_value() {
        let tmp = tempfile::tempdir().unwrap();
        Guidelines::scaffold(tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(".brandi/rules.yaml"),
            "overrides:\n  not-a-rule: error\n  error-prefix: shout\n",
        )
        .unwrap();
        match Guidelines::validate(tmp.path()) {
            Err(BrandiError::Invalid(msg)) => {
                assert!(msg.contains("not-a-rule"), "got: {msg}");
                assert!(msg.contains("shout"), "got: {msg}");
            }
            other => panic!("expected BrandiError::Invalid, got {other:?}"),
        }
    }
}
