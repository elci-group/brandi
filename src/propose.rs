//! Proposal engine: turns scanned brand surfaces and lint findings into a
//! bounded set of concrete aesthetic revisions.

use crate::brief::Brief;
use crate::guidelines::{parse_hex_color, Guidelines};
use crate::rules;
use crate::surface;
use crate::types::{
    Finding, FindingClass, SemanticRole, Severity, Surface, SurfaceDomain, SurfaceKind,
};
use form3::compat::{Colorize, StyledText};
use form3::term::ColorSupport;
use regex::{NoExpand, Regex};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// A proposed amendment to one stylisable object.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Proposal {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub kind: SurfaceKind,
    pub object: String,
    pub class: FindingClass,
    pub role: SemanticRole,
    pub reason: String,
    pub current: String,
    pub revised: String,
    pub operation: ProposalOperation,
    pub priority: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_selection: Option<ColorSelection>,
    /// Exact byte span in the source file when this proposal replaces an
    /// existing surface. Structural proposals have no span.
    pub start_byte: Option<usize>,
    pub end_byte: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProposalOperation {
    Replacement,
    Rewrite,
    StructuralEdit {
        target: String,
        constraint: String,
        current_count: Option<usize>,
        proposed_count: Option<usize>,
    },
    Advisory,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ColorSelection {
    pub source: String,
    pub target: String,
    pub palette: String,
    pub strategy: String,
    pub role: String,
    pub confidence: u8,
}

/// Telemetry captured while generating proposals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposeReport {
    pub proposals: Vec<Proposal>,
    pub surfaces_scanned: usize,
    pub files_considered: usize,
    pub files_scanned: usize,
    pub findings: usize,
    pub typography: FontSuggestion,
}

/// Deterministic typography guidance derived from the visual guidelines.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FontSuggestion {
    pub class: FindingClass,
    pub role: SemanticRole,
    pub family: String,
    pub fallback_stack: String,
    pub style: String,
    pub rationale: String,
    pub specimen: String,
    pub inline_css: String,
    pub inline_html: String,
}

/// Build at most `budget` proposals for the project rooted at `root` and
/// return the proposals plus scan/rule telemetry.
pub fn propose_report(
    brief: &Brief,
    guidelines: &Guidelines,
    root: &Path,
    budget: usize,
) -> crate::error::Result<ProposeReport> {
    let typography = font_suggestion(brief, guidelines);
    if budget == 0 {
        return Ok(ProposeReport {
            proposals: Vec::new(),
            surfaces_scanned: 0,
            files_considered: 0,
            files_scanned: 0,
            findings: 0,
            typography,
        });
    }
    let scan = surface::scan_project(root)?;
    if !scan.complete {
        return Err(crate::error::BrandiError::Invalid(format!(
            "scan incomplete: {} input diagnostics; run `brandi scan --format json` for details",
            scan.diagnostics
                .iter()
                .filter(|item| item.affects_completeness)
                .count()
        )));
    }
    let findings = rules::run_rules_on_surfaces(brief, guidelines, root, &scan.surfaces);
    let proposals = build_proposals(brief, guidelines, root, &scan.surfaces, &findings, budget);
    Ok(ProposeReport {
        surfaces_scanned: scan.surfaces.len(),
        files_considered: scan.files_considered,
        files_scanned: scan.files_scanned,
        findings: findings.len(),
        proposals,
        typography,
    })
}

/// Build at most `budget` proposals for the project rooted at `root`.
pub fn propose(
    brief: &Brief,
    guidelines: &Guidelines,
    root: &Path,
    budget: usize,
) -> crate::error::Result<Vec<Proposal>> {
    Ok(propose_report(brief, guidelines, root, budget)?.proposals)
}

/// Render proposals in a stable, human-readable form.
pub fn render_human(report: &ProposeReport, budget: usize) -> String {
    render_human_with_color(report, budget, crate::output::context().color)
}

fn render_human_with_color(report: &ProposeReport, budget: usize, color: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Brandi proposals (budget {budget}, generated {})\n\n",
        report.proposals.len()
    ));
    if report.proposals.is_empty() {
        out.push_str("No stylisation proposals. The scanned objects are already coherent enough for this budget.\n");
    } else {
        for (index, p) in report.proposals.iter().enumerate() {
            let line = p.line.map_or(String::new(), |line| format!(":{line}"));
            out.push_str(&format!(
                "{}. {}{} [{}] {}\n",
                index + 1,
                p.path.display(),
                line,
                p.kind,
                p.object
            ));
            out.push_str(&format!("   reason: {}\n", p.reason));
            out.push_str(&format!(
                "   finding: {} · {}\n   operation: {} · priority {}/100\n",
                finding_class_name(p.class),
                semantic_role_name(p.role),
                operation_name(&p.operation),
                p.priority
            ));
            out.push_str(&format!("   current: {}\n", p.current));
            out.push_str(&format!("   revised: {}\n", p.revised));
            if let Some(transition) = render_colour_transition(p, color) {
                out.push_str(&format!("   colour: {transition}\n"));
            }
            if let Some(selection) = &p.color_selection {
                out.push_str(&format!(
                    "   selection: {} · {} · confidence {}%\n",
                    selection.palette, selection.strategy, selection.confidence
                ));
            }
        }
    }

    let font = &report.typography;
    let support = if color {
        ColorSupport::Truecolor
    } else {
        ColorSupport::NoColor
    };
    let family = StyledText::new(&font.family)
        .with_color_support(support)
        .bold();
    let specimen = StyledText::new(&font.specimen)
        .with_color_support(support)
        .italic();
    out.push_str("\nTypography opportunity\n");
    out.push_str(&format!("   font: {family} · {}\n", font.style));
    out.push_str(&format!("   reason: {}\n", font.rationale));
    out.push_str(&format!("   specimen: {specimen}\n"));
    out.push_str(&format!("   inline CSS: {}\n", font.inline_css));
    out.push_str(&format!("   inline HTML: {}\n", font.inline_html));
    out
}

fn operation_name(operation: &ProposalOperation) -> &'static str {
    match operation {
        ProposalOperation::Replacement => "replacement",
        ProposalOperation::Rewrite => "rewrite",
        ProposalOperation::StructuralEdit { .. } => "structural edit",
        ProposalOperation::Advisory => "advisory",
    }
}

fn finding_class_name(class: FindingClass) -> &'static str {
    match class {
        FindingClass::Violation => "violation",
        FindingClass::Opportunity => "opportunity",
        FindingClass::Observation => "observation",
    }
}

fn semantic_role_name(role: SemanticRole) -> &'static str {
    match role {
        SemanticRole::Prose => "prose",
        SemanticRole::Heading => "heading",
        SemanticRole::UiCopy => "ui copy",
        SemanticRole::ColorToken => "colour token",
        SemanticRole::SocialProfile => "social profile",
        SemanticRole::DocumentStructure => "document structure",
        SemanticRole::Typography => "typography",
    }
}

fn render_colour_transition(proposal: &Proposal, color: bool) -> Option<String> {
    let current = parse_hex_color(proposal.current.trim())?;
    let revised = parse_hex_color(proposal.revised.trim())?;
    if !color {
        return Some(format!("{} → {}", proposal.current, proposal.revised));
    }
    Some(format!(
        "{} {} → {} {}",
        colour_swatch(current),
        proposal.current,
        colour_swatch(revised),
        proposal.revised
    ))
}

fn colour_swatch((r, g, b): (u8, u8, u8)) -> String {
    StyledText::new("    ")
        .with_color_support(ColorSupport::Truecolor)
        .on_rgb(r, g, b)
        .to_string()
}

fn font_suggestion(brief: &Brief, guidelines: &Guidelines) -> FontSuggestion {
    let style = guidelines.visual.typography.style.trim().to_string();
    let style_key = style.to_ascii_lowercase();
    let preferred: Vec<String> = guidelines
        .visual
        .typography
        .preferred_fonts
        .iter()
        .map(|font| safe_font_family(font))
        .filter(|font| !font.is_empty())
        .collect();
    let family = preferred.first().cloned().unwrap_or_else(|| {
        if style_key.contains("editorial") || style_key.contains("serif") {
            "Source Serif 4".into()
        } else if style_key.contains("geometric") {
            "Space Grotesk".into()
        } else if style_key.contains("friendly") || style_key.contains("playful") {
            "Nunito Sans".into()
        } else if style_key.contains("accessible") {
            "Atkinson Hyperlegible".into()
        } else {
            "Inter".into()
        }
    });
    let generic = if style_key.contains("editorial") || style_key.contains("serif") {
        "ui-serif, Georgia, serif"
    } else {
        "ui-sans-serif, system-ui, sans-serif"
    };
    let mut fallbacks = preferred.iter().skip(1).cloned().collect::<Vec<_>>();
    fallbacks.push(generic.into());
    let fallback_stack = fallbacks.join(", ");
    let css_family = format!("\"{family}\", {fallback_stack}");
    let specimen = format!(
        "{} makes every surface feel intentional — Aa Bb Cc 0123456789",
        brief.identity.product.name.trim()
    );
    let inline_css = format!("font-family: {css_family};");
    let inline_html = format!(
        "<span style=\"{}\">{}</span>",
        escape_html(&inline_css),
        escape_html(&specimen)
    );
    FontSuggestion {
        class: FindingClass::Opportunity,
        role: SemanticRole::Typography,
        family,
        fallback_stack,
        style: if style.is_empty() {
            "unspecified".into()
        } else {
            style
        },
        rationale: if preferred.is_empty() {
            "recommended from the configured typography style".into()
        } else {
            "uses the first configured preferred font with explicit fallbacks".into()
        },
        specimen,
        inline_css,
        inline_html,
    }
}

fn safe_font_family(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric() || matches!(character, ' ' | '-' | '_'))
        .collect::<String>()
        .trim()
        .to_string()
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn build_proposals(
    brief: &Brief,
    guidelines: &Guidelines,
    root: &Path,
    surfaces: &[Surface],
    findings: &[Finding],
    budget: usize,
) -> Vec<Proposal> {
    let mut proposals = Vec::new();
    let mut seen = HashSet::new();

    for finding in ranked_findings(findings) {
        if !proposal_eligible(finding) {
            continue;
        }
        let Some(kind) = finding.kind else {
            continue;
        };
        if finding.semantics.role == SemanticRole::DocumentStructure {
            if let Some(proposal) = virtual_proposal(root, finding, kind) {
                push_unique(&mut proposals, &mut seen, proposal);
            }
            if proposals.len() >= budget {
                return proposals;
            }
            continue;
        }
        let Some(line) = finding.line else {
            if let Some(proposal) = virtual_proposal(root, finding, kind) {
                push_unique(&mut proposals, &mut seen, proposal);
            }
            if proposals.len() >= budget {
                return proposals;
            }
            continue;
        };
        let Some(surface) = surfaces.iter().find(|surface| {
            surface.path == finding.path
                && surface.line == line
                && surface.kind == kind
                && (finding.rule_id != "palette-adherence"
                    || finding
                        .message
                        .to_ascii_lowercase()
                        .contains(&surface.text.trim().to_ascii_lowercase()))
        }) else {
            continue;
        };
        if let Some(proposal) = proposal_for_finding(surface, finding) {
            push_unique(&mut proposals, &mut seen, proposal);
        }
        if proposals.len() >= budget {
            return proposals;
        }
    }

    for surface in surfaces {
        if proposals.len() >= budget {
            break;
        }
        if surface.significance.domain == SurfaceDomain::Operational
            && surface.significance.relevance < 25
        {
            continue;
        }
        if let Some(proposal) = polish_candidate(brief, guidelines, surface) {
            push_unique(&mut proposals, &mut seen, proposal);
        }
    }
    proposals.truncate(budget);
    proposals
}

fn ranked_findings(findings: &[Finding]) -> Vec<&Finding> {
    let mut ranked: Vec<&Finding> = findings.iter().collect();
    ranked.sort_by_key(|f| {
        (
            std::cmp::Reverse(f.semantics.priority),
            severity_rank(f.severity),
            f.path.clone(),
            f.line.unwrap_or(usize::MAX),
            f.rule_id.clone(),
        )
    });
    ranked
}

fn proposal_eligible(finding: &Finding) -> bool {
    finding.semantics.class == FindingClass::Violation
        && !(finding.semantics.domain == SurfaceDomain::Operational
            && finding.semantics.relevance < 25)
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
}

fn push_unique(
    proposals: &mut Vec<Proposal>,
    seen: &mut HashSet<(PathBuf, Option<usize>, String, String)>,
    p: Proposal,
) {
    // The key includes the revised text, not just (path, line, object):
    // two different findings on the same line (e.g. `prohibited-term` and
    // `terminology-variant` both firing on one string) share the same
    // location and object label but produce different revisions, and both
    // deserve to surface rather than have the second one silently dropped
    // as a "duplicate".
    let key = (p.path.clone(), p.line, p.object.clone(), p.revised.clone());
    if p.current == p.revised || !seen.insert(key) {
        return;
    }
    proposals.push(p);
}

fn proposal_for_finding(surface: &Surface, finding: &Finding) -> Option<Proposal> {
    let revised = match finding.rule_id.as_str() {
        "prohibited-term" => revise_prohibited(&surface.text, finding),
        "terminology-variant" => revise_terminology(&surface.text, finding),
        "error-prefix" => finding.suggestion.clone(),
        "error-actionable" => Some(actionable_error(&surface.text)),
        "sentence-case-headings" => finding.suggestion.clone(),
        "palette-adherence" => revise_palette(&surface.text, finding),
        _ => finding.suggestion.clone(),
    }?;

    let color_selection = (finding.rule_id == "palette-adherence")
        .then(|| color_selection(&surface.text, &revised))
        .flatten();
    let external = surface.kind == SurfaceKind::Social;
    Some(Proposal {
        path: surface.path.clone(),
        line: Some(surface.line),
        kind: surface.kind,
        object: object_name(surface.kind, &surface.text),
        class: finding.semantics.class,
        role: finding.semantics.role,
        reason: finding.message.clone(),
        current: compact(&surface.text),
        revised: voice_finish(&revised, surface.kind, finding.semantics.role),
        operation: if external {
            ProposalOperation::Advisory
        } else {
            ProposalOperation::Replacement
        },
        priority: finding.semantics.priority,
        color_selection,
        start_byte: (!external).then_some(surface.start_byte),
        end_byte: (!external).then_some(surface.end_byte),
    })
}

fn virtual_proposal(root: &Path, finding: &Finding, kind: SurfaceKind) -> Option<Proposal> {
    let revised = finding.suggestion.clone()?;
    let (current, revised, operation) = structural_edit(finding, revised);
    Some(Proposal {
        path: finding.path.clone(),
        line: None,
        kind,
        object: if finding.path == root.join("README.md") {
            "README structure".to_string()
        } else {
            "document structure".to_string()
        },
        class: finding.semantics.class,
        role: finding.semantics.role,
        reason: finding.message.clone(),
        current,
        revised,
        operation,
        priority: finding.semantics.priority,
        color_selection: None,
        start_byte: None,
        end_byte: None,
    })
}

fn structural_edit(finding: &Finding, revised: String) -> (String, String, ProposalOperation) {
    if finding.rule_id == "exclamation-limit" {
        let counts = Regex::new(r"^(\d+) exclamation marks \(max (\d+)\)$")
            .ok()
            .and_then(|regex| regex.captures(&finding.message))
            .and_then(|captures| {
                Some((
                    captures[1].parse::<usize>().ok()?,
                    captures[2].parse::<usize>().ok()?,
                ))
            });
        if let Some((current, maximum)) = counts {
            return (
                format!("{current} exclamation marks"),
                format!("{maximum} exclamation marks"),
                ProposalOperation::StructuralEdit {
                    target: "punctuation".into(),
                    constraint: format!("max_exclamation_marks={maximum}"),
                    current_count: Some(current),
                    proposed_count: Some(maximum),
                },
            );
        }
    }
    (
        "structural state".into(),
        revised,
        ProposalOperation::StructuralEdit {
            target: "document".into(),
            constraint: finding.rule_id.clone(),
            current_count: None,
            proposed_count: None,
        },
    )
}

fn polish_candidate(brief: &Brief, guidelines: &Guidelines, surface: &Surface) -> Option<Proposal> {
    match surface.kind {
        SurfaceKind::RepoDoc | SurfaceKind::Doc => polish_doc_line(brief, surface),
        SurfaceKind::UiString => polish_ui_string(brief, surface),
        SurfaceKind::Style => polish_style(guidelines, surface),
        SurfaceKind::Social => None,
    }
}

fn polish_doc_line(brief: &Brief, surface: &Surface) -> Option<Proposal> {
    let text = surface.text.trim();
    if text.len() < 24 || text.starts_with('|') || text.starts_with("```") {
        return None;
    }
    let revised = strengthen_brand_voice(brief, text);
    if revised == text {
        return None;
    }
    Some(Proposal {
        path: surface.path.clone(),
        line: Some(surface.line),
        kind: surface.kind,
        object: object_name(surface.kind, text),
        class: FindingClass::Opportunity,
        role: if text.starts_with('#') {
            SemanticRole::Heading
        } else {
            SemanticRole::Prose
        },
        reason: "eligible prose can carry the brand voice more clearly".to_string(),
        current: compact(text),
        revised,
        operation: ProposalOperation::Rewrite,
        priority: 0,
        color_selection: None,
        start_byte: Some(surface.start_byte),
        end_byte: Some(surface.end_byte),
    })
}

fn polish_ui_string(brief: &Brief, surface: &Surface) -> Option<Proposal> {
    let text = surface.text.trim();
    if text.len() < 12 {
        return None;
    }
    let revised = if looks_like_error(text) {
        actionable_error(text)
    } else {
        strengthen_brand_voice(brief, text)
    };
    if revised == text {
        return None;
    }
    Some(Proposal {
        path: surface.path.clone(),
        line: Some(surface.line),
        kind: surface.kind,
        object: "UI copy".to_string(),
        class: FindingClass::Opportunity,
        role: SemanticRole::UiCopy,
        reason: "user-facing string can be more specific and actionable".to_string(),
        current: compact(text),
        revised,
        operation: ProposalOperation::Rewrite,
        priority: 0,
        color_selection: None,
        start_byte: Some(surface.start_byte),
        end_byte: Some(surface.end_byte),
    })
}

fn polish_style(guidelines: &Guidelines, surface: &Surface) -> Option<Proposal> {
    let revised = nearest_palette_hex(guidelines, surface.text.trim())?;
    if revised.eq_ignore_ascii_case(surface.text.trim()) {
        return None;
    }
    let selection = color_selection(&surface.text, &revised);
    Some(Proposal {
        path: surface.path.clone(),
        line: Some(surface.line),
        kind: SurfaceKind::Style,
        object: "color token".to_string(),
        class: FindingClass::Opportunity,
        role: SemanticRole::ColorToken,
        reason: "color should sit inside the brand palette".to_string(),
        current: surface.text.trim().to_string(),
        revised,
        operation: ProposalOperation::Replacement,
        priority: 0,
        color_selection: selection,
        start_byte: Some(surface.start_byte),
        end_byte: Some(surface.end_byte),
    })
}

fn revise_prohibited(text: &str, finding: &Finding) -> Option<String> {
    let phrase = quoted(&finding.message)?;
    let replacement = prohibited_replacement(&phrase, &finding.message);
    let mut revised = replace_case_insensitive(text, &phrase, replacement);
    revised = fix_article_before(&revised, replacement);
    revised = revised
        .replace("  ", " ")
        .replace(" ,", ",")
        .replace(" .", ".")
        .trim_matches(|c: char| c == ' ' || c == ',' || c == '.')
        .to_string();
    if revised.is_empty() {
        Some("State the concrete user benefit.".to_string())
    } else {
        Some(sentence_finish(&revised))
    }
}

/// Whether `word` (its first token, for a multi-word replacement) opens
/// with a vowel *sound*: spelled-with-a-vowel exceptions like "useful" or
/// "unique" are pronounced with a leading consonant ("yoo-") and still
/// take "a", not "an".
fn starts_with_vowel_sound(word: &str) -> bool {
    let first_word = word
        .split_whitespace()
        .next()
        .unwrap_or(word)
        .to_lowercase();
    let Some(first) = first_word.chars().next() else {
        return false;
    };
    if !matches!(first, 'a' | 'e' | 'i' | 'o' | 'u') {
        return false;
    }
    const CONSONANT_SOUND_EXCEPTIONS: [&str; 6] =
        ["use", "useful", "unique", "unit", "united", "european"];
    !CONSONANT_SOUND_EXCEPTIONS.contains(&first_word.as_str())
}

/// Fix "a"/"an" immediately before `word` to match its leading sound: word
/// substitution swaps in a fixed replacement with no regard for the article
/// that preceded the original phrase, so "an awesome idea" -> "an useful
/// idea" without this.
fn fix_article_before(text: &str, word: &str) -> String {
    if word.is_empty() {
        return text.to_string();
    }
    let correct = if starts_with_vowel_sound(word) {
        "an"
    } else {
        "a"
    };
    let Ok(re) = Regex::new(&format!(r"(?i)\b(a|an)\s+{}\b", regex::escape(word))) else {
        return text.to_string();
    };
    re.replace_all(text, |caps: &regex::Captures| {
        let article = &caps[1];
        let fixed = if article.chars().next().is_some_and(char::is_uppercase) {
            capitalize_first_alpha(correct)
        } else {
            correct.to_string()
        };
        format!("{fixed} {word}")
    })
    .to_string()
}

fn prohibited_replacement(phrase: &str, message: &str) -> &'static str {
    if message.contains("(empty_hype)") {
        let words: Vec<&str> = phrase.split_whitespace().collect();
        if words == ["game", "changer"] {
            return "meaningful";
        }
        if words == ["the", "future", "of"] {
            return "a practical improvement for";
        }
        return match phrase.to_lowercase().as_str() {
            "awesome" | "amazing" | "incredible" => "useful",
            "revolutionary" | "game-changing" => "meaningful",
            _ => "specific",
        };
    }
    if message.contains("(corporate_jargon)") {
        return match phrase.to_lowercase().as_str() {
            "seamless" | "seamlessly" => "smooth",
            "best-in-class" | "world-class" => "reliable",
            "cutting-edge" => "modern",
            "synergy" => "coordination",
            "leverage our" => "use our",
            _ => "",
        };
    }
    ""
}

fn revise_terminology(text: &str, finding: &Finding) -> Option<String> {
    let found = quoted(&finding.message)?;
    let replacement = quoted(finding.suggestion.as_ref()?)?;
    Some(replace_case_insensitive(text, &found, &replacement))
}

fn revise_palette(text: &str, finding: &Finding) -> Option<String> {
    let replacement = finding
        .suggestion
        .as_ref()?
        .strip_prefix("nearest palette color: ")?;
    Some(replace_case_insensitive(text, text.trim(), replacement))
}

fn quoted(text: &str) -> Option<String> {
    let start = text.find('\'')? + 1;
    let end = text[start..].find('\'')? + start;
    Some(text[start..end].to_string())
}

fn replace_case_insensitive(text: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return text.to_string();
    }
    let Ok(re) = Regex::new(&format!("(?i){}", regex::escape(needle))) else {
        return text.to_string();
    };
    re.replace_all(text, NoExpand(replacement)).to_string()
}

fn actionable_error(text: &str) -> String {
    let trimmed = text.trim_end_matches('.');
    if has_action_word(trimmed) {
        sentence_finish(trimmed)
    } else {
        format!("{}. Check the details and try again.", trimmed)
    }
}

fn has_action_word(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "run", "try", "check", "update", "install", "set", "use", "add", "restart", "retry",
        "contact",
    ]
    .iter()
    .any(|word| {
        lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|part| part == *word)
    })
}

fn looks_like_error(text: &str) -> bool {
    let lower = text.to_lowercase();
    ["error", "failed", "invalid", "expired", "missing", "denied"]
        .iter()
        .any(|word| {
            lower
                .split(|c: char| !c.is_alphanumeric())
                .any(|part| part == *word)
        })
}

fn strengthen_brand_voice(brief: &Brief, text: &str) -> String {
    let mut revised = text.trim().to_string();
    let product = brief.identity.product.name.trim();
    if !product.is_empty() && !revised.to_lowercase().contains(&product.to_lowercase()) {
        if revised.starts_with('#') {
            return revised;
        }
        revised = format!("{product} {revised}");
    }
    sentence_finish(&revised)
}

fn voice_finish(text: &str, kind: SurfaceKind, role: SemanticRole) -> String {
    let text = text.trim();
    if kind == SurfaceKind::Style || role == SemanticRole::Heading {
        return text.to_string();
    }
    if kind == SurfaceKind::UiString && looks_like_error(text) {
        return actionable_error(text);
    }
    sentence_finish(text)
}

fn color_selection(source: &str, target: &str) -> Option<ColorSelection> {
    let source_rgb = parse_hex_color(source.trim())?;
    let target_rgb = parse_hex_color(target.trim())?;
    let distance = source_rgb
        .0
        .abs_diff(target_rgb.0)
        .max(source_rgb.1.abs_diff(target_rgb.1))
        .max(source_rgb.2.abs_diff(target_rgb.2));
    Some(ColorSelection {
        source: source.trim().to_ascii_lowercase(),
        target: target.trim().to_ascii_lowercase(),
        palette: "configured".into(),
        strategy: "nearest_palette_colour".into(),
        role: "palette_alignment".into(),
        confidence: 100u8
            .saturating_sub(u8::try_from(u16::from(distance) * 100 / 255).unwrap_or(100)),
    })
}

fn sentence_finish(text: &str) -> String {
    let trimmed = capitalize_first_alpha(text.trim());
    if trimmed.is_empty()
        || trimmed.ends_with('.')
        || trimmed.ends_with('?')
        || trimmed.ends_with('!')
    {
        trimmed
    } else {
        format!("{trimmed}.")
    }
}

/// Uppercase the first alphabetic character (skipping any leading markdown
/// heading markers, punctuation, etc.), leaving the rest untouched.
/// Word-substitution revisions produce a fixed-case replacement regardless
/// of the matched word's original case or sentence position, so a proposal
/// like "awesome idea." -> "useful idea." would otherwise lose its capital.
fn capitalize_first_alpha(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut done = false;
    for c in text.chars() {
        if !done && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            done = true;
        } else {
            out.push(c);
        }
    }
    out
}

fn nearest_palette_hex(guidelines: &Guidelines, raw: &str) -> Option<String> {
    let rgb = parse_hex_color(raw)?;
    let nearest = guidelines
        .visual
        .palette
        .all_colors()
        .into_iter()
        .min_by_key(|c| {
            rgb.0
                .abs_diff(c.0)
                .max(rgb.1.abs_diff(c.1))
                .max(rgb.2.abs_diff(c.2))
        })?;
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        nearest.0, nearest.1, nearest.2
    ))
}

fn object_name(kind: SurfaceKind, text: &str) -> String {
    match kind {
        SurfaceKind::RepoDoc | SurfaceKind::Doc if text.trim_start().starts_with('#') => {
            "heading".to_string()
        }
        SurfaceKind::RepoDoc => "repository prose".to_string(),
        SurfaceKind::Doc => "documentation prose".to_string(),
        SurfaceKind::UiString => "UI copy".to_string(),
        SurfaceKind::Style => "color token".to_string(),
        SurfaceKind::Social => "social profile".to_string(),
    }
}

fn compact(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > 140 {
        let mut out = text.chars().take(137).collect::<String>();
        out.push_str("...");
        out
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brief::Brief;
    use crate::guidelines::Guidelines;

    fn fixture() -> (tempfile::TempDir, Brief, Guidelines) {
        let tmp = tempfile::tempdir().unwrap();
        Brief::scaffold(tmp.path()).unwrap();
        Guidelines::scaffold(tmp.path()).unwrap();
        let brief = Brief::load(tmp.path()).unwrap();
        let guidelines = Guidelines::load(tmp.path()).unwrap();
        (tmp, brief, guidelines)
    }

    #[test]
    fn propose_uses_budget_and_highest_signal_findings() {
        let (tmp, brief, guidelines) = fixture();
        std::fs::write(
            tmp.path().join("README.md"),
            "# Welcome\n\nThis awesome project helps teams.\n",
        )
        .unwrap();

        let proposals = propose(&brief, &guidelines, tmp.path(), 6).unwrap();
        let proposal = proposals
            .iter()
            .find(|proposal| proposal.reason.contains("prohibited"))
            .expect("prohibited public prose should remain within the ranked budget");
        assert_eq!(proposal.object, "repository prose");
        assert_eq!(proposal.revised, "This useful project helps teams.");
    }

    #[test]
    fn propose_revises_ui_errors_and_palette_tokens() {
        let (tmp, brief, guidelines) = fixture();
        std::fs::write(
            tmp.path().join("main.rs"),
            "fn main() { println!(\"Error 500: Invalid token.\"); }\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("style.css"),
            ".x { color: #ff0000; background: #1f2937; }\n",
        )
        .unwrap();

        let proposals = propose(&brief, &guidelines, tmp.path(), 4).unwrap();
        assert!(
            proposals
                .iter()
                .any(|p| p.current == "Error 500: Invalid token."
                    && p.revised.contains("Check the details")),
            "got: {proposals:?}"
        );
        assert!(
            proposals
                .iter()
                .any(|p| p.current == "#ff0000" && p.revised == "#ff4d6d"),
            "got: {proposals:?}"
        );
        assert!(proposals
            .iter()
            .filter(|proposal| proposal.kind == SurfaceKind::Style)
            .all(|proposal| proposal
                .reason
                .to_ascii_lowercase()
                .contains(&proposal.current.to_ascii_lowercase())));
    }

    #[test]
    fn revise_prohibited_fixes_article_and_capitalization() {
        let (tmp, brief, guidelines) = fixture();
        std::fs::write(
            tmp.path().join("README.md"),
            "# Welcome\n\nAn awesome idea. This helps teams.\n",
        )
        .unwrap();
        let proposals = propose(&brief, &guidelines, tmp.path(), 4).unwrap();
        let p = proposals
            .iter()
            .find(|p| p.reason.contains("prohibited"))
            .expect("expected a proposal for the awesome sentence");
        // "An awesome" -> "A useful" (article agreement: "useful" is
        // spelled with a leading vowel but pronounced with a leading
        // consonant sound) and the sentence keeps its leading capital.
        assert_eq!(p.revised, "A useful idea. This helps teams.");
    }

    #[test]
    fn fix_article_before_handles_consonant_sound_exceptions() {
        assert_eq!(
            fix_article_before("an useful idea", "useful"),
            "a useful idea"
        );
        assert_eq!(
            fix_article_before("An useful idea", "useful"),
            "A useful idea"
        );
        assert_eq!(
            fix_article_before("a meaningful idea", "meaningful"),
            "a meaningful idea"
        );
        assert_eq!(
            fix_article_before("an modern idea", "modern"),
            "a modern idea"
        );
    }

    #[test]
    fn capitalize_first_alpha_skips_leading_markup() {
        assert_eq!(capitalize_first_alpha("useful idea."), "Useful idea.");
        assert_eq!(
            capitalize_first_alpha("## the quick start guide"),
            "## The quick start guide"
        );
        assert_eq!(capitalize_first_alpha(""), "");
    }

    #[test]
    fn push_unique_keeps_distinct_revisions_on_the_same_line() {
        let mut proposals = Vec::new();
        let mut seen = HashSet::new();
        let base = Proposal {
            path: PathBuf::from("a.md"),
            line: Some(1),
            kind: SurfaceKind::RepoDoc,
            object: "repository prose".to_string(),
            class: FindingClass::Violation,
            role: SemanticRole::Prose,
            reason: "prohibited-term".to_string(),
            current: "Awesome BRANDI".to_string(),
            revised: "Useful BRANDI".to_string(),
            operation: ProposalOperation::Replacement,
            priority: 1,
            color_selection: None,
            start_byte: Some(0),
            end_byte: Some(14),
        };
        let other = Proposal {
            reason: "terminology-variant".to_string(),
            revised: "Awesome Brandi".to_string(),
            ..base.clone()
        };
        push_unique(&mut proposals, &mut seen, base.clone());
        push_unique(&mut proposals, &mut seen, other.clone());
        assert_eq!(proposals.len(), 2, "distinct revisions must both survive");
        // A true duplicate (identical revised text) is still deduplicated.
        push_unique(&mut proposals, &mut seen, base);
        assert_eq!(proposals.len(), 2);
    }

    #[test]
    fn render_includes_locations_and_budget() {
        let proposal = Proposal {
            path: PathBuf::from("README.md"),
            line: Some(3),
            kind: SurfaceKind::RepoDoc,
            object: "repository prose".to_string(),
            class: FindingClass::Opportunity,
            role: SemanticRole::Prose,
            reason: "too generic".to_string(),
            current: "Nice tool".to_string(),
            revised: "Brandi keeps brand surfaces coherent.".to_string(),
            operation: ProposalOperation::Rewrite,
            priority: 1,
            color_selection: None,
            start_byte: Some(0),
            end_byte: Some(9),
        };
        let (_, brief, guidelines) = fixture();
        let report = ProposeReport {
            proposals: vec![proposal],
            surfaces_scanned: 1,
            files_considered: 1,
            files_scanned: 1,
            findings: 1,
            typography: font_suggestion(&brief, &guidelines),
        };
        let rendered = render_human_with_color(&report, 8, false);
        assert!(rendered.contains("budget 8"));
        assert!(rendered.contains("README.md:3 [repo_doc] repository prose"));
        assert!(rendered.contains("revised: Brandi keeps brand surfaces coherent."));
        assert!(rendered.contains("Typography opportunity"));
        assert!(rendered.contains("inline CSS: font-family:"));
    }

    #[test]
    fn headings_do_not_inherit_sentence_punctuation() {
        assert_eq!(
            voice_finish(
                "### Phase 5 — barbara integration",
                SurfaceKind::Doc,
                SemanticRole::Heading,
            ),
            "### Phase 5 — barbara integration"
        );
    }

    #[test]
    fn structural_exclamation_proposal_has_counts_not_an_instruction() {
        let finding = Finding {
            rule_id: "exclamation-limit".into(),
            severity: Severity::Warning,
            kind: Some(SurfaceKind::Doc),
            path: PathBuf::from("guide.md"),
            line: None,
            message: "6 exclamation marks (max 0)".into(),
            suggestion: Some("remove exclamation marks".into()),
            semantics: crate::types::FindingSemantics::default(),
        };
        let (current, revised, operation) = structural_edit(&finding, "ignored".into());
        assert_eq!(current, "6 exclamation marks");
        assert_eq!(revised, "0 exclamation marks");
        assert!(matches!(
            operation,
            ProposalOperation::StructuralEdit {
                current_count: Some(6),
                proposed_count: Some(0),
                ..
            }
        ));
    }

    #[test]
    fn colour_selection_is_auditable() {
        let selection = color_selection("#ff0000", "#ff4d6d").unwrap();
        assert_eq!(selection.palette, "configured");
        assert_eq!(selection.strategy, "nearest_palette_colour");
        assert_eq!(selection.source, "#ff0000");
        assert_eq!(selection.target, "#ff4d6d");
    }

    #[test]
    fn colour_suggestions_render_exact_truecolour_swatches_and_plain_hex_fallbacks() {
        let (tmp, brief, guidelines) = fixture();
        std::fs::write(tmp.path().join("style.css"), ".x { color: #ff0000; }\n").unwrap();
        let report = propose_report(&brief, &guidelines, tmp.path(), 4).unwrap();

        let rich = render_human_with_color(&report, 4, true);
        assert!(rich.contains("\u{1b}[48;2;255;0;0m"));
        assert!(rich.contains("\u{1b}[48;2;255;77;109m"));
        assert!(rich.contains("#ff0000"));
        assert!(rich.contains("#ff4d6d"));

        let plain = render_human_with_color(&report, 4, false);
        assert!(!plain.contains('\u{1b}'));
        assert!(plain.contains("colour: #ff0000 → #ff4d6d"));
    }

    #[test]
    fn typography_suggestion_uses_preferred_font_in_inline_specimens() {
        let (_, brief, mut guidelines) = fixture();
        guidelines.visual.typography.preferred_fonts = vec!["IBM Plex Sans".into(), "Arial".into()];
        let suggestion = font_suggestion(&brief, &guidelines);
        assert_eq!(suggestion.family, "IBM Plex Sans");
        assert_eq!(
            suggestion.fallback_stack,
            "Arial, ui-sans-serif, system-ui, sans-serif"
        );
        assert!(suggestion.inline_css.contains("\"IBM Plex Sans\""));
        assert!(suggestion
            .inline_html
            .contains("style=\"font-family: &quot;IBM Plex Sans&quot;"));
    }
}
