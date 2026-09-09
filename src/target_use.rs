//! Cached identification of a target project's presentation surfaces and frontends.
//!
//! Brandi records the inference as a Padagonia software ontology. A stable
//! evidence fingerprint lets later scans reuse the same target-use decision
//! until the relevant manifests, frontend markers, or surface paths change.

use crate::error::{BrandiError, Result};
use crate::types::{ExposureAudience, Surface};
use padagonia::{
    ComponentGraph, DecompositionConfidence, RelationshipSet, SoftwareComponent,
    SoftwareComponentKind, SoftwareOntology, SoftwareRelationship, SoftwareRelationshipKind,
    TransformationBoundary, SOFTWARE_ONTOLOGY_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const CACHE_SCHEMA_VERSION: &str = "brandi-padagonia-target-use-v1";
const MAX_EVIDENCE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontendKind {
    Cli,
    Tui,
    Gui,
    Web,
    MobileApp,
    TvApp,
}

impl FrontendKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cli => "CLI",
            Self::Tui => "TUI",
            Self::Gui => "GUI",
            Self::Web => "web",
            Self::MobileApp => "mobile app",
            Self::TvApp => "TV app",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrontendUse {
    pub kind: FrontendKind,
    pub reason: String,
    pub evidence: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetedSurfaceSummary {
    pub kind: String,
    pub count: usize,
    pub audiences: Vec<String>,
    pub representative_paths: Vec<PathBuf>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PadagoniaReuse {
    pub ontology_schema_version: String,
    pub cache_schema_version: String,
    pub cache_path: PathBuf,
    pub evidence_fingerprint: String,
    pub reused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetSummary {
    pub project: PathBuf,
    pub frontends: Vec<FrontendUse>,
    pub targeted_surfaces: Vec<TargetedSurfaceSummary>,
    pub rationale: String,
    pub padagonia: PadagoniaReuse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedTargetUse {
    schema_version: String,
    evidence_fingerprint: String,
    frontends: Vec<FrontendUse>,
    ontology: SoftwareOntology,
}

pub fn identify(root: &Path, surfaces: &[Surface]) -> Result<TargetSummary> {
    let evidence = frontend_evidence(root)?;
    let fingerprint = evidence_fingerprint(root, surfaces, &evidence)?;
    let cache_path = cache_path(root);
    let cached = load_cache(&cache_path).filter(|cache| {
        cache.schema_version == CACHE_SCHEMA_VERSION
            && cache.evidence_fingerprint == fingerprint
            && cache.ontology.validate().is_ok()
    });
    let (frontends, reused) = if let Some(cache) = cached {
        (cache.frontends, true)
    } else {
        let frontends = infer_frontends(&evidence);
        let cache = CachedTargetUse {
            schema_version: CACHE_SCHEMA_VERSION.into(),
            evidence_fingerprint: fingerprint.clone(),
            ontology: target_ontology(root, &frontends),
            frontends: frontends.clone(),
        };
        // Identification remains useful if a read-only target prevents cache
        // persistence; scan output records reuse only after a successful load.
        let _ = save_cache(&cache_path, &cache);
        (frontends, false)
    };

    Ok(TargetSummary {
        project: root.to_path_buf(),
        targeted_surfaces: summarize_surfaces(root, surfaces),
        rationale: "Brandi targets discovered communication surfaces because they express, govern, or expose the project's identity; operational surfaces remain visible but carry lower semantic weight.".into(),
        frontends,
        padagonia: PadagoniaReuse {
            ontology_schema_version: SOFTWARE_ONTOLOGY_SCHEMA_VERSION.into(),
            cache_schema_version: CACHE_SCHEMA_VERSION.into(),
            cache_path,
            evidence_fingerprint: fingerprint,
            reused,
        },
    })
}

fn cache_path(root: &Path) -> PathBuf {
    root.join(".brandi/state/padagonia-target-use.json")
}

fn load_cache(path: &Path) -> Option<CachedTargetUse> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn save_cache(path: &Path, cache: &CachedTargetUse) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_file() {
            return Err(BrandiError::Invalid(format!(
                "refusing to replace non-regular Padagonia cache {}",
                path.display()
            )));
        }
    }
    let parent = path
        .parent()
        .ok_or_else(|| BrandiError::Invalid("Padagonia cache path has no parent".into()))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".padagonia-target-use-{}.tmp", std::process::id()));
    fs::write(&temporary, serde_json::to_vec_pretty(cache)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}

#[derive(Debug)]
struct EvidenceFile {
    relative: PathBuf,
    content: String,
}

fn frontend_evidence(root: &Path) -> Result<Vec<EvidenceFile>> {
    const FILES: &[&str] = &[
        "Cargo.toml",
        "package.json",
        "index.html",
        "tauri.conf.json",
        "src-tauri/tauri.conf.json",
        "pubspec.yaml",
        "android/app/src/main/AndroidManifest.xml",
        "AndroidManifest.xml",
        "ios/Runner/Info.plist",
        "Package.swift",
        "tizen-manifest.xml",
        "appinfo.json",
    ];
    let mut evidence = Vec::new();
    for relative in FILES {
        let path = root.join(relative);
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > MAX_EVIDENCE_BYTES {
            continue;
        }
        evidence.push(EvidenceFile {
            relative: PathBuf::from(relative),
            content: fs::read_to_string(path)
                .unwrap_or_default()
                .to_ascii_lowercase(),
        });
    }
    for directory in ["tui", "web", "frontend", "android", "ios", "tvos"] {
        if root.join(directory).is_dir() {
            evidence.push(EvidenceFile {
                relative: PathBuf::from(directory),
                content: "directory".into(),
            });
        }
    }
    evidence.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(evidence)
}

fn evidence_fingerprint(
    root: &Path,
    surfaces: &[Surface],
    evidence: &[EvidenceFile],
) -> Result<String> {
    let mut digest = Sha256::new();
    digest.update(root.file_name().unwrap_or_default().as_encoded_bytes());
    for item in evidence {
        digest.update(item.relative.as_os_str().as_encoded_bytes());
        digest.update(item.content.as_bytes());
    }
    let mut surface_keys = surfaces
        .iter()
        .map(|surface| {
            format!(
                "{}:{}:{}",
                surface
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&surface.path)
                    .display(),
                surface.kind,
                audience_name(surface.significance.exposure.audience)
            )
        })
        .collect::<Vec<_>>();
    surface_keys.sort();
    surface_keys.dedup();
    for key in surface_keys {
        digest.update(key.as_bytes());
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn infer_frontends(evidence: &[EvidenceFile]) -> Vec<FrontendUse> {
    let mut matches: BTreeMap<FrontendKind, BTreeSet<PathBuf>> = BTreeMap::new();
    for item in evidence {
        let path = item.relative.to_string_lossy().to_ascii_lowercase();
        let text = item.content.as_str();
        let mut record = |kind| {
            matches
                .entry(kind)
                .or_default()
                .insert(item.relative.clone());
        };
        if path == "tui"
            || contains_any(
                text,
                &["ratatui", "crossterm", "bubbletea", "termui", "ncurses"],
            )
        {
            record(FrontendKind::Tui);
        }
        if contains_any(
            text,
            &[
                "clap",
                "structopt",
                "commander",
                "cobra",
                "click",
                "argparse",
            ],
        ) {
            record(FrontendKind::Cli);
        }
        if path.contains("tauri.conf")
            || contains_any(text, &["tauri", "electron", "gtk", "egui", "iced", "qt"])
        {
            record(FrontendKind::Gui);
        }
        if path == "web"
            || path == "frontend"
            || path == "index.html"
            || contains_any(
                text,
                &["react", "next", "vue", "svelte", "astro", "solid-js"],
            )
        {
            record(FrontendKind::Web);
        }
        if path == "android"
            || path == "ios"
            || path.contains("androidmanifest")
            || path.contains("info.plist")
            || contains_any(
                text,
                &[
                    "react-native",
                    "flutter",
                    "android.intent.category.launcher",
                ],
            )
        {
            record(FrontendKind::MobileApp);
        }
        if path == "tvos"
            || path.contains("tizen")
            || path.ends_with("appinfo.json")
            || contains_any(
                text,
                &[
                    "leanback_launcher",
                    "com.apple.product-type.application.watchapp",
                    "tvos",
                    "webos",
                ],
            )
        {
            record(FrontendKind::TvApp);
        }
    }
    matches
        .into_iter()
        .map(|(kind, evidence)| FrontendUse {
            kind,
            reason: format!(
                "{} project markers or dependencies identify a {} user interface",
                evidence.len(),
                kind.label()
            ),
            evidence: evidence.into_iter().collect(),
        })
        .collect()
}

fn contains_any(content: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| content.contains(needle))
}

fn summarize_surfaces(root: &Path, surfaces: &[Surface]) -> Vec<TargetedSurfaceSummary> {
    let mut groups: BTreeMap<String, (usize, BTreeSet<String>, BTreeSet<PathBuf>)> =
        BTreeMap::new();
    for surface in surfaces {
        let group = groups.entry(surface.kind.to_string()).or_default();
        group.0 += 1;
        group
            .1
            .insert(audience_name(surface.significance.exposure.audience).into());
        group.2.insert(
            surface
                .path
                .strip_prefix(root)
                .unwrap_or(&surface.path)
                .to_path_buf(),
        );
    }
    groups
        .into_iter()
        .map(|(kind, (count, audiences, paths))| TargetedSurfaceSummary {
            reason: surface_reason(&kind).into(),
            kind,
            count,
            audiences: audiences.into_iter().collect(),
            representative_paths: paths.into_iter().take(5).collect(),
        })
        .collect()
}

fn surface_reason(kind: &str) -> &'static str {
    match kind {
        "repo_doc" => {
            "Repository documents establish the project's public and contributor-facing identity."
        }
        "doc" => "Documentation communicates product capability, terminology, and voice.",
        "ui_string" => {
            "Interface copy is encountered directly by users of the identified frontends."
        }
        "style" => "Style sources implement the visual palette and presentation rules.",
        "social" => "Linked social accounts are both public brand surfaces and evidence sources.",
        _ => "The detected surface can communicate or govern project identity.",
    }
}

fn target_ontology(root: &Path, frontends: &[FrontendUse]) -> SoftwareOntology {
    let root_id = "target-project".to_string();
    let mut components = vec![SoftwareComponent {
        id: root_id.clone(),
        name: root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("target project")
            .into(),
        kind: SoftwareComponentKind::Application,
        path: Some(root.display().to_string()),
        boundary: established_boundary("top-level target project"),
        attributes: BTreeMap::from([("role".into(), "brand-target".into())]),
        evidence_ids: Vec::new(),
    }];
    let mut relationships = Vec::new();
    let mut confidence = BTreeMap::from([(root_id.clone(), 1.0)]);
    for frontend in frontends {
        let id = format!("frontend:{:?}", frontend.kind).to_ascii_lowercase();
        components.push(SoftwareComponent {
            id: id.clone(),
            name: frontend.kind.label().into(),
            kind: SoftwareComponentKind::Interface,
            path: frontend
                .evidence
                .first()
                .map(|path| root.join(path).display().to_string()),
            boundary: established_boundary(&frontend.reason),
            attributes: BTreeMap::from([
                ("frontend_kind".into(), frontend.kind.label().into()),
                ("target_use".into(), "user-facing".into()),
            ]),
            evidence_ids: frontend
                .evidence
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
        });
        relationships.push(SoftwareRelationship {
            source: root_id.clone(),
            target: id.clone(),
            kind: SoftwareRelationshipKind::Contains,
            confidence: 1.0,
            evidence_ids: frontend
                .evidence
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
        });
        confidence.insert(id, 1.0);
    }
    SoftwareOntology {
        schema_version: SOFTWARE_ONTOLOGY_SCHEMA_VERSION.into(),
        namespace: "brandi.target-use".into(),
        graph: ComponentGraph {
            root: root_id,
            components,
            relationships: RelationshipSet { relationships },
            decomposition_confidence: DecompositionConfidence {
                overall: 1.0,
                component_boundaries: confidence,
                unresolved_boundaries: Vec::new(),
            },
        },
        vocabulary: BTreeMap::from([
            (
                "frontend_kind".into(),
                "The form of user-facing application interface.".into(),
            ),
            (
                "target_use".into(),
                "Why Brandi treats a component as a presentation target.".into(),
            ),
        ]),
    }
}

fn established_boundary(reason: &str) -> TransformationBoundary {
    TransformationBoundary {
        independently_addressable: true,
        independently_transformable: true,
        boundary_established: true,
        granularity: 1,
        reasons: vec![reason.into()],
    }
}

fn audience_name(audience: ExposureAudience) -> &'static str {
    match audience {
        ExposureAudience::Public => "public",
        ExposureAudience::User => "user",
        ExposureAudience::Contributor => "contributor",
        ExposureAudience::Agent => "agent",
        ExposureAudience::Developer => "developer",
        ExposureAudience::None => "none",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SurfaceKind, SurfaceSignificance};

    fn surface(root: &Path, kind: SurfaceKind, path: &str) -> Surface {
        Surface {
            kind,
            path: root.join(path),
            line: 1,
            text: "copy".into(),
            context: "test".into(),
            confidence: 100,
            exposure: 5,
            significance: SurfaceSignificance::default(),
            reason: "test".into(),
            start_byte: 0,
            end_byte: 4,
        }
    }

    #[test]
    fn identifies_frontends_and_reuses_padagonia_inference() {
        let temporary = tempfile::tempdir().unwrap();
        fs::create_dir_all(temporary.path().join(".brandi/state")).unwrap();
        fs::create_dir(temporary.path().join("tui")).unwrap();
        fs::write(
            temporary.path().join("Cargo.toml"),
            "[dependencies]\nclap = '4'\nratatui = '1'\n",
        )
        .unwrap();
        let surfaces = vec![surface(
            temporary.path(),
            SurfaceKind::UiString,
            "src/main.rs",
        )];

        let first = identify(temporary.path(), &surfaces).unwrap();
        assert!(!first.padagonia.reused);
        assert_eq!(
            first
                .frontends
                .iter()
                .map(|frontend| frontend.kind)
                .collect::<Vec<_>>(),
            vec![FrontendKind::Cli, FrontendKind::Tui]
        );
        assert_eq!(first.targeted_surfaces[0].kind, "ui_string");

        let second = identify(temporary.path(), &surfaces).unwrap();
        assert!(second.padagonia.reused);
        assert_eq!(first.frontends, second.frontends);
    }

    #[test]
    fn invalidates_cached_target_use_when_frontend_evidence_changes() {
        let temporary = tempfile::tempdir().unwrap();
        fs::create_dir_all(temporary.path().join(".brandi/state")).unwrap();
        fs::write(
            temporary.path().join("package.json"),
            "{\"dependencies\":{}}",
        )
        .unwrap();
        let surfaces = vec![];
        let first = identify(temporary.path(), &surfaces).unwrap();
        assert!(first.frontends.is_empty());
        fs::write(
            temporary.path().join("package.json"),
            "{\"dependencies\":{\"react\":\"latest\"}}",
        )
        .unwrap();
        let second = identify(temporary.path(), &surfaces).unwrap();
        assert!(!second.padagonia.reused);
        assert_eq!(second.frontends[0].kind, FrontendKind::Web);
    }

    #[test]
    fn target_use_taxonomy_covers_every_supported_frontend_form() {
        let evidence = vec![
            EvidenceFile {
                relative: "Cargo.toml".into(),
                content: "clap ratatui".into(),
            },
            EvidenceFile {
                relative: "src-tauri/tauri.conf.json".into(),
                content: "tauri".into(),
            },
            EvidenceFile {
                relative: "package.json".into(),
                content: "react".into(),
            },
            EvidenceFile {
                relative: "AndroidManifest.xml".into(),
                content: "android.intent.category.launcher leanback_launcher".into(),
            },
        ];
        assert_eq!(
            infer_frontends(&evidence)
                .into_iter()
                .map(|frontend| frontend.kind)
                .collect::<Vec<_>>(),
            vec![
                FrontendKind::Cli,
                FrontendKind::Tui,
                FrontendKind::Gui,
                FrontendKind::Web,
                FrontendKind::MobileApp,
                FrontendKind::TvApp,
            ]
        );
    }
}
