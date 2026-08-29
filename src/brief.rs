//! Brand brief: the visionary half of the brand definition stored in
//! `.brandi/` — who the product is (`identity.yaml`) and who it's for
//! (`audience.yaml`).
//!
//! Every struct uses `#[serde(default)]` so partial files load cleanly.

use crate::error::{BrandiError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Default content for `.brandi/identity.yaml`.
const IDENTITY_YAML: &str = r#"product:
  name: Brandi
  tagline: "Brand coherence intelligence"
  mission: "Keep every outward expression of a product aligned with its identity."
  aliases: []
  former_names: []
archetype: [explorer, engineer, challenger]
terminology:
  canonical: {}
  banned_variants: {}
"#;

/// Default content for `.brandi/audience.yaml`.
const AUDIENCE_YAML: &str = r#"segments:
  - name: platform_engineers
    description: "Teams responsible for engineering governance"
    pains: ["identity drift", "inconsistent UX copy"]
narratives:
  - capability: "brand linting"
    narrative: "prevent identity drift before it ships"
    audiences: [platform_engineers]
    formats: [x_post, blog, demo_video, docs]
"#;

/// Who the product is and who it speaks to.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Brief {
    /// Project root the brief was loaded from (not part of the YAML files).
    pub root: PathBuf,
    pub identity: Identity,
    pub audience: Audience,
}

/// Who the product is: names, archetypes, canonical terminology.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Identity {
    pub product: Product,
    pub archetype: Vec<String>,
    pub terminology: Terminology,
}

/// Core product naming.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Product {
    pub name: String,
    pub tagline: String,
    pub mission: String,
    pub aliases: Vec<String>,
    pub former_names: Vec<String>,
}

/// Canonical terms and banned variants of them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Terminology {
    pub canonical: BTreeMap<String, String>,
    pub banned_variants: BTreeMap<String, String>,
}

/// Who the brand speaks to and the stories it tells.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Audience {
    pub segments: Vec<Segment>,
    pub narratives: Vec<Narrative>,
}

/// One audience segment.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Segment {
    pub name: String,
    pub description: String,
    pub pains: Vec<String>,
}

/// A capability narrative mapped to audiences and formats.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Narrative {
    pub capability: String,
    pub narrative: String,
    pub audiences: Vec<String>,
    pub formats: Vec<String>,
}

impl Brief {
    /// Load the brief from `<project_root>/.brandi/`.
    ///
    /// A missing brief file is a `BrandiError::Invalid` naming the file;
    /// YAML parse errors are wrapped in `BrandiError::Yaml`.
    pub fn load(project_root: &Path) -> Result<Brief> {
        let dir = project_root.join(".brandi");
        Ok(Brief {
            root: project_root.to_path_buf(),
            identity: load_yaml(&dir.join("identity.yaml"))?,
            audience: load_yaml(&dir.join("audience.yaml"))?,
        })
    }

    /// Validate the brief under `<project_root>/.brandi/`.
    ///
    /// Returns human-readable warning strings (an empty vec means clean).
    /// Hard problems — missing files, bad YAML — are returned as
    /// `Err(BrandiError::Invalid)` with all problems joined.
    pub fn validate(project_root: &Path) -> Result<Vec<String>> {
        let dir = project_root.join(".brandi");
        let mut problems: Vec<String> = Vec::new();

        let identity = check_yaml::<Identity>(&dir.join("identity.yaml"), &mut problems);
        let audience = check_yaml::<Audience>(&dir.join("audience.yaml"), &mut problems);

        if !problems.is_empty() {
            return Err(BrandiError::Invalid(problems.join("; ")));
        }

        let identity = identity.unwrap_or_default();
        let audience = audience.unwrap_or_default();

        // Soft problems become warnings.
        let mut warnings: Vec<String> = Vec::new();
        if identity.product.name.trim().is_empty() {
            warnings.push("identity: product.name is empty".to_string());
        }
        if identity.product.tagline.trim().is_empty() {
            warnings.push("identity: product.tagline is empty".to_string());
        }
        if audience.segments.is_empty() {
            warnings.push("audience: no segments defined".to_string());
        }
        Ok(warnings)
    }

    /// Create `.brandi/` under `project_root` and write the brief's default
    /// YAML files.
    ///
    /// Idempotent: existing files are never overwritten. Returns the list of
    /// files actually created.
    pub fn scaffold(project_root: &Path) -> Result<Vec<PathBuf>> {
        let dir = project_root.join(".brandi");
        std::fs::create_dir_all(&dir)?;

        let mut created: Vec<PathBuf> = Vec::new();
        let defaults: [(&str, &str); 2] = [
            ("identity.yaml", IDENTITY_YAML),
            ("audience.yaml", AUDIENCE_YAML),
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

/// The message for a missing/unreadable brief file, with the actionable
/// next step brandi's own `error-actionable` rule expects of everyone
/// else's error messages: `brandi init` is the one command that actually
/// fixes this, so name it instead of just naming the missing file.
fn missing_brief_file_message(path: &Path) -> String {
    // No "; " inside this message: `Brief::validate` joins multiple
    // problems with "; ", and `commands::brief_validate` splits on that
    // same separator to print one problem per line — a literal "; " in an
    // individual problem's own text would corrupt that split.
    format!(
        "missing brief file: {} (run `brandi init` to scaffold .brandi/)",
        path.display()
    )
}

/// Read and parse one brief YAML file for `load`.
fn load_yaml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let content = std::fs::read_to_string(path)
        .map_err(|_| BrandiError::Invalid(missing_brief_file_message(path)))?;
    Ok(serde_yaml::from_str(&content)?)
}

/// Read and parse one brief YAML file for `validate`, recording problems
/// instead of failing fast. Returns `None` when the file is missing or invalid.
fn check_yaml<T: serde::de::DeserializeOwned>(
    path: &Path,
    problems: &mut Vec<String>,
) -> Option<T> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => {
            problems.push(missing_brief_file_message(path));
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

    const BRIEF_FILES: [&str; 2] = ["identity.yaml", "audience.yaml"];

    #[test]
    fn scaffold_creates_all_files_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let created = Brief::scaffold(tmp.path()).unwrap();
        assert_eq!(created.len(), 2);
        for name in BRIEF_FILES {
            assert!(
                tmp.path().join(".brandi").join(name).exists(),
                "missing {name}"
            );
        }

        // Modify one file; a second scaffold must not overwrite anything.
        let custom = "product:\n  name: CustomName\n";
        std::fs::write(tmp.path().join(".brandi/identity.yaml"), custom).unwrap();
        let created_again = Brief::scaffold(tmp.path()).unwrap();
        assert!(created_again.is_empty());
        assert_eq!(
            std::fs::read_to_string(tmp.path().join(".brandi/identity.yaml")).unwrap(),
            custom
        );
    }

    #[test]
    fn load_applies_defaults_for_partial_files() {
        let tmp = tempfile::tempdir().unwrap();
        Brief::scaffold(tmp.path()).unwrap();
        // Partial identity file: everything but product.name falls back to defaults.
        std::fs::write(
            tmp.path().join(".brandi/identity.yaml"),
            "product:\n  name: Acme\n",
        )
        .unwrap();
        let brief = Brief::load(tmp.path()).unwrap();
        assert_eq!(brief.identity.product.name, "Acme");
        assert_eq!(brief.identity.product.tagline, "");
        assert!(brief.identity.archetype.is_empty());
        assert_eq!(brief.root, tmp.path());
    }

    #[test]
    fn load_missing_file_names_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        match Brief::load(tmp.path()) {
            Err(BrandiError::Invalid(msg)) => {
                assert!(msg.contains("identity.yaml"), "got: {msg}");
                // Actionable next step, not just a bare fact — this is the
                // one error a brand-new user is most likely to hit first.
                assert!(msg.contains("brandi init"), "got: {msg}");
            }
            other => panic!("expected BrandiError::Invalid, got {other:?}"),
        }
    }

    #[test]
    fn validate_scaffolded_brief_is_clean() {
        let tmp = tempfile::tempdir().unwrap();
        Brief::scaffold(tmp.path()).unwrap();
        let warnings = Brief::validate(tmp.path()).unwrap();
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    #[test]
    fn validate_flags_empty_identity_fields() {
        let tmp = tempfile::tempdir().unwrap();
        Brief::scaffold(tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(".brandi/identity.yaml"),
            "product:\n  name: \"\"\n  tagline: \"\"\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join(".brandi/audience.yaml"), "segments: []\n").unwrap();
        let warnings = Brief::validate(tmp.path()).unwrap();
        assert!(warnings.iter().any(|w| w.contains("product.name")));
        assert!(warnings.iter().any(|w| w.contains("product.tagline")));
        assert!(warnings.iter().any(|w| w.contains("no segments")));
    }
}
