//! Social/narrative planning derived from the brand brief.
//!
//! Turns `audience.yaml` (segments + narratives) into two human-readable
//! views: `render_graph` draws the capability → audience → format tree,
//! `render_plan` emits a per-format content checklist.

use crate::brief::{Brief, Narrative};
use crate::error::{BrandiError, Result};
use crate::types::{
    ExposureAudience, ExposureProfile, ExposureReach, ExposureVisibility, Surface, SurfaceDomain,
    SurfaceKind, SurfaceProvenance, SurfaceSignificance, SurfaceSubtype,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const SOCIAL_ACCOUNTS_SCHEMA_VERSION: &str = "brandi-social-accounts-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocialAccountRegistry {
    pub schema_version: String,
    #[serde(default)]
    pub accounts: Vec<SocialAccount>,
}

impl Default for SocialAccountRegistry {
    fn default() -> Self {
        Self {
            schema_version: SOCIAL_ACCOUNTS_SCHEMA_VERSION.into(),
            accounts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocialAccount {
    pub id: String,
    pub provider: String,
    pub handle: String,
    pub display_name: String,
    pub profile_url: String,
    #[serde(default)]
    pub bio: String,
    pub credential_env: String,
    pub connected_at: String,
    pub source: SocialAccountSource,
    pub surface: SocialAccountSurface,
    /// Runtime availability only. Credential values are never serialized.
    #[serde(default)]
    pub credential_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocialAccountSource {
    pub content: bool,
    pub metrics: bool,
    pub last_sync: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocialAccountSurface {
    pub profile: bool,
    pub publishing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectSocialAccount {
    pub provider: String,
    pub handle: String,
    pub display_name: String,
    pub profile_url: String,
    pub bio: String,
    pub credential_env: String,
}

pub fn load_accounts(root: &Path) -> Result<SocialAccountRegistry> {
    let path = accounts_path(root);
    if !path.exists() {
        return Ok(SocialAccountRegistry::default());
    }
    let mut registry: SocialAccountRegistry = serde_json::from_slice(&fs::read(&path)?)?;
    if registry.schema_version != SOCIAL_ACCOUNTS_SCHEMA_VERSION {
        return Err(BrandiError::Invalid(format!(
            "unsupported social account schema '{}'",
            registry.schema_version
        )));
    }
    for account in &mut registry.accounts {
        account.credential_available = credential_available(&account.credential_env);
    }
    registry.accounts.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then(left.handle.cmp(&right.handle))
    });
    Ok(registry)
}

pub fn connect_account(root: &Path, input: ConnectSocialAccount) -> Result<SocialAccountRegistry> {
    connect_account_with_status(root, input, None)
}

fn connect_account_with_status(
    root: &Path,
    input: ConnectSocialAccount,
    credential_status: Option<bool>,
) -> Result<SocialAccountRegistry> {
    validate_account(&input)?;
    let available =
        credential_status.unwrap_or_else(|| credential_available(&input.credential_env));
    if !available {
        return Err(BrandiError::Invalid(format!(
            "credential environment variable {} is missing or empty",
            input.credential_env
        )));
    }
    let mut registry = load_accounts(root)?;
    let id = account_id(&input.provider, &input.handle);
    let account = SocialAccount {
        id: id.clone(),
        provider: input.provider.trim().to_ascii_lowercase(),
        handle: normalize_handle(&input.handle),
        display_name: input.display_name.trim().to_string(),
        profile_url: input.profile_url.trim().to_string(),
        bio: input.bio.trim().to_string(),
        credential_env: input.credential_env.trim().to_string(),
        connected_at: Utc::now().to_rfc3339(),
        source: SocialAccountSource {
            content: true,
            metrics: true,
            last_sync: None,
        },
        surface: SocialAccountSurface {
            profile: true,
            publishing: true,
        },
        credential_available: true,
    };
    if let Some(existing) = registry.accounts.iter_mut().find(|item| item.id == id) {
        *existing = account;
    } else {
        registry.accounts.push(account);
    }
    write_accounts(root, &registry)?;
    load_accounts(root)
}

pub fn disconnect_account(
    root: &Path,
    id: &str,
    confirmation: &str,
) -> Result<SocialAccountRegistry> {
    if id != confirmation {
        return Err(BrandiError::Invalid(
            "disconnect confirmation must exactly match the account id".into(),
        ));
    }
    let mut registry = load_accounts(root)?;
    let before = registry.accounts.len();
    registry.accounts.retain(|account| account.id != id);
    if registry.accounts.len() == before {
        return Err(BrandiError::NotFound(format!("social account {id}")));
    }
    write_accounts(root, &registry)?;
    load_accounts(root)
}

pub fn account_surfaces(root: &Path) -> Result<Vec<Surface>> {
    let registry = load_accounts(root)?;
    let mut surfaces = Vec::new();
    for account in registry.accounts {
        let path = PathBuf::from(format!("social://{}/{}", account.provider, account.id));
        for (line, field, text) in [
            (1, "display_name", account.display_name),
            (2, "handle", account.handle),
            (3, "bio", account.bio),
            (4, "profile_url", account.profile_url),
        ] {
            if text.trim().is_empty() {
                continue;
            }
            surfaces.push(Surface {
                kind: SurfaceKind::Social,
                path: path.clone(),
                line,
                text: text.clone(),
                context: format!("{} profile {field}", account.provider),
                confidence: 100,
                exposure: 5,
                significance: social_significance(),
                reason: format!(
                    "linked social account {field}; account is both source and surface"
                ),
                start_byte: 0,
                end_byte: text.len(),
            });
        }
    }
    Ok(surfaces)
}

fn social_significance() -> SurfaceSignificance {
    SurfaceSignificance {
        domain: SurfaceDomain::Semantic,
        subtype: SurfaceSubtype::SocialProfile,
        provenance: SurfaceProvenance::Mirrored,
        exposure: ExposureProfile {
            audience: ExposureAudience::Public,
            reach: ExposureReach::Product,
            visibility: ExposureVisibility::Direct,
            score: 5,
        },
        relevance: 95,
        authority: 90,
        semantic_weight: 80,
    }
}

fn accounts_path(root: &Path) -> PathBuf {
    root.join(".brandi/social-accounts.json")
}

fn write_accounts(root: &Path, registry: &SocialAccountRegistry) -> Result<()> {
    let path = accounts_path(root);
    let parent = path
        .parent()
        .ok_or_else(|| BrandiError::Invalid("invalid account path".into()))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".social-accounts.{}.tmp", std::process::id()));
    let mut persisted = registry.clone();
    for account in &mut persisted.accounts {
        account.credential_available = false;
    }
    fs::write(&temporary, serde_json::to_vec_pretty(&persisted)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn credential_available(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| !value.is_empty())
}

fn validate_account(input: &ConnectSocialAccount) -> Result<()> {
    const PROVIDERS: [&str; 7] = [
        "x",
        "instagram",
        "linkedin",
        "mastodon",
        "bluesky",
        "youtube",
        "tiktok",
    ];
    let provider = input.provider.trim().to_ascii_lowercase();
    if !PROVIDERS.contains(&provider.as_str()) {
        return Err(BrandiError::Invalid(format!(
            "unsupported social provider '{}'",
            input.provider
        )));
    }
    if normalize_handle(&input.handle).is_empty() || input.display_name.trim().is_empty() {
        return Err(BrandiError::Invalid(
            "social handle and display name are required".into(),
        ));
    }
    if !matches!(input.profile_url.strip_prefix("https://"), Some(rest) if !rest.is_empty()) {
        return Err(BrandiError::Invalid(
            "social profile URL must use https".into(),
        ));
    }
    let credential = input.credential_env.trim();
    if credential.is_empty()
        || !credential.chars().enumerate().all(|(index, character)| {
            character == '_'
                || character.is_ascii_uppercase()
                || (index > 0 && character.is_ascii_digit())
        })
    {
        return Err(BrandiError::Invalid(
            "credential environment variable must use uppercase shell-variable syntax".into(),
        ));
    }
    if input.bio.len() > 500 || input.display_name.len() > 120 || input.handle.len() > 120 {
        return Err(BrandiError::Invalid(
            "social account metadata exceeds its safety limit".into(),
        ));
    }
    Ok(())
}

fn normalize_handle(handle: &str) -> String {
    handle.trim().trim_start_matches('@').to_string()
}

fn account_id(provider: &str, handle: &str) -> String {
    let handle = normalize_handle(handle)
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!("{}-{handle}", provider.trim().to_ascii_lowercase())
}

/// Render the narrative graph (capabilities → audiences → formats) as text.
///
/// The product name is the root; each narrative is a branch holding its core
/// message, its audience segments (descriptions resolved from
/// `audience.segments` when the name is known), and its formats as leaves.
pub fn render_graph(brief: &Brief) -> String {
    let product = match brief.identity.product.name.trim() {
        "" => "unnamed product",
        name => name,
    };
    let mut out: Vec<String> = vec![format!("{product} — content graph")];

    let narratives = &brief.audience.narratives;
    if narratives.is_empty() {
        out.push("no narratives defined in audience.yaml".to_string());
        return out.join("\n");
    }

    for (i, narrative) in narratives.iter().enumerate() {
        let last = i + 1 == narratives.len();
        let (branch, pad) = if last {
            ("└─ ", "   ")
        } else {
            ("├─ ", "│  ")
        };
        out.push(format!("{branch}{}", narrative.capability));
        out.push(format!("{pad}├─ narrative: {}", narrative.narrative));

        if narrative.audiences.is_empty() {
            out.push(format!("{pad}├─ audiences: (none)"));
        } else {
            out.push(format!("{pad}├─ audiences:"));
            for (j, name) in narrative.audiences.iter().enumerate() {
                let twig = if j + 1 == narrative.audiences.len() {
                    "└─ "
                } else {
                    "├─ "
                };
                let description = brief
                    .audience
                    .segments
                    .iter()
                    .find(|s| &s.name == name)
                    .map(|s| s.description.trim())
                    .filter(|d| !d.is_empty());
                match description {
                    Some(d) => out.push(format!("{pad}│  {twig}{name} — {d}")),
                    None => out.push(format!("{pad}│  {twig}{name}")),
                }
            }
        }

        if narrative.formats.is_empty() {
            out.push(format!("{pad}└─ formats: (none)"));
        } else {
            out.push(format!("{pad}└─ formats: {}", narrative.formats.join(", ")));
        }
    }

    out.join("\n")
}

/// Render a content plan, optionally filtered to one audience segment.
///
/// Every included narrative becomes a checkbox header (`[ ] capability —
/// "message"`) with one nested TODO line per format, followed by a
/// `N narratives, M content items` summary. When `segment` is `Some` but
/// matches no narrative, the segments that do exist are listed instead.
pub fn render_plan(brief: &Brief, segment: Option<&str>) -> String {
    let narratives: Vec<&Narrative> = brief
        .audience
        .narratives
        .iter()
        .filter(|n| match segment {
            Some(name) => n.audiences.iter().any(|a| a == name),
            None => true,
        })
        .collect();

    let mut out: Vec<String> = Vec::new();
    match segment {
        Some(name) => out.push(format!("Content plan — {name}")),
        None => out.push("Content plan — all audiences".to_string()),
    }
    out.push(String::new());

    if let Some(name) = segment {
        if narratives.is_empty() {
            out.push(format!("no narratives target segment '{name}'"));
            let available: Vec<&str> = brief
                .audience
                .segments
                .iter()
                .map(|s| s.name.as_str())
                .collect();
            if available.is_empty() {
                out.push("no segments are defined in audience.yaml".to_string());
            } else {
                out.push(format!("available segments: {}", available.join(", ")));
            }
            return out.join("\n");
        }
    }

    if narratives.is_empty() {
        out.push("no narratives defined in audience.yaml".to_string());
    }

    let mut items = 0usize;
    for (i, narrative) in narratives.iter().enumerate() {
        if i > 0 {
            out.push(String::new());
        }
        out.push(format!(
            "[ ] {} — \"{}\"",
            narrative.capability, narrative.narrative
        ));
        for format in &narrative.formats {
            out.push(format!("    [ ] {format}"));
            items += 1;
        }
    }

    out.push(String::new());
    out.push(format!(
        "{} narratives, {items} content items",
        narratives.len()
    ));
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scaffold a default brief in a tempdir and load it back.
    /// The TempDir is returned so it stays alive for the test.
    fn scaffolded_brief() -> (tempfile::TempDir, Brief) {
        let tmp = tempfile::tempdir().unwrap();
        Brief::scaffold(tmp.path()).unwrap();
        let brief = Brief::load(tmp.path()).unwrap();
        (tmp, brief)
    }

    /// Scaffold, then replace `audience.yaml` with custom content.
    fn brief_with_audience(yaml: &str) -> (tempfile::TempDir, Brief) {
        let (tmp, _default) = scaffolded_brief();
        std::fs::write(tmp.path().join(".brandi/audience.yaml"), yaml).unwrap();
        let brief = Brief::load(tmp.path()).unwrap();
        (tmp, brief)
    }

    /// Two segments, two narratives, one per segment.
    const TWO_NARRATIVE_AUDIENCE: &str = r#"segments:
  - name: platform_engineers
    description: "Teams responsible for engineering governance"
    pains: ["identity drift"]
  - name: founders
    description: "People starting companies"
    pains: []
narratives:
  - capability: "brand linting"
    narrative: "prevent identity drift before it ships"
    audiences: [platform_engineers]
    formats: [x_post, blog, demo_video, docs]
  - capability: "visual checks"
    narrative: "catch off-brand screenshots before launch"
    audiences: [founders]
    formats: [blog]
"#;

    #[test]
    fn render_graph_draws_tree_with_segment_descriptions() {
        let (_tmp, brief) = scaffolded_brief();
        let graph = render_graph(&brief);
        assert!(graph.contains("Brandi — content graph"), "got:\n{graph}");
        assert!(graph.contains("└─ brand linting"), "got:\n{graph}");
        assert!(
            graph.contains("├─ narrative: prevent identity drift before it ships"),
            "got:\n{graph}"
        );
        assert!(graph.contains("├─ audiences:"), "got:\n{graph}");
        assert!(
            graph.contains("└─ platform_engineers — Teams responsible for engineering governance"),
            "got:\n{graph}"
        );
        assert!(
            graph.contains("└─ formats: x_post, blog, demo_video, docs"),
            "got:\n{graph}"
        );
    }

    #[test]
    fn render_graph_handles_unknown_segments_and_empty_formats() {
        let (_tmp, brief) = brief_with_audience(
            "segments: []\nnarratives:\n  - capability: \"c\"\n    narrative: \"n\"\n    audiences: [mystery]\n    formats: []\n",
        );
        let graph = render_graph(&brief);
        // Unknown segment names are still listed, without a description.
        assert!(graph.contains("└─ mystery"), "got:\n{graph}");
        assert!(!graph.contains("mystery —"), "got:\n{graph}");
        // Empty format lists degrade gracefully.
        assert!(graph.contains("└─ formats: (none)"), "got:\n{graph}");
    }

    #[test]
    fn render_graph_empty_narratives_is_friendly() {
        let (_tmp, brief) = brief_with_audience("segments: []\nnarratives: []\n");
        let graph = render_graph(&brief);
        assert!(
            graph.contains("no narratives defined in audience.yaml"),
            "got:\n{graph}"
        );
    }

    #[test]
    fn render_plan_filtered_lists_only_matching_narratives() {
        let (_tmp, brief) = brief_with_audience(TWO_NARRATIVE_AUDIENCE);
        let plan = render_plan(&brief, Some("platform_engineers"));
        assert!(
            plan.contains("Content plan — platform_engineers"),
            "got:\n{plan}"
        );
        assert!(
            plan.contains("[ ] brand linting — \"prevent identity drift before it ships\""),
            "got:\n{plan}"
        );
        assert!(plan.contains("    [ ] demo_video"), "got:\n{plan}");
        assert!(!plan.contains("visual checks"), "got:\n{plan}");
        assert!(
            plan.contains("1 narratives, 4 content items"),
            "got:\n{plan}"
        );
    }

    #[test]
    fn render_plan_without_segment_covers_all_narratives() {
        let (_tmp, brief) = brief_with_audience(TWO_NARRATIVE_AUDIENCE);
        let plan = render_plan(&brief, None);
        assert!(plan.contains("brand linting"), "got:\n{plan}");
        assert!(plan.contains("visual checks"), "got:\n{plan}");
        assert!(
            plan.contains("2 narratives, 5 content items"),
            "got:\n{plan}"
        );
    }

    #[test]
    fn render_plan_unknown_segment_lists_available_segments() {
        let (_tmp, brief) = brief_with_audience(TWO_NARRATIVE_AUDIENCE);
        let plan = render_plan(&brief, Some("aliens"));
        assert!(
            plan.contains("no narratives target segment 'aliens'"),
            "got:\n{plan}"
        );
        assert!(
            plan.contains("available segments: platform_engineers, founders"),
            "got:\n{plan}"
        );
        // No plan body / summary when nothing matched.
        assert!(!plan.contains("content items"), "got:\n{plan}");
    }

    fn account_input() -> ConnectSocialAccount {
        ConnectSocialAccount {
            provider: "mastodon".into(),
            handle: "@brandi".into(),
            display_name: "Brandi".into(),
            profile_url: "https://social.example/@brandi".into(),
            bio: "Brand coherence intelligence".into(),
            credential_env: "BRANDI_MASTODON_TOKEN".into(),
        }
    }

    #[test]
    fn connected_accounts_persist_references_not_credentials() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            connect_account_with_status(root.path(), account_input(), Some(true)).unwrap();
        assert_eq!(registry.accounts.len(), 1);
        let stored =
            std::fs::read_to_string(root.path().join(".brandi/social-accounts.json")).unwrap();
        assert!(stored.contains("BRANDI_MASTODON_TOKEN"));
        assert!(stored.contains("\"credential_available\": false"));
        assert!(!stored.contains("secret"));
    }

    #[test]
    fn accounts_project_into_public_source_and_surface_evidence() {
        let root = tempfile::tempdir().unwrap();
        connect_account_with_status(root.path(), account_input(), Some(true)).unwrap();
        let account = &load_accounts(root.path()).unwrap().accounts[0];
        assert!(account.source.content && account.source.metrics);
        assert!(account.surface.profile && account.surface.publishing);
        let surfaces = account_surfaces(root.path()).unwrap();
        assert!(surfaces.len() >= 4);
        assert!(surfaces.iter().all(|surface| {
            surface.kind == SurfaceKind::Social
                && surface.significance.exposure.audience == ExposureAudience::Public
        }));
    }

    #[test]
    fn disconnect_requires_exact_account_id_confirmation() {
        let root = tempfile::tempdir().unwrap();
        let registry =
            connect_account_with_status(root.path(), account_input(), Some(true)).unwrap();
        let id = registry.accounts[0].id.clone();
        assert!(disconnect_account(root.path(), &id, "wrong").is_err());
        assert!(disconnect_account(root.path(), &id, &id)
            .unwrap()
            .accounts
            .is_empty());
    }
}
