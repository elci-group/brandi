//! Report assembly and rendering for lint results.

use crate::error::Result;
use crate::output::{self, Severity as OutputSeverity};
use crate::types::{
    BaselineDelta, Finding, LintReport, ScanResult, Scores, Severity, Surface, SurfaceKind,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const REPORT_SCHEMA_VERSION: &str = "brandi-report-v5";
pub const SCORING_MODEL_VERSION: &str = "scoring-v3-significance";

fn severity_weight(severity: Severity) -> u64 {
    match severity {
        Severity::Error => 10,
        Severity::Warning => 4,
        Severity::Info => 1,
    }
}

/// Keys of a score/count map in display order: the `SurfaceKind` declaration
/// order first, then any extra buckets (e.g. "general") alphabetically
/// (they arrive sorted from the `BTreeMap`).
fn ordered_keys<V>(map: &BTreeMap<String, V>) -> Vec<&str> {
    const KINDS: [SurfaceKind; 5] = [
        SurfaceKind::RepoDoc,
        SurfaceKind::Doc,
        SurfaceKind::UiString,
        SurfaceKind::Style,
        SurfaceKind::Social,
    ];
    let mut ordered: Vec<&str> = Vec::with_capacity(map.len());
    for kind in KINDS {
        if let Some((key, _)) = map.get_key_value(&kind.to_string()) {
            ordered.push(key.as_str());
        }
    }
    for key in map.keys() {
        if !ordered.contains(&key.as_str()) {
            ordered.push(key.as_str());
        }
    }
    ordered
}

/// Build a `LintReport` (scores, surface counts, timestamp) from findings and
/// the surfaces that were scanned.
pub fn build_report(root: &Path, findings: Vec<Finding>, surfaces: &[Surface]) -> LintReport {
    let scan = ScanResult {
        schema_version: crate::surface::EXTRACTION_SCHEMA_VERSION.into(),
        complete: true,
        surfaces: surfaces.to_vec(),
        diagnostics: Vec::new(),
        files_considered: 0,
        files_scanned: 0,
    };
    build_report_from_scan(root, findings, &scan, None)
}

pub fn build_report_from_scan(
    root: &Path,
    findings: Vec<Finding>,
    scan: &ScanResult,
    baseline: Option<(&Path, &LintReport)>,
) -> LintReport {
    let mut surface_counts: BTreeMap<String, usize> = BTreeMap::new();
    for surface in &scan.surfaces {
        *surface_counts.entry(surface.kind.to_string()).or_insert(0) += 1;
    }

    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut infos = 0usize;
    let mut weighted_penalty_units = 0u64;
    let mut penalty_by_kind: BTreeMap<String, u64> = BTreeMap::new();
    for finding in &findings {
        match finding.severity {
            Severity::Error => errors += 1,
            Severity::Warning => warnings += 1,
            Severity::Info => infos += 1,
        }
        let evidence = scan.surfaces.iter().find(|surface| {
            surface.path == finding.path
                && finding.line.is_none_or(|line| line == surface.line)
                && finding.kind.is_none_or(|kind| kind == surface.kind)
        });
        let confidence = evidence.map_or(100u64, |surface| u64::from(surface.confidence));
        let exposure = evidence.map_or(3u64, |surface| u64::from(surface.exposure));
        let semantic_weight = evidence.map_or(100u64, |surface| {
            u64::from(surface.significance.semantic_weight)
        });
        let penalty = severity_weight(finding.severity)
            .saturating_mul(confidence)
            .saturating_mul(exposure)
            .saturating_mul(semantic_weight)
            .div_ceil(10_000);
        weighted_penalty_units = weighted_penalty_units.saturating_add(penalty);
        let kind = finding
            .kind
            .map(|k| k.to_string())
            .unwrap_or_else(|| "general".to_string());
        *penalty_by_kind.entry(kind).or_insert(0) += penalty;
    }

    let exposure_units = scan
        .surfaces
        .iter()
        .map(weighted_exposure)
        .sum::<u64>()
        .max(3);
    let density = weighted_penalty_units
        .saturating_mul(1_000)
        .div_ceil(exposure_units);
    let mut overall = 100u32.saturating_sub(
        u32::try_from(density.div_ceil(1_000))
            .unwrap_or(100)
            .min(100),
    );
    if !scan.complete {
        overall = overall.min(99);
    }

    let mut by_kind: BTreeMap<String, u32> = penalty_by_kind
        .into_iter()
        .map(|(kind, penalty)| {
            let denominator = scan
                .surfaces
                .iter()
                .filter(|surface| surface.kind.to_string() == kind)
                .map(weighted_exposure)
                .sum::<u64>()
                .max(3);
            let kind_density = penalty.saturating_mul(1_000).div_ceil(denominator);
            let score = 100u32.saturating_sub(
                u32::try_from(kind_density.div_ceil(1_000))
                    .unwrap_or(100)
                    .min(100),
            );
            (kind, score)
        })
        .collect();
    // Kinds that were scanned but have no findings still appear, at 100.
    for kind in surface_counts.keys() {
        by_kind.entry(kind.clone()).or_insert(100);
    }

    let baseline_delta = baseline.map(|(path, previous)| {
        let current: BTreeSet<String> = findings.iter().map(finding_fingerprint).collect();
        let old: BTreeSet<String> = previous.findings.iter().map(finding_fingerprint).collect();
        BaselineDelta {
            baseline_path: path.to_path_buf(),
            new_findings: current.difference(&old).count(),
            resolved_findings: old.difference(&current).count(),
        }
    });

    LintReport {
        schema_version: REPORT_SCHEMA_VERSION.into(),
        root: root.to_path_buf(),
        timestamp: chrono::Local::now().to_rfc3339(),
        scores: Scores {
            model_version: SCORING_MODEL_VERSION.into(),
            overall,
            by_kind,
            errors,
            warnings,
            infos,
            normalized_density_per_1000: u32::try_from(density).unwrap_or(u32::MAX),
            weighted_penalty_units,
        },
        surface_counts,
        findings,
        scan_complete: scan.complete,
        scan_diagnostics: scan.diagnostics.clone(),
        baseline_delta,
    }
}

fn weighted_exposure(surface: &Surface) -> u64 {
    u64::from(surface.exposure.max(1))
        .saturating_mul(u64::from(surface.significance.semantic_weight))
        .div_ceil(100)
}

fn finding_fingerprint(finding: &Finding) -> String {
    format!(
        "{}|{}|{}|{}",
        finding.rule_id,
        finding.path.display(),
        finding.line.unwrap_or(0),
        finding.message
    )
}

/// Render a report as human-readable text.
pub fn render_human(report: &LintReport) -> String {
    let mut out: Vec<String> = Vec::new();

    out.push(format!(
        "Brandi report — {} ({})",
        report.root.display(),
        report.timestamp
    ));
    out.push(String::new());

    let scanned = ordered_keys(&report.surface_counts)
        .into_iter()
        .map(|kind| format!("{kind} {}", report.surface_counts[kind]))
        .collect::<Vec<_>>()
        .join(", ");
    out.push(format!(
        "Surfaces scanned: {}",
        if scanned.is_empty() {
            "none".to_string()
        } else {
            scanned
        }
    ));
    out.push(format!(
        "Scan status: {} ({})",
        if report.scan_complete {
            "complete"
        } else {
            "incomplete"
        },
        report.schema_version
    ));
    for diagnostic in &report.scan_diagnostics {
        out.push(format!(
            "  {} {} — {}",
            if diagnostic.affects_completeness {
                "incomplete"
            } else {
                "skipped"
            },
            diagnostic.path.display(),
            diagnostic.message
        ));
    }
    out.push(String::new());

    // Findings grouped by path; groups ordered by path, entries by line.
    out.push("Findings:".to_string());
    let mut by_path: BTreeMap<&Path, Vec<&Finding>> = BTreeMap::new();
    for finding in &report.findings {
        by_path
            .entry(finding.path.as_path())
            .or_default()
            .push(finding);
    }
    for group in by_path.values_mut() {
        group.sort_by_key(|finding| finding.line.unwrap_or(0));
    }
    for (path, group) in &by_path {
        out.push(path.display().to_string());
        for finding in group {
            let marker = match finding.severity {
                Severity::Error => "✗",
                Severity::Warning => "⚠",
                Severity::Info => "ℹ",
            };
            let location = match finding.line {
                Some(line) => format!("L{line} "),
                None => String::new(),
            };
            let text = format!(
                "  {marker} {location}[{}] {}",
                finding.rule_id, finding.message
            );
            let line = match finding.severity {
                Severity::Error => output::styled(text, OutputSeverity::Error),
                Severity::Warning => output::styled(text, OutputSeverity::Warning),
                Severity::Info => output::styled(text, OutputSeverity::Info),
            };
            out.push(line.to_string());
            if let Some(suggestion) = &finding.suggestion {
                out.push(format!("    → {suggestion}"));
            }
        }
    }

    // One green checkmark per catalog rule that produced no findings.
    let failing_rules: BTreeSet<&str> = report
        .findings
        .iter()
        .map(|finding| finding.rule_id.as_str())
        .collect();
    let mut passing: Vec<String> = Vec::new();
    for (rule_id, description) in crate::rules::rule_catalog() {
        if !failing_rules.contains(rule_id) {
            passing.push(
                output::styled(
                    format!("✓ {rule_id} — {description}"),
                    OutputSeverity::Success,
                )
                .to_string(),
            );
        }
    }
    if !passing.is_empty() {
        out.push(String::new());
        out.extend(passing);
    }

    out.push(String::new());
    out.push(format!(
        "Errors: {}  Warnings: {}  Info: {}",
        report.scores.errors, report.scores.warnings, report.scores.infos
    ));

    let score = format!("Brand coherence: {}/100", report.scores.overall);
    let score = if report.scores.overall < 50 {
        output::styled(score, OutputSeverity::Error)
    } else if report.scores.overall < 80 {
        output::styled(score, OutputSeverity::Warning)
    } else {
        output::styled(score, OutputSeverity::Success)
    };
    out.push(score.to_string());
    out.push(format!(
        "  {}  density {}/1000  weighted units {}",
        report.scores.model_version,
        report.scores.normalized_density_per_1000,
        report.scores.weighted_penalty_units
    ));
    if let Some(delta) = &report.baseline_delta {
        out.push(format!(
            "  baseline {} — {} new, {} resolved",
            delta.baseline_path.display(),
            delta.new_findings,
            delta.resolved_findings
        ));
    }

    let by_kind = ordered_keys(&report.scores.by_kind)
        .into_iter()
        .map(|kind| format!("{kind} {}", report.scores.by_kind[kind]))
        .collect::<Vec<_>>()
        .join("  ");
    out.push(format!(
        "  {}",
        if by_kind.is_empty() {
            "none".to_string()
        } else {
            by_kind
        }
    ));

    out.join("\n")
}

/// Render a report as JSON.
pub fn render_json(report: &LintReport) -> Result<String> {
    Ok(serde_json::to_string_pretty(report)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn surface(kind: SurfaceKind, path: &str) -> Surface {
        Surface {
            kind,
            path: PathBuf::from(path),
            line: 1,
            text: "surface text".to_string(),
            context: "test".into(),
            confidence: 100,
            exposure: 3,
            significance: crate::types::SurfaceSignificance::default(),
            reason: "test surface".into(),
            start_byte: 0,
            end_byte: 12,
        }
    }

    fn finding(
        rule_id: &str,
        severity: Severity,
        kind: Option<SurfaceKind>,
        path: &str,
        line: Option<usize>,
    ) -> Finding {
        Finding {
            rule_id: rule_id.to_string(),
            severity,
            kind,
            path: PathBuf::from(path),
            line,
            message: format!("message for {rule_id}"),
            suggestion: None,
            semantics: crate::types::FindingSemantics::default(),
        }
    }

    #[test]
    fn scores_penalize_by_severity() {
        let surfaces = [surface(SurfaceKind::RepoDoc, "README.md")];
        let findings = vec![
            finding(
                "r1",
                Severity::Error,
                Some(SurfaceKind::RepoDoc),
                "README.md",
                Some(1),
            ),
            finding(
                "r2",
                Severity::Warning,
                Some(SurfaceKind::RepoDoc),
                "README.md",
                Some(2),
            ),
            finding(
                "r3",
                Severity::Info,
                Some(SurfaceKind::RepoDoc),
                "README.md",
                Some(3),
            ),
        ];
        let report = build_report(Path::new("/proj"), findings, &surfaces);
        assert_eq!(report.scores.overall, 100 - 10 - 4 - 1);
        assert_eq!(report.scores.errors, 1);
        assert_eq!(report.scores.warnings, 1);
        assert_eq!(report.scores.infos, 1);
        assert_eq!(report.scores.by_kind["repo_doc"], 85);
    }

    #[test]
    fn scores_saturate_at_zero() {
        let findings = (0..11)
            .map(|i| finding(&format!("r{i}"), Severity::Error, None, "f", None))
            .collect();
        let report = build_report(Path::new("/proj"), findings, &[]);
        assert_eq!(report.scores.overall, 0);
        assert_eq!(report.scores.by_kind["general"], 0);
        assert_eq!(report.scores.errors, 11);
    }

    #[test]
    fn by_kind_groups_findings_and_keeps_clean_kinds_at_100() {
        let surfaces = [
            surface(SurfaceKind::RepoDoc, "README.md"),
            surface(SurfaceKind::Style, "theme.css"),
            surface(SurfaceKind::Style, "other.css"),
        ];
        let findings = vec![
            finding(
                "r1",
                Severity::Error,
                Some(SurfaceKind::RepoDoc),
                "README.md",
                Some(1),
            ),
            finding("r2", Severity::Warning, None, "README.md", Some(2)),
        ];
        let report = build_report(Path::new("/proj"), findings, &surfaces);
        assert_eq!(report.scores.by_kind["repo_doc"], 90);
        assert_eq!(report.scores.by_kind["general"], 96);
        // Scanned but no findings: still present, at 100.
        assert_eq!(report.scores.by_kind["style"], 100);
        assert_eq!(report.scores.by_kind.len(), 3);
        assert_eq!(report.surface_counts["repo_doc"], 1);
        assert_eq!(report.surface_counts["style"], 2);
        // The global score is density-normalized across all three surfaces;
        // absolute severity counts and per-kind scores remain separate.
        assert_eq!(report.scores.overall, 95);
    }

    #[test]
    fn timestamp_is_rfc3339() {
        let report = build_report(Path::new("/proj"), vec![], &[]);
        assert!(chrono::DateTime::parse_from_rfc3339(&report.timestamp).is_ok());
    }

    #[test]
    fn scoring_v2_reports_stable_density_across_project_sizes() {
        let small: Vec<Surface> = (0..10)
            .map(|index| surface(SurfaceKind::UiString, &format!("small/{index}.rs")))
            .collect();
        let small_findings = vec![finding(
            "r1",
            Severity::Warning,
            Some(SurfaceKind::UiString),
            "small/0.rs",
            Some(1),
        )];
        let large: Vec<Surface> = (0..100)
            .map(|index| surface(SurfaceKind::UiString, &format!("large/{index}.rs")))
            .collect();
        let large_findings: Vec<Finding> = (0..10)
            .map(|index| {
                finding(
                    "r1",
                    Severity::Warning,
                    Some(SurfaceKind::UiString),
                    &format!("large/{index}.rs"),
                    Some(1),
                )
            })
            .collect();
        let small_report = build_report(Path::new("/small"), small_findings, &small);
        let large_report = build_report(Path::new("/large"), large_findings, &large);
        assert_eq!(
            small_report.scores.normalized_density_per_1000,
            large_report.scores.normalized_density_per_1000
        );
        assert_eq!(small_report.scores.overall, large_report.scores.overall);
        assert_eq!(small_report.scores.model_version, SCORING_MODEL_VERSION);
    }

    #[test]
    fn incomplete_scan_cannot_report_a_clean_hundred() {
        let scan = ScanResult {
            schema_version: crate::surface::EXTRACTION_SCHEMA_VERSION.into(),
            complete: false,
            surfaces: Vec::new(),
            diagnostics: vec![crate::types::ScanDiagnostic {
                path: PathBuf::from("unreadable.rs"),
                code: "read-error".into(),
                message: "permission denied".into(),
                affects_completeness: true,
            }],
            files_considered: 1,
            files_scanned: 0,
        };
        let report = build_report_from_scan(Path::new("/proj"), Vec::new(), &scan, None);
        assert!(!report.scan_complete);
        assert_eq!(report.scores.overall, 99);
    }

    #[test]
    fn render_json_parses_back_with_expected_keys() {
        let surfaces = [surface(SurfaceKind::Doc, "docs/a.md")];
        let findings = vec![finding(
            "r1",
            Severity::Warning,
            Some(SurfaceKind::Doc),
            "docs/a.md",
            Some(7),
        )];
        let report = build_report(Path::new("/proj"), findings, &surfaces);
        let json = render_json(&report).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        for key in ["root", "timestamp", "scores", "findings"] {
            assert!(value.get(key).is_some(), "missing key {key} in {json}");
        }
        assert_eq!(value["root"], "/proj");
        assert_eq!(value["scores"]["overall"], 96);
        assert_eq!(value["findings"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn finding_kind_and_score_map_keys_use_the_same_casing_in_json() {
        // Regression test: `Finding.kind` used to serialize as default
        // PascalCase ("UiString") while `scores.by_kind` and
        // `surface_counts` keys used `SurfaceKind`'s Display impl
        // (snake_case, "ui_string") for the same concept — a consumer
        // correlating a finding to its kind's score bucket by string
        // equality would silently fail to match.
        let surfaces = [surface(SurfaceKind::UiString, "src/cli.rs")];
        let findings = vec![finding(
            "r1",
            Severity::Warning,
            Some(SurfaceKind::UiString),
            "src/cli.rs",
            Some(3),
        )];
        let report = build_report(Path::new("/proj"), findings, &surfaces);
        let json = render_json(&report).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["findings"][0]["kind"], "ui_string");
        assert!(value["scores"]["by_kind"].get("ui_string").is_some());
        assert!(value["surface_counts"].get("ui_string").is_some());
    }

    #[test]
    fn render_human_shows_score_and_checkmarks_for_passing_rules() {
        let catalog = crate::rules::rule_catalog();
        assert!(!catalog.is_empty(), "rule catalog should not be empty");
        let surfaces = [surface(SurfaceKind::RepoDoc, "README.md")];
        let report = build_report(Path::new("/proj"), vec![], &surfaces);
        let output = render_human(&report);
        assert!(output.contains("Brandi report — /proj"), "got:\n{output}");
        assert!(
            output.contains("Surfaces scanned: repo_doc 1"),
            "got:\n{output}"
        );
        assert!(
            output.contains("Errors: 0  Warnings: 0  Info: 0"),
            "got:\n{output}"
        );
        assert!(
            output.contains("Brand coherence: 100/100"),
            "got:\n{output}"
        );
        // No findings: every catalog rule gets a green checkmark line.
        for (rule_id, _description) in &catalog {
            assert!(
                output.contains(&format!("✓ {rule_id}")),
                "missing ✓ line for {rule_id}:\n{output}"
            );
        }
    }

    #[test]
    fn render_human_groups_sorts_and_hides_failing_rules() {
        let catalog = crate::rules::rule_catalog();
        assert!(!catalog.is_empty(), "rule catalog should not be empty");
        let (failing_id, _) = catalog[0];

        let mut with_suggestion = finding(
            failing_id,
            Severity::Error,
            Some(SurfaceKind::Doc),
            "b.md",
            Some(30),
        );
        with_suggestion.suggestion = Some("do this instead".to_string());
        let findings = vec![
            with_suggestion,
            finding(
                "other",
                Severity::Warning,
                Some(SurfaceKind::Doc),
                "b.md",
                Some(12),
            ),
            finding(
                "other2",
                Severity::Info,
                Some(SurfaceKind::RepoDoc),
                "a.md",
                None,
            ),
        ];
        let report = build_report(Path::new("/proj"), findings, &[]);
        let output = render_human(&report);

        // Penalty 10 + 4 + 1 = 15.
        assert!(output.contains("Brand coherence: 85/100"), "got:\n{output}");
        // Groups sorted by path: a.md block before b.md block.
        let a_pos = output.find("a.md").expect("a.md missing");
        let b_pos = output.find("b.md").expect("b.md missing");
        assert!(a_pos < b_pos, "paths not sorted:\n{output}");
        // Within b.md: L12 entry before L30 entry.
        let l12 = output.find("L12").expect("L12 missing");
        let l30 = output.find("L30").expect("L30 missing");
        assert!(l12 < l30, "lines not sorted:\n{output}");
        // Suggestion renders indented under its finding.
        assert!(output.contains("    → do this instead"), "got:\n{output}");
        // A rule with findings gets no ✓ line.
        assert!(
            !output.contains(&format!("✓ {failing_id}")),
            "unexpected ✓ for failing rule {failing_id}:\n{output}"
        );
    }
}
