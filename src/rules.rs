//! Lint rules: check discovered surfaces against the brand brief and
//! guidelines.
//!
//! Ten rules, each a small function over the brief and/or guidelines plus
//! either discovered surfaces (per-surface rules) or files read from disk
//! (whole-file rules). Findings are capped at
//! `MAX_FINDINGS_PER_RULE_PER_FILE` per rule per file, keeping the first
//! ones silently; per-rule severity overrides from the optional
//! `.brandi/rules.yaml` are applied last.

use crate::brief::Brief;
use crate::error::Result;
use crate::guidelines::{parse_hex_color, Guidelines, RuleOverrides};
use crate::surface::{scan_file, scan_surfaces, strip_fenced_code};
use crate::types::{
    ExposureAudience, Finding, FindingClass, FindingSemantics, SemanticRole, Severity, Surface,
    SurfaceDomain, SurfaceKind, SurfaceProvenance,
};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Maximum findings kept per rule per file; extras are dropped silently.
const MAX_FINDINGS_PER_RULE_PER_FILE: usize = 10;

/// All rules as (rule_id, one-line description), in stable catalog order.
pub fn rule_catalog() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "prohibited-term",
            "prohibited words and phrases from the guidelines",
        ),
        (
            "terminology-variant",
            "banned terminology variants, former product names, and product-name casing",
        ),
        (
            "error-prefix",
            "machine-style error prefixes and numeric error codes in UI strings",
        ),
        ("error-actionable", "error messages without a next step"),
        (
            "exclamation-limit",
            "files using more exclamation marks than the voice allows",
        ),
        (
            "sentence-case-headings",
            "Title Case headings where sentence case is required",
        ),
        (
            "readme-mission",
            "README introduction missing the product name or mission",
        ),
        (
            "readme-image-refs",
            "broken image references in markdown files",
        ),
        (
            "palette-adherence",
            "colors outside the brand palette tolerance",
        ),
        (
            "readme-h1",
            "README not opening with an H1 that names the product",
        ),
        (
            "accessibility-language",
            "optional ambiguous or exclusionary interaction language",
        ),
        (
            "inclusive-terminology",
            "optional non-inclusive legacy terminology",
        ),
        (
            "reading-level",
            "optional sentences that exceed the configured plain-language budget",
        ),
        (
            "localization-readiness",
            "optional interpolation patterns that are difficult to translate",
        ),
        (
            "terminology-ownership",
            "optional product terminology used without an ownership marker",
        ),
    ]
}

/// Run every lint rule over the whole project rooted at `root`.
pub fn run_rules(brief: &Brief, guidelines: &Guidelines, root: &Path) -> Result<Vec<Finding>> {
    let surfaces = scan_surfaces(root)?;
    Ok(run_rules_on_surfaces(brief, guidelines, root, &surfaces))
}

/// Evaluate a previously completed immutable scan. This avoids extractor
/// drift and duplicate repository traversal within one lint operation.
pub fn run_rules_on_surfaces(
    brief: &Brief,
    guidelines: &Guidelines,
    root: &Path,
    surfaces: &[Surface],
) -> Vec<Finding> {
    let docs = doc_files(surfaces);
    let mut findings = Vec::new();
    findings.extend(prohibited_terms(guidelines, surfaces));
    findings.extend(terminology_variants(brief, surfaces));
    findings.extend(error_prefixes(guidelines, surfaces));
    findings.extend(error_actionable(guidelines, surfaces));
    for (path, kind) in &docs {
        findings.extend(exclamation_limit_file(guidelines, path, *kind));
    }
    findings.extend(sentence_case_headings(brief, guidelines, surfaces));
    findings.extend(readme_mission(brief, root));
    for (path, kind) in &docs {
        findings.extend(readme_image_refs_file(path, *kind));
    }
    findings.extend(palette_adherence(guidelines, surfaces));
    findings.extend(readme_h1(brief, root));
    if optional_rule_enabled(&guidelines.rules_overrides, "accessibility-language") {
        findings.extend(term_pattern_findings(
            surfaces,
            "accessibility-language",
            &["click here", "simply", "obviously", "crazy", "blind spot"],
            "Use specific, ability-neutral language that names the action.",
        ));
    }
    if optional_rule_enabled(&guidelines.rules_overrides, "inclusive-terminology") {
        findings.extend(term_pattern_findings(
            surfaces,
            "inclusive-terminology",
            &["whitelist", "blacklist", "master/slave"],
            "Use allowlist/denylist or primary/replica terminology.",
        ));
    }
    if optional_rule_enabled(&guidelines.rules_overrides, "reading-level") {
        findings.extend(reading_level_findings(surfaces));
    }
    if optional_rule_enabled(&guidelines.rules_overrides, "localization-readiness") {
        findings.extend(localization_findings(surfaces));
    }
    if optional_rule_enabled(&guidelines.rules_overrides, "terminology-ownership") {
        findings.extend(terminology_ownership_findings(brief, surfaces));
    }
    let findings = apply_overrides(cap_findings(findings), &guidelines.rules_overrides);
    annotate_findings(findings, surfaces)
}

fn annotate_findings(mut findings: Vec<Finding>, surfaces: &[Surface]) -> Vec<Finding> {
    for finding in &mut findings {
        let evidence = surfaces.iter().find(|surface| {
            surface.path == finding.path
                && finding.line.is_none_or(|line| line == surface.line)
                && finding.kind.is_none_or(|kind| kind == surface.kind)
        });
        let role = match finding.rule_id.as_str() {
            "sentence-case-headings" => SemanticRole::Heading,
            "palette-adherence" => SemanticRole::ColorToken,
            "exclamation-limit" | "readme-mission" | "readme-h1" => SemanticRole::DocumentStructure,
            _ if finding.kind == Some(SurfaceKind::UiString) => SemanticRole::UiCopy,
            _ if finding.kind == Some(SurfaceKind::Social) => SemanticRole::SocialProfile,
            _ => SemanticRole::Prose,
        };
        let impact = match role {
            SemanticRole::ColorToken | SemanticRole::UiCopy => 85,
            SemanticRole::SocialProfile => 90,
            SemanticRole::Heading => 70,
            SemanticRole::DocumentStructure => 60,
            SemanticRole::Prose => 50,
            SemanticRole::Typography => 45,
        };
        let (domain, provenance, audience, confidence, relevance, authority, exposure, weight) =
            evidence.map_or(
                (
                    SurfaceDomain::Semantic,
                    SurfaceProvenance::Authored,
                    ExposureAudience::Developer,
                    100,
                    70,
                    70,
                    3,
                    70,
                ),
                |surface| {
                    (
                        surface.significance.domain,
                        surface.significance.provenance,
                        surface.significance.exposure.audience,
                        surface.confidence,
                        surface.significance.relevance,
                        surface.significance.authority,
                        surface.significance.exposure.score,
                        surface.significance.semantic_weight,
                    )
                },
            );
        let priority = u64::from(confidence)
            .saturating_mul(u64::from(relevance))
            .saturating_mul(u64::from(authority))
            .saturating_mul(u64::from(exposure.max(1)))
            .saturating_mul(u64::from(impact))
            .saturating_mul(u64::from(weight))
            .div_ceil(500_000_000)
            .min(100);
        finding.semantics = FindingSemantics {
            class: FindingClass::Violation,
            role,
            domain,
            provenance,
            audience,
            confidence,
            relevance,
            authority,
            exposure,
            impact,
            priority,
        };
    }
    findings
}

fn optional_rule_enabled(overrides: &RuleOverrides, rule_id: &str) -> bool {
    matches!(
        overrides.overrides.get(rule_id).map(String::as_str),
        Some("error" | "warning" | "info")
    )
}

fn term_pattern_findings(
    surfaces: &[Surface],
    rule_id: &str,
    terms: &[&str],
    suggestion: &str,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for surface in surfaces {
        let lower = surface.text.to_ascii_lowercase();
        for term in terms {
            if lower.contains(term) {
                findings.push(Finding {
                    rule_id: rule_id.into(),
                    severity: Severity::Warning,
                    kind: Some(surface.kind),
                    path: surface.path.clone(),
                    line: Some(surface.line),
                    message: format!("optional policy matched {term:?}"),
                    suggestion: Some(suggestion.into()),
                    semantics: FindingSemantics::default(),
                });
            }
        }
    }
    findings
}

fn reading_level_findings(surfaces: &[Surface]) -> Vec<Finding> {
    surfaces
        .iter()
        .filter(|surface| surface.text.split_whitespace().count() > 35)
        .map(|surface| Finding {
            rule_id: "reading-level".into(),
            severity: Severity::Info,
            kind: Some(surface.kind),
            path: surface.path.clone(),
            line: Some(surface.line),
            message: "sentence exceeds the 35-word plain-language budget".into(),
            suggestion: Some("Split the sentence or remove unnecessary clauses.".into()),
            semantics: FindingSemantics::default(),
        })
        .collect()
}

fn localization_findings(surfaces: &[Surface]) -> Vec<Finding> {
    surfaces
        .iter()
        .filter(|surface| {
            surface.kind == SurfaceKind::UiString
                && (surface.text.contains("${")
                    || surface.text.matches("{}").count() > 1
                    || surface.text.contains(" + "))
        })
        .map(|surface| Finding {
            rule_id: "localization-readiness".into(),
            severity: Severity::Info,
            kind: Some(surface.kind),
            path: surface.path.clone(),
            line: Some(surface.line),
            message: "message composition may prevent translators from reordering content".into(),
            suggestion: Some(
                "Use one named localization message with explicit placeholders.".into(),
            ),
            semantics: FindingSemantics::default(),
        })
        .collect()
}

fn terminology_ownership_findings(brief: &Brief, surfaces: &[Surface]) -> Vec<Finding> {
    if !brief.identity.product.name.trim().is_empty() {
        return Vec::new();
    }
    surfaces
        .first()
        .map(|surface| Finding {
            rule_id: "terminology-ownership".into(),
            severity: Severity::Warning,
            kind: Some(surface.kind),
            path: surface.path.clone(),
            line: Some(surface.line),
            message: "product terminology has no configured owner".into(),
            suggestion: Some("Set product.name in .brandi/identity.yaml.".into()),
            semantics: FindingSemantics::default(),
        })
        .into_iter()
        .collect()
}

/// Run every applicable lint rule over a single file.
///
/// Per-surface rules always run on the file's surfaces. Whole-file rules
/// apply only to RepoDoc/Doc files (`exclamation-limit`, `readme-image-refs`)
/// and to `<root>/README.md` (`readme-mission`, `readme-h1`). A file with no
/// surfaces has no lintable content, so whole-file doc rules are skipped.
pub fn lint_file(
    brief: &Brief,
    guidelines: &Guidelines,
    root: &Path,
    file: &Path,
) -> Result<Vec<Finding>> {
    let surfaces = scan_file(root, file)?;
    let mut findings = Vec::new();
    findings.extend(prohibited_terms(guidelines, &surfaces));
    findings.extend(terminology_variants(brief, &surfaces));
    findings.extend(error_prefixes(guidelines, &surfaces));
    findings.extend(error_actionable(guidelines, &surfaces));
    findings.extend(sentence_case_headings(brief, guidelines, &surfaces));
    findings.extend(palette_adherence(guidelines, &surfaces));
    if optional_rule_enabled(&guidelines.rules_overrides, "accessibility-language") {
        findings.extend(term_pattern_findings(
            &surfaces,
            "accessibility-language",
            &["click here", "simply", "obviously", "crazy", "blind spot"],
            "Use specific, ability-neutral language that names the action.",
        ));
    }
    if optional_rule_enabled(&guidelines.rules_overrides, "inclusive-terminology") {
        findings.extend(term_pattern_findings(
            &surfaces,
            "inclusive-terminology",
            &["whitelist", "blacklist", "master/slave"],
            "Use allowlist/denylist or primary/replica terminology.",
        ));
    }
    if optional_rule_enabled(&guidelines.rules_overrides, "reading-level") {
        findings.extend(reading_level_findings(&surfaces));
    }
    if optional_rule_enabled(&guidelines.rules_overrides, "localization-readiness") {
        findings.extend(localization_findings(&surfaces));
    }
    if optional_rule_enabled(&guidelines.rules_overrides, "terminology-ownership") {
        findings.extend(terminology_ownership_findings(brief, &surfaces));
    }

    let abs = if file.is_absolute() {
        file.to_path_buf()
    } else {
        root.join(file)
    };
    if let Some(kind) = surfaces.first().map(|s| s.kind) {
        if matches!(kind, SurfaceKind::RepoDoc | SurfaceKind::Doc) {
            findings.extend(exclamation_limit_file(guidelines, &abs, kind));
            findings.extend(readme_image_refs_file(&abs, kind));
        }
    }
    if is_root_readme(root, &abs) {
        findings.extend(readme_mission(brief, root));
        findings.extend(readme_h1(brief, root));
    }
    Ok(apply_overrides(
        cap_findings(findings),
        &guidelines.rules_overrides,
    ))
}

/// Map every discovered RepoDoc/Doc file to its kind (sorted by path).
fn doc_files(surfaces: &[Surface]) -> BTreeMap<PathBuf, SurfaceKind> {
    let mut files = BTreeMap::new();
    for s in surfaces {
        if matches!(s.kind, SurfaceKind::RepoDoc | SurfaceKind::Doc) {
            files.entry(s.path.clone()).or_insert(s.kind);
        }
    }
    files
}

/// True when `abs_file` is exactly `<root>/README.md`.
fn is_root_readme(root: &Path, abs_file: &Path) -> bool {
    abs_file
        .strip_prefix(root)
        .is_ok_and(|rel| rel == Path::new("README.md"))
}

/// Keep the first `MAX_FINDINGS_PER_RULE_PER_FILE` findings per (rule, file);
/// a group that overflows the cap gets one extra info-severity finding
/// saying how many were hidden, so truncation is visible in both the human
/// report and the JSON output instead of vanishing silently.
fn cap_findings(findings: Vec<Finding>) -> Vec<Finding> {
    let mut counts: BTreeMap<(String, PathBuf), usize> = BTreeMap::new();
    let mut kept: Vec<Finding> = Vec::with_capacity(findings.len());
    for f in findings {
        let key = (f.rule_id.clone(), f.path.clone());
        let n = counts.entry(key).or_insert(0);
        *n += 1;
        if *n <= MAX_FINDINGS_PER_RULE_PER_FILE {
            kept.push(f);
        }
    }
    for ((rule_id, path), total) in &counts {
        if *total > MAX_FINDINGS_PER_RULE_PER_FILE {
            let hidden = total - MAX_FINDINGS_PER_RULE_PER_FILE;
            let (plural, verb) = if hidden == 1 {
                ("", "was")
            } else {
                ("s", "were")
            };
            kept.push(Finding {
                rule_id: rule_id.clone(),
                severity: Severity::Info,
                kind: None,
                path: path.clone(),
                line: None,
                message: format!(
                    "{hidden} more '{rule_id}' finding{plural} in this file \
                     {verb} omitted (showing the first {MAX_FINDINGS_PER_RULE_PER_FILE})"
                ),
                suggestion: None,
                semantics: FindingSemantics::default(),
            });
        }
    }
    kept
}

/// Warn on stderr about `.brandi/rules.yaml` overrides that `brandi
/// guidelines validate` would reject as hard errors: an unknown rule id, or
/// a value other than off/error/warning/info. `lint`/`check`/`daemon` never
/// call full guidelines validation, so without this a typo (e.g. `Off`
/// instead of `off`) silently leaves the rule running at its default
/// severity with no indication anything was wrong.
fn warn_on_invalid_overrides(overrides: &RuleOverrides) {
    let known_rules: BTreeSet<&str> = rule_catalog().into_iter().map(|(id, _)| id).collect();
    for (rule_id, value) in &overrides.overrides {
        if !known_rules.contains(rule_id.as_str()) {
            let _ = crate::output::warning(format_args!(
                ".brandi/rules.yaml overrides unknown rule '{rule_id}'; \
                 run `brandi guidelines validate` to see the rule catalog"
            ));
        } else if !matches!(value.as_str(), "off" | "error" | "warning" | "info") {
            let _ = crate::output::warning(format_args!(
                ".brandi/rules.yaml sets '{rule_id}' to '{value}', which is not \
                 off/error/warning/info; the rule keeps its default severity — run \
                 `brandi guidelines validate` to confirm"
            ));
        }
    }
}

/// Apply per-rule severity overrides from `.brandi/rules.yaml`: `off` drops
/// the rule's findings entirely, any other value replaces the severity.
fn apply_overrides(findings: Vec<Finding>, overrides: &RuleOverrides) -> Vec<Finding> {
    warn_on_invalid_overrides(overrides);
    findings
        .into_iter()
        .filter_map(
            |mut f| match overrides.overrides.get(&f.rule_id).map(String::as_str) {
                Some("off") => None,
                Some("error") => {
                    f.severity = Severity::Error;
                    Some(f)
                }
                Some("warning") => {
                    f.severity = Severity::Warning;
                    Some(f)
                }
                Some("info") => {
                    f.severity = Severity::Info;
                    Some(f)
                }
                _ => Some(f),
            },
        )
        .collect()
}

/// Map a prohibited-category severity string ("error"|"warning"|"info") to
/// `Severity`; missing or unknown values default to warning.
fn prohibited_severity(value: Option<&String>) -> Severity {
    match value.map(String::as_str) {
        Some("error") => Severity::Error,
        Some("info") => Severity::Info,
        _ => Severity::Warning,
    }
}

/// Case-insensitive word-boundary regex for a literal phrase.
fn word_phrase_regex(phrase: &str) -> Option<Regex> {
    if phrase.trim().is_empty() {
        return None;
    }
    Regex::new(&format!(r"(?i)\b{}\b", regex::escape(phrase))).ok()
}

/// Word-boundary regex matching any of `words`; `None` when the list is empty.
fn any_word_regex(words: &[String]) -> Option<Regex> {
    if words.is_empty() {
        return None;
    }
    let alts: Vec<String> = words.iter().map(|w| regex::escape(w)).collect();
    Regex::new(&format!(r"\b(?:{})\b", alts.join("|"))).ok()
}

/// Rule `prohibited-term`: match prohibited phrases (case-insensitive,
/// word-boundary) against RepoDoc/Doc/UiString surface text.
fn prohibited_terms(guidelines: &Guidelines, surfaces: &[Surface]) -> Vec<Finding> {
    // One precompiled matcher per (category, phrase).
    let mut matchers: Vec<(&str, &str, Severity, Regex)> = Vec::new();
    for (category, phrases) in &guidelines.prohibited.categories {
        let severity = prohibited_severity(guidelines.prohibited.severity.get(category));
        for phrase in phrases {
            if let Some(re) = word_phrase_regex(phrase) {
                matchers.push((category, phrase, severity, re));
            }
        }
    }
    let mut out = Vec::new();
    for s in surfaces {
        if !matches!(
            s.kind,
            SurfaceKind::RepoDoc | SurfaceKind::Doc | SurfaceKind::UiString | SurfaceKind::Social
        ) {
            continue;
        }
        for (category, phrase, severity, re) in &matchers {
            if re.is_match(&s.text) {
                out.push(Finding {
                    rule_id: "prohibited-term".to_string(),
                    severity: *severity,
                    kind: Some(s.kind),
                    path: s.path.clone(),
                    line: Some(s.line),
                    message: format!("'{phrase}' is prohibited ({category})"),
                    suggestion: Some(format!("remove or rephrase '{phrase}'")),
                    semantics: FindingSemantics::default(),
                });
            }
        }
    }
    out
}

/// Replace inline code spans (`...`) with a space so code is never linted.
/// An unterminated backtick leaves the rest of the line as normal text.
fn strip_inline_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '`' {
            out.push(c);
            continue;
        }
        let mut closed = false;
        let mut buf = String::new();
        for c2 in chars.by_ref() {
            if c2 == '`' {
                closed = true;
                break;
            }
            buf.push(c2);
        }
        out.push(' ');
        if !closed {
            // No closing backtick: keep the buffered content as normal text.
            out.push_str(&buf);
            break;
        }
    }
    out
}

/// Rule `terminology-variant`: banned variants and former names (Error),
/// plus product-name casing drift (Warning), in documents and social profiles.
fn terminology_variants(brief: &Brief, surfaces: &[Surface]) -> Vec<Finding> {
    let product = &brief.identity.product;
    // (found, correct, matcher, is_former_name)
    let mut banned: Vec<(String, String, Regex, bool)> = Vec::new();
    for (variant, correct) in &brief.identity.terminology.banned_variants {
        if let Some(re) = word_phrase_regex(variant) {
            banned.push((variant.clone(), correct.clone(), re, false));
        }
    }
    for former in &product.former_names {
        if let Some(re) = word_phrase_regex(former) {
            banned.push((former.clone(), product.name.clone(), re, true));
        }
    }
    let Ok(word_re) = Regex::new(r"[\w-]+") else {
        return Vec::new();
    };
    // A single-word banned variant that happens to be a casing variant of
    // the product name (e.g. banning "BRANDI" to forbid shouting the name)
    // would otherwise trip both the banned-variant check above and the
    // casing-drift check below for the same occurrence; skip the latter
    // when a match is already covered by a banned variant.
    let banned_lower: BTreeSet<String> = banned
        .iter()
        .map(|(found, ..)| found.to_lowercase())
        .collect();

    let mut out = Vec::new();
    for s in surfaces {
        if !matches!(
            s.kind,
            SurfaceKind::RepoDoc | SurfaceKind::Doc | SurfaceKind::Social
        ) {
            continue;
        }
        let text = strip_inline_code(&s.text);
        for (found, correct, re, is_former) in &banned {
            if !re.is_match(&text) {
                continue;
            }
            let message = if *is_former {
                format!("'{found}' is a former product name")
            } else {
                format!("'{found}' is a banned terminology variant")
            };
            out.push(Finding {
                rule_id: "terminology-variant".to_string(),
                severity: Severity::Error,
                kind: Some(s.kind),
                path: s.path.clone(),
                line: Some(s.line),
                message,
                suggestion: Some(format!("use '{correct}' instead")),
                semantics: FindingSemantics::default(),
            });
        }
        if product.name.is_empty() {
            continue;
        }
        for m in word_re.find_iter(&text) {
            let word = m.as_str();
            if banned_lower.contains(&word.to_lowercase()) {
                continue;
            }
            if word.eq_ignore_ascii_case(&product.name)
                && word != product.name
                && !product.aliases.iter().any(|a| a == word)
            {
                out.push(Finding {
                    rule_id: "terminology-variant".to_string(),
                    severity: Severity::Warning,
                    kind: Some(s.kind),
                    path: s.path.clone(),
                    line: Some(s.line),
                    message: format!("inconsistent product naming: '{word}'"),
                    suggestion: Some(format!("use '{}'", product.name)),
                    semantics: FindingSemantics::default(),
                });
            }
        }
    }
    out
}

/// Lowercase everything, then uppercase the first character.
fn sentence_case(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut chars = lower.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Human-phrased rewrite of an error string with `strip_len` bytes of
/// machine prefix removed: drop leading separators, then sentence-case.
fn rewrite_error_message(text: &str, strip_len: usize) -> Option<String> {
    let rest = text[strip_len..]
        .trim_start_matches(|c: char| c == ':' || c == '-' || c == '.' || c.is_whitespace());
    if rest.is_empty() {
        return Some("drop the prefix and phrase the problem as a sentence".to_string());
    }
    Some(sentence_case(rest))
}

/// Rule `error-prefix`: UI strings starting with a machine-style prefix or a
/// numeric error code. One finding per surface; numeric codes are preferred
/// over prefixes when stripping, then the longest matching prefix.
fn error_prefixes(guidelines: &Guidelines, surfaces: &[Surface]) -> Vec<Finding> {
    let style = &guidelines.voice.style.error_messages;
    // Zero-or-more non-alphanumeric separator so a code glued directly to
    // the word ("Error402:") or joined with an underscore ("ERROR_402:")
    // is caught, not just "Error 402" with a literal space.
    let code_re = if style.forbid_numeric_codes {
        Regex::new(r"^(?i)error[^0-9a-zA-Z]*\d+").ok()
    } else {
        None
    };
    let mut prefix_res: Vec<Regex> = Vec::new();
    for prefix in &style.forbid_prefixes {
        if prefix.trim().is_empty() {
            continue;
        }
        if let Ok(re) = Regex::new(&format!(r"^(?i){}\b", regex::escape(prefix))) {
            prefix_res.push(re);
        }
    }
    let mut out = Vec::new();
    for s in surfaces {
        if s.kind != SurfaceKind::UiString {
            continue;
        }
        let text = s.text.trim_start();
        let strip = code_re
            .as_ref()
            .and_then(|re| re.find(text).map(|m| m.end()))
            .or_else(|| {
                prefix_res
                    .iter()
                    .filter_map(|re| re.find(text))
                    .map(|m| m.end())
                    .max()
            });
        let Some(strip_len) = strip else {
            continue;
        };
        out.push(Finding {
            rule_id: "error-prefix".to_string(),
            severity: Severity::Warning,
            kind: Some(s.kind),
            path: s.path.clone(),
            line: Some(s.line),
            message: "machine-style error prefix".to_string(),
            suggestion: rewrite_error_message(text, strip_len),
            semantics: FindingSemantics::default(),
        });
    }
    out
}

/// Rule `error-actionable`: error UI strings that give the user no next step.
/// Word-boundary matching avoids substring hits ("user" containing "use").
fn error_actionable(guidelines: &Guidelines, surfaces: &[Surface]) -> Vec<Finding> {
    if !guidelines.voice.style.error_messages.require_actionable {
        return Vec::new();
    }
    let error_re = Regex::new(
        r"(?i)\b(?:error|failed|invalid|expired|missing|denied|fatal|cannot|can't|unable)\b",
    )
    .ok();
    let action_re = Regex::new(
        r"(?i)\b(?:run|try|check|reconnect|see|update|install|set|use|add|fix|restart|retry|contact|verify|ensure|enable|reset|provide|grant)\b",
    )
    .ok();
    let (Some(error_re), Some(action_re)) = (error_re, action_re) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for s in surfaces {
        if s.kind != SurfaceKind::UiString {
            continue;
        }
        if error_re.is_match(&s.text) && !action_re.is_match(&s.text) {
            out.push(Finding {
                rule_id: "error-actionable".to_string(),
                severity: Severity::Info,
                kind: Some(s.kind),
                path: s.path.clone(),
                line: Some(s.line),
                message: "error message without a next step".to_string(),
                suggestion: Some(
                    "append a next step, e.g. 'Try again' or 'Contact support'".to_string(),
                ),
                semantics: FindingSemantics::default(),
            });
        }
    }
    out
}

/// Rule `exclamation-limit`, one file at a time: count `!` in the whole
/// file and emit a single warning when the budget is exceeded. Markdown
/// image syntax (`![alt](src)`) and fenced code blocks are stripped first —
/// they are not prose.
fn exclamation_limit_file(guidelines: &Guidelines, path: &Path, kind: SurfaceKind) -> Vec<Finding> {
    let max = guidelines.voice.style.max_exclamation_marks;
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(image_re) = Regex::new(r"!\[[^\]]*\]\([^)]+\)") else {
        return Vec::new();
    };
    let stripped = strip_fenced_code(&content);
    let content = image_re.replace_all(&stripped, "");
    let count = content.matches('!').count();
    if count <= max {
        return Vec::new();
    }
    vec![Finding {
        rule_id: "exclamation-limit".to_string(),
        severity: Severity::Warning,
        kind: Some(kind),
        path: path.to_path_buf(),
        line: None,
        message: format!("{count} exclamation marks (max {max})"),
        suggestion: Some(format!(
            "remove exclamation marks; the voice allows at most {max}"
        )),
        semantics: FindingSemantics::default(),
    }]
}

/// True when every alphabetic character in `word` is uppercase and there
/// are at least two of them — a cheap, dictionary-free acronym heuristic
/// (HTTPS, AWS, URL) that keeps such words out of the lowercase rewrite.
fn looks_like_acronym(word: &str) -> bool {
    let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).collect();
    letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase())
}

/// Rewrite a heading in sentence case: lowercase every word except
/// single-letter words, likely acronyms, inline-code spans, and the
/// product name/aliases (canonical casing).
fn sentence_case_heading(body: &str, name: &str, aliases: &[String]) -> String {
    body.split_whitespace()
        .enumerate()
        .map(|(i, w)| {
            if let Some(canonical) = canonical_casing(w, name, aliases) {
                return canonical;
            }
            if w.contains('`') || w.chars().count() == 1 || looks_like_acronym(w) {
                return w.to_string();
            }
            let lower = w.to_lowercase();
            if i != 0 {
                return lower;
            }
            let mut chars = lower.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => lower,
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The canonical casing for `word` when it matches the product name or an alias.
fn canonical_casing(word: &str, name: &str, aliases: &[String]) -> Option<String> {
    if !name.is_empty() && word.eq_ignore_ascii_case(name) {
        return Some(name.to_string());
    }
    aliases
        .iter()
        .find(|a| word.eq_ignore_ascii_case(a))
        .cloned()
}

/// Rule `sentence-case-headings`: flag Title Case ATX headings when the
/// voice requires sentence case. Heuristic: headings with 3+ words where
/// more than half of the words with 4+ letters start uppercase.
fn sentence_case_headings(
    brief: &Brief,
    guidelines: &Guidelines,
    surfaces: &[Surface],
) -> Vec<Finding> {
    if !guidelines.voice.style.sentence_case_headings {
        return Vec::new();
    }
    let Ok(heading_re) = Regex::new(r"^(#{1,6})\s+(.+?)\s*$") else {
        return Vec::new();
    };
    let Ok(closing_re) = Regex::new(r"\s+#+\s*$") else {
        return Vec::new();
    };
    let product = &brief.identity.product;
    let mut out = Vec::new();
    for s in surfaces {
        if !matches!(s.kind, SurfaceKind::RepoDoc | SurfaceKind::Doc) {
            continue;
        }
        let Some(caps) = heading_re.captures(s.text.trim_start()) else {
            continue;
        };
        let body = closing_re.replace(&caps[2], "");
        let words: Vec<&str> = body.split_whitespace().collect();
        if words.len() < 3 {
            continue;
        }
        let long: Vec<&&str> = words.iter().filter(|w| w.chars().count() >= 4).collect();
        if long.is_empty() {
            continue;
        }
        let upper = long
            .iter()
            .filter(|w| w.chars().next().is_some_and(|c| c.is_uppercase()))
            .count();
        if upper * 2 <= long.len() {
            continue;
        }
        let rewrite = sentence_case_heading(&body, &product.name, &product.aliases);
        out.push(Finding {
            rule_id: "sentence-case-headings".to_string(),
            severity: Severity::Warning,
            kind: Some(s.kind),
            path: s.path.clone(),
            line: Some(s.line),
            message: "Title Case heading".to_string(),
            suggestion: Some(format!("{} {rewrite}", &caps[1])),
            semantics: FindingSemantics::default(),
        });
    }
    out
}

/// The first paragraph of a markdown document: the first run of non-empty,
/// non-heading lines. Returns (1-based start line, joined text).
fn first_paragraph(content: &str) -> Option<(usize, String)> {
    let mut lines = content.lines().enumerate();
    while let Some((i, line)) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let start = i + 1;
        let mut text = trimmed.to_string();
        for (_, next) in lines.by_ref() {
            let t = next.trim();
            if t.is_empty() || t.starts_with('#') {
                break;
            }
            text.push(' ');
            text.push_str(t);
        }
        return Some((start, text));
    }
    None
}

/// Significant words (5+ letters, lowercased) from the given texts.
fn significant_words(texts: &[&str]) -> Vec<String> {
    let mut words = BTreeSet::new();
    for text in texts {
        for w in text.split(|c: char| !c.is_alphanumeric()) {
            if w.chars().count() >= 5 {
                words.insert(w.to_lowercase());
            }
        }
    }
    words.into_iter().collect()
}

/// Rule `readme-mission`: README.md must exist and its first paragraph must
/// name the product (or an alias) and echo tagline/mission/archetype words.
fn readme_mission(brief: &Brief, root: &Path) -> Vec<Finding> {
    let product = &brief.identity.product;
    let path = root.join("README.md");
    if !path.exists() {
        return vec![Finding {
            rule_id: "readme-mission".to_string(),
            severity: Severity::Error,
            kind: Some(SurfaceKind::RepoDoc),
            path,
            line: None,
            message: "README.md is missing".to_string(),
            suggestion: Some(
                "create a README.md that opens with the product name and states its mission"
                    .to_string(),
            ),
            semantics: FindingSemantics::default(),
        }];
    }
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };

    let mut names: Vec<&str> = Vec::new();
    if !product.name.trim().is_empty() {
        names.push(product.name.as_str());
    }
    names.extend(
        product
            .aliases
            .iter()
            .map(String::as_str)
            .filter(|a| !a.trim().is_empty()),
    );

    let mut sig_sources: Vec<&str> = vec![product.tagline.as_str(), product.mission.as_str()];
    sig_sources.extend(brief.identity.archetype.iter().map(String::as_str));
    let sig_re = any_word_regex(&significant_words(&sig_sources));

    let (line, para) = first_paragraph(&content)
        .map(|(l, t)| (Some(l), t))
        .unwrap_or((None, String::new()));
    let lower = para.to_lowercase();
    let name_ok = names.is_empty() || names.iter().any(|n| lower.contains(&n.to_lowercase()));
    let sig_ok = sig_re.as_ref().is_none_or(|re| re.is_match(&lower));
    if name_ok && sig_ok {
        return Vec::new();
    }

    let mut missing = Vec::new();
    if !name_ok {
        missing.push("the product name");
    }
    if !sig_ok {
        missing.push("the mission");
    }
    let lead = if product.name.trim().is_empty() {
        "The product".to_string()
    } else {
        product.name.clone()
    };
    let suggestion = if product.tagline.trim().is_empty() {
        "state the product's mission in the first paragraph".to_string()
    } else {
        format!(
            "state the mission in the first paragraph, e.g. \"{lead} — {}\"",
            product.tagline
        )
    };
    vec![Finding {
        rule_id: "readme-mission".to_string(),
        severity: Severity::Warning,
        kind: Some(SurfaceKind::RepoDoc),
        path,
        line,
        message: format!("README introduction is missing {}", missing.join(" and ")),
        suggestion: Some(suggestion),
        semantics: FindingSemantics::default(),
    }]
}

/// Rule `readme-image-refs`, one file at a time: local markdown image
/// targets (http(s):// and data: are skipped) must exist relative to the
/// markdown file's directory. Fenced code blocks are stripped first —
/// image syntax in code examples is not a real reference.
fn readme_image_refs_file(path: &Path, kind: SurfaceKind) -> Vec<Finding> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(image_re) = Regex::new(r"!\[[^\]]*\]\(([^)]+)\)") else {
        return Vec::new();
    };
    let content = strip_fenced_code(&content);
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut out = Vec::new();
    for caps in image_re.captures_iter(&content) {
        let (Some(whole), Some(raw)) = (caps.get(0), caps.get(1)) else {
            continue;
        };
        // Strip an optional <...> wrapper or trailing "title" part.
        let raw = raw.as_str().trim();
        let target = if let Some(rest) = raw.strip_prefix('<') {
            rest.split('>').next().unwrap_or(rest)
        } else {
            raw.split_whitespace().next().unwrap_or("")
        };
        if target.is_empty() {
            continue;
        }
        let lower = target.to_lowercase();
        if lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("data:")
        {
            continue;
        }
        let resolved = dir.join(target);
        if resolved.exists() {
            continue;
        }
        let line = content[..whole.start()].matches('\n').count() + 1;
        out.push(Finding {
            rule_id: "readme-image-refs".to_string(),
            severity: Severity::Warning,
            kind: Some(kind),
            path: path.to_path_buf(),
            line: Some(line),
            message: format!("broken image reference '{target}'"),
            suggestion: Some(format!(
                "fix the path or add the missing file (looked for {})",
                resolved.display()
            )),
            semantics: FindingSemantics::default(),
        });
    }
    out
}

/// Euclidean distance between two RGB colors, in the same 0-255-per-channel
/// units as `color_tolerance`. A max-per-channel (Chebyshev) distance used
/// to be used here, which under-penalizes a color drifting moderately on
/// every channel at once — a color that drifts by exactly `color_tolerance`
/// on all three channels simultaneously is visually much further off-brand
/// than the tolerance name implies, but passed under Chebyshev.
fn channel_distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let dr = f64::from(a.0) - f64::from(b.0);
    let dg = f64::from(a.1) - f64::from(b.1);
    let db = f64::from(a.2) - f64::from(b.2);
    (dr * dr + dg * dg + db * db).sqrt()
}

/// Rule `palette-adherence`: a Style surface whose `#rrggbb` literal is
/// farther than `visual.color_tolerance` from every palette color.
fn palette_adherence(guidelines: &Guidelines, surfaces: &[Surface]) -> Vec<Finding> {
    let palette = guidelines.visual.palette.all_colors();
    if palette.is_empty() {
        return Vec::new();
    }
    let tolerance = guidelines.visual.color_tolerance;
    let mut out = Vec::new();
    for s in surfaces {
        if s.kind != SurfaceKind::Style {
            continue;
        }
        let Some(rgb) = parse_hex_color(s.text.trim()) else {
            continue;
        };
        let Some((nearest, distance)) = palette
            .iter()
            .map(|c| (*c, channel_distance(rgb, *c)))
            .min_by(|(_, d1), (_, d2)| d1.total_cmp(d2))
        else {
            continue;
        };
        if distance <= f64::from(tolerance) {
            continue;
        }
        out.push(Finding {
            rule_id: "palette-adherence".to_string(),
            severity: Severity::Warning,
            kind: Some(s.kind),
            path: s.path.clone(),
            line: Some(s.line),
            message: format!("off-palette color #{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2),
            suggestion: Some(format!(
                "nearest palette color: #{:02x}{:02x}{:02x}",
                nearest.0, nearest.1, nearest.2
            )),
            semantics: FindingSemantics::default(),
        });
    }
    out
}

/// Rule `readme-h1`: the README's first non-empty line must be an H1 naming
/// the product (or an alias). A missing README is reported by
/// `readme-mission`, so this rule stays quiet then.
fn readme_h1(brief: &Brief, root: &Path) -> Vec<Finding> {
    let product = &brief.identity.product;
    let path = root.join("README.md");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let first = content
        .lines()
        .enumerate()
        .find(|(_, l)| !l.trim().is_empty());

    let mut names: Vec<&str> = Vec::new();
    if !product.name.trim().is_empty() {
        names.push(product.name.as_str());
    }
    names.extend(
        product
            .aliases
            .iter()
            .map(String::as_str)
            .filter(|a| !a.trim().is_empty()),
    );

    let ok = match first {
        Some((_, line)) => match line.trim().strip_prefix("# ") {
            Some(heading) => {
                let heading = heading.to_lowercase();
                names.is_empty() || names.iter().any(|n| heading.contains(&n.to_lowercase()))
            }
            None => false,
        },
        None => false,
    };
    if ok {
        return Vec::new();
    }
    let suggestion = if product.name.trim().is_empty() {
        "open the README with an H1 naming the product".to_string()
    } else {
        format!("make the first line '# {}'", product.name)
    };
    vec![Finding {
        rule_id: "readme-h1".to_string(),
        severity: Severity::Warning,
        kind: Some(SurfaceKind::RepoDoc),
        path,
        line: first.map(|(i, _)| i + 1),
        message: "README does not open with an H1 naming the product".to_string(),
        suggestion: Some(suggestion),
        semantics: FindingSemantics::default(),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scaffold the default brief and guidelines in a tempdir and load them
    /// back for tweaking.
    fn scaffold_brief_and_guidelines() -> (tempfile::TempDir, Brief, Guidelines) {
        let tmp = tempfile::tempdir().unwrap();
        Brief::scaffold(tmp.path()).unwrap();
        Guidelines::scaffold(tmp.path()).unwrap();
        let brief = Brief::load(tmp.path()).unwrap();
        let guidelines = Guidelines::load(tmp.path()).unwrap();
        (tmp, brief, guidelines)
    }

    fn surf(kind: SurfaceKind, path: &str, line: usize, text: &str) -> Surface {
        Surface {
            kind,
            path: PathBuf::from(path),
            line,
            text: text.to_string(),
            context: "test".into(),
            confidence: 100,
            exposure: 3,
            significance: crate::types::SurfaceSignificance::default(),
            reason: "test surface".into(),
            start_byte: 0,
            end_byte: text.len(),
        }
    }

    #[test]
    fn rule_catalog_lists_all_rules_in_stable_order() {
        let catalog = rule_catalog();
        let ids: Vec<&str> = catalog.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            ids,
            vec![
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
            ]
        );
        assert!(catalog.iter().all(|(_, desc)| !desc.is_empty()));
    }

    #[test]
    fn prohibited_term_fires_with_category_severity() {
        let (_tmp, _, mut g) = scaffold_brief_and_guidelines();
        g.prohibited
            .categories
            .insert("buzz".to_string(), vec!["frobnicate".to_string()]);
        g.prohibited
            .severity
            .insert("buzz".to_string(), "info".to_string());
        // No severity entry -> default warning.
        g.prohibited
            .categories
            .insert("misc".to_string(), vec!["zorp".to_string()]);
        let surfaces = vec![
            surf(
                SurfaceKind::RepoDoc,
                "README.md",
                3,
                "We frobnicate seamlessly and it is awesome, zorp.",
            ),
            surf(SurfaceKind::UiString, "src/cli.rs", 9, "Seamless setup"),
            surf(
                SurfaceKind::Social,
                "social://mastodon/brandi",
                3,
                "An awesome profile",
            ),
        ];
        let findings = prohibited_terms(&g, &surfaces);
        let sev = |phrase: &str| {
            findings
                .iter()
                .find(|f| f.message.contains(phrase))
                .map(|f| f.severity)
        };
        assert_eq!(sev("frobnicate"), Some(Severity::Info));
        assert_eq!(sev("seamlessly"), Some(Severity::Warning)); // corporate_jargon
        assert_eq!(sev("awesome"), Some(Severity::Error)); // empty_hype
        assert_eq!(sev("zorp"), Some(Severity::Warning)); // default
                                                          // UiString surfaces are covered too (default guidelines phrase "seamless").
        let ui = findings
            .iter()
            .find(|f| f.kind == Some(SurfaceKind::UiString));
        assert!(
            ui.is_some(),
            "expected a prohibited-term hit on the UiString surface"
        );
        assert!(findings
            .iter()
            .any(|finding| finding.kind == Some(SurfaceKind::Social)));
        let f = findings
            .iter()
            .find(|f| f.message.contains("frobnicate"))
            .unwrap();
        assert_eq!(f.rule_id, "prohibited-term");
        assert_eq!(f.message, "'frobnicate' is prohibited (buzz)");
        assert_eq!(f.kind, Some(SurfaceKind::RepoDoc));
        assert_eq!(f.line, Some(3));
        assert!(f.suggestion.is_some());
    }

    #[test]
    fn prohibited_term_respects_word_boundaries() {
        let (_tmp, _, g) = scaffold_brief_and_guidelines();
        let surfaces = vec![surf(
            SurfaceKind::Doc,
            "docs/a.md",
            1,
            "seamlessness awesomeness delves",
        )];
        assert!(prohibited_terms(&g, &surfaces).is_empty());
    }

    #[test]
    fn terminology_variant_flags_banned_and_former_names() {
        let (_tmp, mut b, _) = scaffold_brief_and_guidelines();
        b.identity
            .terminology
            .banned_variants
            .insert("BrandAI".to_string(), "Brandi".to_string());
        b.identity.product.former_names = vec!["Brandold".to_string()];
        let surfaces = vec![surf(
            SurfaceKind::Doc,
            "docs/a.md",
            2,
            "BrandAI (formerly Brandold) is great.",
        )];
        let findings = terminology_variants(&b, &surfaces);
        assert_eq!(findings.len(), 2);
        for f in &findings {
            assert_eq!(f.rule_id, "terminology-variant");
            assert_eq!(f.severity, Severity::Error);
            assert_eq!(f.suggestion.as_deref(), Some("use 'Brandi' instead"));
            assert_eq!(f.line, Some(2));
        }
        assert!(findings
            .iter()
            .any(|f| f.message.contains("banned terminology variant")));
        assert!(findings
            .iter()
            .any(|f| f.message.contains("former product name")));
    }

    #[test]
    fn terminology_case_check_flags_inconsistent_casing() {
        let (_tmp, mut b, _) = scaffold_brief_and_guidelines();
        b.identity.product.aliases = vec!["brandi-cli".to_string()];
        let surfaces = vec![surf(
            SurfaceKind::Doc,
            "docs/a.md",
            1,
            "brandi and BRANDI and Brandi and brandi-cli are here",
        )];
        let findings = terminology_variants(&b, &surfaces);
        // "brandi" and "BRANDI" flagged; exact "Brandi" and alias "brandi-cli" not.
        assert_eq!(findings.len(), 2);
        assert!(findings
            .iter()
            .any(|f| f.message == "inconsistent product naming: 'brandi'"));
        assert!(findings
            .iter()
            .any(|f| f.message == "inconsistent product naming: 'BRANDI'"));
        for f in &findings {
            assert_eq!(f.severity, Severity::Warning);
            assert_eq!(f.suggestion.as_deref(), Some("use 'Brandi'"));
        }
    }

    #[test]
    fn terminology_variant_does_not_double_count_casing_of_banned_name() {
        // Banning a shouted spelling of the product name used to trip both
        // the banned-variant check (Error) and the independent casing-drift
        // check (Warning) for the same occurrence — one real defect, two
        // penalties. Now only the banned-variant finding fires.
        let (_tmp, mut b, _) = scaffold_brief_and_guidelines();
        b.identity
            .terminology
            .banned_variants
            .insert("BRANDI".to_string(), "Brandi".to_string());
        let surfaces = vec![surf(SurfaceKind::Doc, "docs/a.md", 1, "Welcome to BRANDI.")];
        let findings = terminology_variants(&b, &surfaces);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("banned terminology variant"));
    }

    #[test]
    fn terminology_skips_inline_code_spans() {
        let (_tmp, mut b, _) = scaffold_brief_and_guidelines();
        b.identity
            .terminology
            .banned_variants
            .insert("BrandAI".to_string(), "Brandi".to_string());
        let surfaces = vec![surf(
            SurfaceKind::Doc,
            "docs/a.md",
            1,
            "Use `BrandAI` in code, but BrandAI in prose, and `brandi` stays",
        )];
        let findings = terminology_variants(&b, &surfaces);
        // Only the prose "BrandAI"; the code-spanned banned variant and the
        // code-spanned lowercase product name are both ignored.
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("banned terminology variant"));
    }

    #[test]
    fn error_prefix_flags_numeric_codes_and_prefixes() {
        let (_tmp, _, g) = scaffold_brief_and_guidelines(); // defaults: prefixes + numeric codes on
        let surfaces = vec![
            surf(
                SurfaceKind::UiString,
                "src/cli.rs",
                7,
                "Error 402: Invalid authentication token.",
            ),
            surf(
                SurfaceKind::UiString,
                "src/cli.rs",
                8,
                "Fatal: cannot connect to the daemon.",
            ),
            surf(
                SurfaceKind::UiString,
                "src/cli.rs",
                9,
                "The connection failed.",
            ),
        ];
        let findings = error_prefixes(&g, &surfaces);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].rule_id, "error-prefix");
        assert_eq!(findings[0].message, "machine-style error prefix");
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(
            findings[0].suggestion.as_deref(),
            Some("Invalid authentication token.")
        );
        assert_eq!(
            findings[1].suggestion.as_deref(),
            Some("Cannot connect to the daemon.")
        );
    }

    #[test]
    fn error_prefix_catches_glued_numeric_codes() {
        let (_tmp, _, g) = scaffold_brief_and_guidelines();
        for text in ["Error402: Invalid input.", "ERROR_402: Invalid input."] {
            let surfaces = vec![surf(SurfaceKind::UiString, "a.rs", 1, text)];
            let findings = error_prefixes(&g, &surfaces);
            assert_eq!(findings.len(), 1, "expected '{text}' to be flagged");
        }
    }

    #[test]
    fn error_prefix_respects_guidelines_gates() {
        let (_tmp, _, mut g) = scaffold_brief_and_guidelines();
        g.voice.style.error_messages.forbid_prefixes = vec![];
        g.voice.style.error_messages.forbid_numeric_codes = false;
        let surfaces = vec![surf(SurfaceKind::UiString, "a.rs", 1, "Error 1: bad.")];
        assert!(error_prefixes(&g, &surfaces).is_empty());
    }

    #[test]
    fn error_actionable_flags_dead_end_messages() {
        let (_tmp, _, g) = scaffold_brief_and_guidelines(); // require_actionable: true
        let surfaces = vec![
            surf(SurfaceKind::UiString, "a.rs", 1, "Invalid token."),
            surf(
                SurfaceKind::UiString,
                "a.rs",
                2,
                "Invalid token. Try again.",
            ),
            surf(SurfaceKind::UiString, "a.rs", 3, "Everything is fine."),
        ];
        let findings = error_actionable(&g, &surfaces);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "error-actionable");
        assert_eq!(findings[0].severity, Severity::Info);
        assert_eq!(findings[0].message, "error message without a next step");
        assert!(findings[0].suggestion.is_some());
    }

    #[test]
    fn error_actionable_respects_gate() {
        let (_tmp, _, mut g) = scaffold_brief_and_guidelines();
        g.voice.style.error_messages.require_actionable = false;
        let surfaces = vec![surf(SurfaceKind::UiString, "a.rs", 1, "Invalid token.")];
        assert!(error_actionable(&g, &surfaces).is_empty());
    }

    #[test]
    fn error_actionable_catches_fatal_and_cannot() {
        let (_tmp, _, g) = scaffold_brief_and_guidelines();
        let surfaces = vec![surf(
            SurfaceKind::UiString,
            "a.rs",
            1,
            "Fatal: cannot connect to the daemon.",
        )];
        let findings = error_actionable(&g, &surfaces);
        assert_eq!(findings.len(), 1, "fatal/cannot must trigger the check");
    }

    #[test]
    fn error_actionable_recognizes_more_action_verbs() {
        let (_tmp, _, g) = scaffold_brief_and_guidelines();
        for text in [
            "Invalid token. Verify the token and retry.",
            "Invalid token. Ensure the token is current.",
            "Invalid token. Enable two-factor auth first.",
            "Invalid token. Reset your credentials.",
        ] {
            let surfaces = vec![surf(SurfaceKind::UiString, "a.rs", 1, text)];
            assert!(
                error_actionable(&g, &surfaces).is_empty(),
                "expected '{text}' to count as actionable"
            );
        }
    }

    #[test]
    fn exclamation_limit_counts_per_file() {
        let (tmp, _, mut g) = scaffold_brief_and_guidelines();
        g.voice.style.max_exclamation_marks = 1;
        let path = tmp.path().join("docs.md");
        std::fs::write(&path, "Wow!\nThis!\nWorks!\n").unwrap();
        let findings = exclamation_limit_file(&g, &path, SurfaceKind::Doc);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "exclamation-limit");
        assert_eq!(findings[0].message, "3 exclamation marks (max 1)");
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(findings[0].line, None);
        assert_eq!(findings[0].kind, Some(SurfaceKind::Doc));
        // Within budget: silent.
        g.voice.style.max_exclamation_marks = 3;
        assert!(exclamation_limit_file(&g, &path, SurfaceKind::Doc).is_empty());
    }

    #[test]
    fn exclamation_limit_ignores_markdown_image_syntax() {
        let (tmp, _, g) = scaffold_brief_and_guidelines();
        let path = tmp.path().join("README.md");
        std::fs::write(&path, "# Title\n\n![screenshot](docs/a.png)\n").unwrap();
        // The default guidelines allow 0 exclamation marks; the image reference
        // must not count as one.
        assert!(exclamation_limit_file(&g, &path, SurfaceKind::RepoDoc).is_empty());
    }

    #[test]
    fn sentence_case_headings_flags_title_case() {
        let (_tmp, b, g) = scaffold_brief_and_guidelines(); // sentence_case_headings: true
        let surfaces = vec![
            surf(SurfaceKind::Doc, "d.md", 1, "## The Quick Start Guide"),
            surf(SurfaceKind::Doc, "d.md", 2, "## Getting started"),
            surf(SurfaceKind::Doc, "d.md", 3, "## FAQ"),
            surf(SurfaceKind::Doc, "d.md", 4, "not a heading"),
        ];
        let findings = sentence_case_headings(&b, &g, &surfaces);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "sentence-case-headings");
        assert_eq!(findings[0].message, "Title Case heading");
        assert_eq!(
            findings[0].suggestion.as_deref(),
            Some("## The quick start guide")
        );
    }

    #[test]
    fn sentence_case_headings_preserves_product_name() {
        let (_tmp, b, g) = scaffold_brief_and_guidelines();
        let surfaces = vec![surf(SurfaceKind::Doc, "d.md", 1, "## Brandi Quick Start")];
        let findings = sentence_case_headings(&b, &g, &surfaces);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].suggestion.as_deref(),
            Some("## Brandi quick start")
        );
    }

    #[test]
    fn sentence_case_headings_preserves_acronyms_and_inline_code() {
        let (_tmp, b, g) = scaffold_brief_and_guidelines();
        let surfaces = vec![
            surf(SurfaceKind::Doc, "d.md", 1, "## Support HTTPS Requests"),
            surf(
                SurfaceKind::Doc,
                "d.md",
                2,
                "## Use `README.md` File Correctly",
            ),
        ];
        let findings = sentence_case_headings(&b, &g, &surfaces);
        assert_eq!(findings.len(), 2);
        assert_eq!(
            findings[0].suggestion.as_deref(),
            Some("## Support HTTPS requests")
        );
        assert_eq!(
            findings[1].suggestion.as_deref(),
            Some("## Use `README.md` file correctly")
        );
    }

    #[test]
    fn sentence_case_headings_respects_gate() {
        let (_tmp, b, mut g) = scaffold_brief_and_guidelines();
        g.voice.style.sentence_case_headings = false;
        let surfaces = vec![surf(
            SurfaceKind::Doc,
            "d.md",
            1,
            "## The Quick Start Guide",
        )];
        assert!(sentence_case_headings(&b, &g, &surfaces).is_empty());
    }

    #[test]
    fn readme_mission_errors_when_missing() {
        let (tmp, b, _) = scaffold_brief_and_guidelines();
        let findings = readme_mission(&b, tmp.path());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "readme-mission");
        assert_eq!(findings[0].severity, Severity::Error);
        assert_eq!(findings[0].path, tmp.path().join("README.md"));
        assert_eq!(findings[0].line, None);
        assert_eq!(findings[0].kind, Some(SurfaceKind::RepoDoc));
    }

    #[test]
    fn readme_mission_checks_first_paragraph() {
        let (tmp, b, _) = scaffold_brief_and_guidelines();
        // Name + a significant tagline word ("coherence"): clean.
        std::fs::write(
            tmp.path().join("README.md"),
            "# Brandi\n\nBrandi brings brand coherence intelligence to your repo.\n",
        )
        .unwrap();
        assert!(readme_mission(&b, tmp.path()).is_empty());
        // No name, no mission vocabulary: warning.
        std::fs::write(
            tmp.path().join("README.md"),
            "# Welcome\n\nThis project does stuff for people.\n",
        )
        .unwrap();
        let findings = readme_mission(&b, tmp.path());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(findings[0].line, Some(3));
        assert!(findings[0].suggestion.is_some());
    }

    #[test]
    fn readme_image_refs_flags_missing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let docs = tmp.path().join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("logo.png"), b"png").unwrap();
        let md = docs.join("guide.md");
        std::fs::write(
            &md,
            "![ok](logo.png)\n\
             ![bad](missing.jpg)\n\
             ![web](https://example.com/x.png)\n\
             ![inline](data:image/png;base64,AAA)\n",
        )
        .unwrap();
        let findings = readme_image_refs_file(&md, SurfaceKind::Doc);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "readme-image-refs");
        assert_eq!(findings[0].message, "broken image reference 'missing.jpg'");
        assert_eq!(findings[0].line, Some(2));
        assert_eq!(findings[0].severity, Severity::Warning);
    }

    #[test]
    fn palette_adherence_flags_off_palette_colors() {
        let (_tmp, _, g) = scaffold_brief_and_guidelines(); // primary #FF2DAA, tolerance 24
        let surfaces = vec![
            surf(SurfaceKind::Style, "theme.css", 1, "#FF2DAA"), // exact
            surf(SurfaceKind::Style, "theme.css", 2, "#FF2DAB"), // distance 1 <= 24
            surf(SurfaceKind::Style, "theme.css", 3, "#FF0000"), // way off
            surf(SurfaceKind::Style, "theme.css", 4, "not a color"),
        ];
        let findings = palette_adherence(&g, &surfaces);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "palette-adherence");
        assert_eq!(findings[0].message, "off-palette color #ff0000");
        assert_eq!(
            findings[0].suggestion.as_deref(),
            Some("nearest palette color: #ff4d6d")
        );
        assert_eq!(findings[0].line, Some(3));
        assert_eq!(findings[0].severity, Severity::Warning);
    }

    #[test]
    fn channel_distance_is_euclidean_not_chebyshev() {
        // Drifting by 24 on all three channels at once: the old max-per-
        // channel (Chebyshev) distance was 24 (would pass a tolerance of
        // 24), but the true Euclidean distance is 24*sqrt(3) ≈ 41.6 — a
        // visually much bigger departure from the palette color than the
        // tolerance name implies, and now correctly measured as such.
        let a = (255u8, 77u8, 109u8);
        let b = (231u8, 53u8, 85u8);
        let d = channel_distance(a, b);
        assert!((d - 41.569).abs() < 0.01, "unexpected distance: {d}");
        assert!(d > 24.0);
    }

    #[test]
    fn readme_h1_requires_h1_with_product_name() {
        let (tmp, mut b, _) = scaffold_brief_and_guidelines();
        let readme = tmp.path().join("README.md");
        // Good H1.
        std::fs::write(&readme, "# Brandi\n\nSome intro.\n").unwrap();
        assert!(readme_h1(&b, tmp.path()).is_empty());
        // Alias in the H1 also passes.
        b.identity.product.aliases = vec!["brandi-cli".to_string()];
        std::fs::write(&readme, "# brandi-cli docs\n").unwrap();
        assert!(readme_h1(&b, tmp.path()).is_empty());
        // H1 without the product name.
        std::fs::write(&readme, "# Welcome to the project\n").unwrap();
        let findings = readme_h1(&b, tmp.path());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "readme-h1");
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(findings[0].line, Some(1));
        // First line not an H1.
        std::fs::write(&readme, "Brandi\n======\n").unwrap();
        assert_eq!(readme_h1(&b, tmp.path()).len(), 1);
        // Missing README: quiet (readme-mission reports the error instead).
        std::fs::remove_file(&readme).unwrap();
        assert!(readme_h1(&b, tmp.path()).is_empty());
    }

    #[test]
    fn findings_are_capped_at_ten_per_rule_per_file() {
        let (_tmp, _, g) = scaffold_brief_and_guidelines();
        let surfaces: Vec<Surface> = (1..=15)
            .map(|i| surf(SurfaceKind::RepoDoc, "README.md", i, "this is awesome"))
            .collect();
        let mut all = prohibited_terms(&g, &surfaces);
        assert_eq!(all.len(), 15);
        // A different file is capped independently.
        all.extend(prohibited_terms(
            &g,
            &[surf(SurfaceKind::Doc, "docs.md", 1, "awesome")],
        ));
        let capped = cap_findings(all);
        // 10 kept + 1 truncation-notice finding for README.md, plus 1
        // (uncapped, no notice) for docs.md.
        assert_eq!(capped.len(), 12);
        assert_eq!(
            capped
                .iter()
                .filter(|f| f.path == Path::new("docs.md"))
                .count(),
            1
        );
        let notice = capped
            .iter()
            .find(|f| f.path == Path::new("README.md") && f.line.is_none())
            .expect("truncation notice for README.md");
        assert!(notice.message.contains("5 more"));
        assert!(notice.message.contains("were omitted"));
        assert_eq!(notice.severity, Severity::Info);
    }

    #[test]
    fn truncation_notice_uses_singular_grammar_for_exactly_one_hidden() {
        let surfaces: Vec<Surface> = (1..=11)
            .map(|i| surf(SurfaceKind::RepoDoc, "a.md", i, "this is awesome"))
            .collect();
        let (_tmp, _, g) = scaffold_brief_and_guidelines();
        let capped = cap_findings(prohibited_terms(&g, &surfaces));
        let notice = capped
            .iter()
            .find(|f| f.line.is_none())
            .expect("truncation notice");
        assert_eq!(
            notice.message,
            "1 more 'prohibited-term' finding in this file was omitted (showing the first 10)"
        );
    }

    #[test]
    fn lint_file_on_standalone_source_file_runs_only_applicable_rules() {
        let (tmp, b, g) = scaffold_brief_and_guidelines();
        let src = tmp.path().join("tool.rs");
        std::fs::write(
            &src,
            "fn main() {\n    println!(\"Error 500: Invalid configuration.\");\n}\n",
        )
        .unwrap();
        let findings = lint_file(&b, &g, tmp.path(), &src).unwrap();
        assert!(!findings.is_empty());
        let ids: Vec<&str> = findings.iter().map(|f| f.rule_id.as_str()).collect();
        assert!(ids.contains(&"error-prefix"), "got: {ids:?}");
        // Whole-file rules (exclamation-limit, readme-image-refs, readme-*)
        // never apply to a source file.
        for id in ids {
            assert!(
                matches!(
                    id,
                    "prohibited-term"
                        | "terminology-variant"
                        | "error-prefix"
                        | "error-actionable"
                        | "sentence-case-headings"
                        | "palette-adherence"
                ),
                "unexpected rule {id}"
            );
        }
    }

    #[test]
    fn run_rules_smoke_over_fixture_project() {
        let (tmp, b, g) = scaffold_brief_and_guidelines();
        std::fs::write(
            tmp.path().join("README.md"),
            "# Brandi\n\nBrandi keeps brand coherence intelligence everywhere.\n\nThis is awesome.\n",
        )
        .unwrap();
        let findings = run_rules(&b, &g, tmp.path()).unwrap();
        assert!(
            findings
                .iter()
                .any(|f| f.rule_id == "prohibited-term" && f.message.contains("awesome")),
            "expected the planted prohibited term, got: {findings:?}"
        );
    }

    #[test]
    fn prohibited_term_skips_fenced_code_but_fires_after_it() {
        let (tmp, _, g) = scaffold_brief_and_guidelines();
        std::fs::write(
            tmp.path().join("docs.md"),
            "# Docs\n\n```yaml\nexample: awesome\n```\n\nThis is awesome.\n",
        )
        .unwrap();
        let surfaces = scan_file(tmp.path(), Path::new("docs.md")).unwrap();
        let findings = prohibited_terms(&g, &surfaces);
        // Only the prose occurrence on line 7; the fenced yaml is not prose.
        assert_eq!(findings.len(), 1, "got: {findings:?}");
        assert_eq!(
            findings[0].line,
            Some(7),
            "fence stripping preserves line numbers"
        );
        assert!(findings[0].message.contains("awesome"));
    }

    #[test]
    fn exclamation_limit_ignores_fenced_code() {
        let (tmp, _, g) = scaffold_brief_and_guidelines(); // max_exclamation_marks: 0
        let path = tmp.path().join("docs.md");
        std::fs::write(&path, "# Docs\n\n```sh\necho wow! amazing!\n```\n").unwrap();
        assert!(exclamation_limit_file(&g, &path, SurfaceKind::Doc).is_empty());
    }

    #[test]
    fn readme_image_refs_ignores_images_inside_fences() {
        let tmp = tempfile::tempdir().unwrap();
        let md = tmp.path().join("guide.md");
        std::fs::write(
            &md,
            "![real](missing.png)\n\n```markdown\n![example](also-missing.png)\n```\n",
        )
        .unwrap();
        let findings = readme_image_refs_file(&md, SurfaceKind::Doc);
        assert_eq!(findings.len(), 1, "got: {findings:?}");
        assert_eq!(findings[0].message, "broken image reference 'missing.png'");
        assert_eq!(findings[0].line, Some(1));
    }

    #[test]
    fn hex_colors_inside_fences_still_reach_palette_adherence() {
        let (tmp, b, g) = scaffold_brief_and_guidelines();
        std::fs::write(
            tmp.path().join("docs.md"),
            "# Docs\n\n```css\nbody { color: #ff0000; }\n```\n",
        )
        .unwrap();
        let findings = lint_file(&b, &g, tmp.path(), Path::new("docs.md")).unwrap();
        assert!(
            findings.iter().any(|f| f.rule_id == "palette-adherence"
                && f.line == Some(4)
                && f.message == "off-palette color #ff0000"),
            "expected a palette finding for the fenced hex, got: {findings:?}"
        );
    }

    #[test]
    fn rule_override_off_drops_findings() {
        let (tmp, b, _) = scaffold_brief_and_guidelines();
        std::fs::write(
            tmp.path().join(".brandi/rules.yaml"),
            "overrides:\n  prohibited-term: off\n",
        )
        .unwrap();
        let g = Guidelines::load(tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join("README.md"),
            "# Brandi\n\nBrandi keeps brand coherence intelligence everywhere.\n\nThis is awesome.\n",
        )
        .unwrap();
        let findings = run_rules(&b, &g, tmp.path()).unwrap();
        assert!(
            findings.iter().all(|f| f.rule_id != "prohibited-term"),
            "override 'off' should drop prohibited-term findings, got: {findings:?}"
        );
        assert!(
            findings.is_empty(),
            "no other findings expected, got: {findings:?}"
        );
    }

    #[test]
    fn rule_override_replaces_severity_and_changes_score() {
        let (tmp, b, _) = scaffold_brief_and_guidelines();
        std::fs::write(
            tmp.path().join(".brandi/rules.yaml"),
            "overrides:\n  error-actionable: error\n",
        )
        .unwrap();
        let g = Guidelines::load(tmp.path()).unwrap();
        let src = tmp.path().join("tool.rs");
        std::fs::write(&src, "fn main() {\n    println!(\"Invalid token.\");\n}\n").unwrap();
        let findings = lint_file(&b, &g, tmp.path(), &src).unwrap();
        assert_eq!(findings.len(), 1, "got: {findings:?}");
        assert_eq!(findings[0].rule_id, "error-actionable");
        // Default severity is info (score 99); the override makes it an error.
        assert_eq!(findings[0].severity, Severity::Error);
        let report = crate::report::build_report(tmp.path(), findings, &[]);
        assert_eq!(report.scores.overall, 90);
    }

    #[test]
    fn unknown_rule_override_is_ignored_at_lint_time() {
        let (tmp, b, _) = scaffold_brief_and_guidelines();
        std::fs::write(
            tmp.path().join(".brandi/rules.yaml"),
            "overrides:\n  made-up-rule: off\n",
        )
        .unwrap();
        let g = Guidelines::load(tmp.path()).unwrap();
        let src = tmp.path().join("tool.rs");
        std::fs::write(&src, "fn main() {\n    println!(\"Invalid token.\");\n}\n").unwrap();
        let findings = lint_file(&b, &g, tmp.path(), &src).unwrap();
        // Nothing is dropped or re-leveled by the unknown id.
        assert_eq!(findings.len(), 1, "got: {findings:?}");
        assert_eq!(findings[0].rule_id, "error-actionable");
        assert_eq!(findings[0].severity, Severity::Info);
    }
}
