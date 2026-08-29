//! Versioned, deterministic surface-extraction corpus evaluation.

use crate::error::{BrandiError, Result};
use crate::surface;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct Corpus {
    schema: String,
    version: String,
    cases: Vec<CorpusCase>,
}

#[derive(Debug, Deserialize)]
struct CorpusCase {
    id: String,
    profile: String,
    path: PathBuf,
    content: String,
    expected_surfaces: Vec<String>,
    expected_non_surfaces: Vec<String>,
    #[serde(default)]
    expected_findings: Vec<ExpectedFinding>,
}

#[derive(Debug, Deserialize)]
struct ExpectedFinding {
    rule_id: String,
    severity: String,
}

#[derive(Debug, Serialize)]
pub struct EvaluationResult {
    pub corpus_version: String,
    pub extraction_schema_version: String,
    pub cases: usize,
    pub true_positives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub precision: f64,
    pub recall: f64,
    pub false_positive_clusters: BTreeMap<String, usize>,
    pub false_negative_clusters: BTreeMap<String, usize>,
    pub rule_labels: usize,
    pub stability_hash: String,
}

pub fn evaluate_corpus(path: &Path) -> Result<EvaluationResult> {
    let bytes = std::fs::read(path)?;
    let corpus: Corpus = serde_json::from_slice(&bytes)?;
    if corpus.schema != "brandi-evaluation-corpus" || corpus.version != "1" {
        return Err(BrandiError::Invalid(
            "unsupported evaluation corpus schema/version; use brandi-evaluation-corpus v1".into(),
        ));
    }
    let mut true_positives = 0usize;
    let mut false_positives = 0usize;
    let mut false_negatives = 0usize;
    let mut fp_clusters = BTreeMap::new();
    let mut fn_clusters = BTreeMap::new();
    let mut stability = Vec::new();
    let mut rule_labels = 0usize;

    for case in &corpus.cases {
        let predicted = surface::extract_content(&case.path, &case.content).map_err(|error| {
            BrandiError::Invalid(format!("evaluation case {} failed: {error}", case.id))
        })?;
        let actual: BTreeSet<String> = predicted
            .into_iter()
            .map(|surface| surface.text.trim().to_string())
            .filter(|text| !text.is_empty())
            .collect();
        let expected: BTreeSet<String> = case.expected_surfaces.iter().cloned().collect();
        let explicit_negative: BTreeSet<String> =
            case.expected_non_surfaces.iter().cloned().collect();
        let tp = actual.intersection(&expected).count();
        let fp = actual.difference(&expected).count();
        let missed = expected.difference(&actual).count();
        true_positives += tp;
        false_positives += fp;
        false_negatives += missed;
        if fp > 0 {
            *fp_clusters.entry(case.profile.clone()).or_insert(0) += fp;
        }
        if missed > 0 {
            *fn_clusters.entry(case.profile.clone()).or_insert(0) += missed;
        }
        for negative in actual.intersection(&explicit_negative) {
            stability.push(format!("unexpected-negative:{}:{negative}", case.id));
        }
        for text in actual {
            stability.push(format!("{}:{text}", case.id));
        }
        rule_labels += case
            .expected_findings
            .iter()
            .filter(|label| !label.rule_id.is_empty() && !label.severity.is_empty())
            .count();
    }
    stability.sort();
    let precision = ratio(true_positives, true_positives + false_positives);
    let recall = ratio(true_positives, true_positives + false_negatives);
    let mut digest = Sha256::new();
    for line in stability {
        digest.update(line.as_bytes());
        digest.update(b"\n");
    }
    Ok(EvaluationResult {
        corpus_version: corpus.version,
        extraction_schema_version: surface::EXTRACTION_SCHEMA_VERSION.into(),
        cases: corpus.cases.len(),
        true_positives,
        false_positives,
        false_negatives,
        precision,
        recall,
        false_positive_clusters: fp_clusters,
        false_negative_clusters: fn_clusters,
        rule_labels,
        stability_hash: format!("sha256:{:x}", digest.finalize()),
    })
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        1.0
    } else {
        numerator as f64 / denominator as f64
    }
}

pub fn render_human(result: &EvaluationResult) -> String {
    format!(
        "Corpus {} — {} cases\nprecision {:.4} ({} TP, {} FP)\nrecall {:.4} ({} FN)\nrule labels {}\nstability {}\n{}",
        result.corpus_version,
        result.cases,
        result.precision,
        result.true_positives,
        result.false_positives,
        result.recall,
        result.false_negatives,
        result.rule_labels,
        result.stability_hash,
        if result.precision >= 0.95 && result.recall >= 0.90 {
            "PASS"
        } else {
            "FAIL"
        }
    )
}
