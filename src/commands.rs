//! Command implementations: one function per subcommand, wired to the
//! brief/guidelines and the (separately owned) surface/rules/report/assets/
//! social/daemon modules.

use crate::brief::Brief;
use crate::cli::{AssetKindArg, Format, UninitializedAction};
use crate::error::{BrandiError, Result};
use crate::guidelines::Guidelines;
use crate::{
    adb_social, assets, automation, creative, daemon, propose, report, rules,
    scan as scan_pipeline, social, surface, tape,
};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

/// Resolve the Brandi project associated with a path by walking upward to the
/// nearest directory containing `.brandi/`. With no target, resolution starts
/// at the process working directory. File targets start from their parent.
fn associated_project_root(target: Option<&Path>) -> Result<PathBuf> {
    let requested = match target {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => std::env::current_dir()?.join(path),
        None => std::env::current_dir()?,
    };
    let resolved = requested.canonicalize().map_err(|_| {
        BrandiError::NotFound(format!(
            "target path does not exist: {}",
            requested.display()
        ))
    })?;
    let start = if resolved.is_file() {
        resolved.parent().unwrap_or(&resolved)
    } else {
        resolved.as_path()
    };
    start
        .ancestors()
        .find(|candidate| candidate.join(".brandi").is_dir())
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            BrandiError::NotFound(format!(
                "no Brandi project found at or above {}",
                requested.display()
            ))
        })
}

#[derive(serde::Serialize)]
struct BriefOutput<'a> {
    identity: &'a crate::brief::Identity,
    audience: &'a crate::brief::Audience,
}

#[derive(serde::Serialize)]
struct GuidelinesOutput<'a> {
    voice: &'a crate::guidelines::Voice,
    visual: &'a crate::guidelines::Visual,
    prohibited: &'a crate::guidelines::Prohibited,
    rules: &'a crate::guidelines::RuleOverrides,
}

// Transitional stream gate: every legacy call in this module is routed
// through the output layer while command results are migrated to typed events.
// The macros deliberately propagate I/O errors, including broken pipes.
macro_rules! println {
    () => {{
        crate::output::line("")?;
    }};
    ($($arg:tt)*) => {{
        crate::output::line(format_args!($($arg)*))?;
    }};
}

macro_rules! print {
    ($($arg:tt)*) => {{
        crate::output::write(format_args!($($arg)*))?;
    }};
}

macro_rules! eprintln {
    ($($arg:tt)*) => {{
        crate::output::warning(format_args!($($arg)*))?;
    }};
}

pub fn direct_run(
    path: Option<&Path>,
    portfolio: bool,
    options: &crate::direct::DirectOptions,
    format: &Format,
) -> Result<i32> {
    let progress = crate::output::ProgressGuard::start(if options.preflight_only {
        "Projecting direct pre-flight evidence"
    } else {
        "Routing generation and independent asset review"
    });
    let report = crate::direct::run_scoped(path, portfolio, options)?;
    progress.finish();
    match format {
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(&report)?),
        Format::Human => {
            println!(
                "Brandi direct — {} project{}",
                report.projects.len(),
                if report.projects.len() == 1 { "" } else { "s" }
            );
            for project in &report.projects {
                println!("\n{}", project.project.display());
                if let Some(error) = &project.error {
                    println!("  failed: {error}");
                    continue;
                }
                let outcome = project
                    .outcome
                    .as_ref()
                    .expect("successful direct project has outcome");
                println!(
                    "  plan: {} · {:?} · {} actions · ${:.4} estimated",
                    outcome.plan_id, outcome.strategy, outcome.actions, outcome.estimated_cost_usd
                );
                println!(
                    "  brief section {}: {}",
                    outcome.section, outcome.section_value
                );
                println!("  routes: {}", outcome.routes.join(", "));
                println!(
                    "  exact sRGB palette: {}",
                    outcome
                        .palette
                        .iter()
                        .map(|color| color.hex.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                println!("  visual pre-flight: {}", outcome.preflight.svg.display());
                println!("  RUGID scene: {}", outcome.preflight.rdf.display());
                println!("  machine plan: {}", outcome.preflight.json.display());
                if outcome.preflight_only {
                    println!("  pre-flight only; no provider was called");
                } else {
                    println!(
                        "  generated: {} · evidence: {}",
                        outcome.generated,
                        outcome.evidence.display()
                    );
                    for (objective, selected) in &outcome.selected {
                        println!("  selected {objective}: {selected}");
                    }
                }
                if let Some(token) = &outcome.postflight_confirmation {
                    println!("  post-flight confirmation: {token}");
                }
            }
        }
    }
    Ok(if report.projects.iter().any(|item| item.error.is_some()) {
        1
    } else {
        0
    })
}

pub fn creative_run(
    mode: creative::CreativeMode,
    path: Option<&Path>,
    portfolio: bool,
    format: &Format,
) -> Result<i32> {
    let progress = crate::output::ProgressGuard::start(match mode {
        creative::CreativeMode::Direct => "Generating brand assets",
        creative::CreativeMode::Reshoot => "Revising brand assets",
    });
    let report = creative::run(mode, path, portfolio)?;
    progress.finish();
    match format {
        Format::Human => print!("{}", report.render_human()),
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(&report)?),
    }
    Ok(if report.failed() == 0 { 0 } else { 1 })
}

/// `brandi init`: scaffold `.brandi/` and print what was created.
pub fn init(path: &Path) -> Result<()> {
    let created = scaffold_project(path)?;
    if matches!(
        crate::output::context().format,
        Format::Json | Format::Jsonl
    ) {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "brandi-init-v1",
                "created": created,
                "changed": !created.is_empty()
            }))?
        );
        return Ok(());
    }
    if created.is_empty() {
        println!("already exists, nothing to do");
    } else {
        println!("created {} files:", created.len());
        for file in created {
            println!("  {}", file.display());
        }
    }
    Ok(())
}

fn scaffold_project(path: &Path) -> Result<Vec<PathBuf>> {
    let mut created = Brief::scaffold(path)?;
    created.extend(Guidelines::scaffold(path)?);
    Ok(created)
}

/// `brandi brief [PATH]`: display the brief for the nearest associated project.
pub fn brief_show(target: Option<&Path>, format: &Format) -> Result<()> {
    let root = associated_project_root(target)?;
    let brief = Brief::load(&root)?;
    let output = BriefOutput {
        identity: &brief.identity,
        audience: &brief.audience,
    };
    match format {
        Format::Human => print!("{}", serde_yaml::to_string(&output)?),
        Format::Json | Format::Jsonl => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "brandi-brief-v1",
                "project": root,
                "brief": output,
            }))?
        ),
    }
    Ok(())
}

/// `brandi guidelines [PATH]`: display the guidelines for the nearest
/// associated project.
pub fn guidelines_show(target: Option<&Path>, format: &Format) -> Result<()> {
    let root = associated_project_root(target)?;
    let guidelines = Guidelines::load(&root)?;
    let output = GuidelinesOutput {
        voice: &guidelines.voice,
        visual: &guidelines.visual,
        prohibited: &guidelines.prohibited,
        rules: &guidelines.rules_overrides,
    };
    match format {
        Format::Human => print!("{}", serde_yaml::to_string(&output)?),
        Format::Json | Format::Jsonl => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "brandi-guidelines-v1",
                "project": root,
                "guidelines": output,
            }))?
        ),
    }
    Ok(())
}

/// `brandi brief validate`: print brief warnings, or "brief OK".
///
/// Hard failures print one line per problem, the same shape as the soft
/// warnings below them — `Brief::validate` collapses them into a single
/// semicolon-joined `BrandiError::Invalid` for callers that just need a
/// one-line `Display`, but that read as an undifferentiated wall of text
/// here, unlike the one-per-line warning path for the exact same command.
pub fn brief_validate(path: &Path) -> Result<()> {
    match Brief::validate(path) {
        Ok(warnings) => {
            if matches!(
                crate::output::context().format,
                Format::Json | Format::Jsonl
            ) {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": "brandi-validation-v1",
                        "subject": "brief",
                        "valid": true,
                        "warnings": warnings
                    }))?
                );
                return Ok(());
            }
            if warnings.is_empty() {
                println!("brief OK");
            } else {
                for warning in warnings {
                    println!("warning: {warning}");
                }
            }
            Ok(())
        }
        Err(BrandiError::Invalid(joined)) => {
            let problems: Vec<&str> = joined.split("; ").collect();
            if matches!(
                crate::output::context().format,
                Format::Json | Format::Jsonl
            ) {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": "brandi-validation-v1",
                        "subject": "brief",
                        "valid": false,
                        "problems": problems
                    }))?
                );
            } else {
                for problem in &problems {
                    println!("problem: {problem}");
                }
            }
            Err(BrandiError::Invalid(
                "brief validation failed; see the problems above".to_string(),
            ))
        }
        Err(e) => Err(e),
    }
}

/// `brandi guidelines validate`: print guidelines warnings, or "guidelines OK".
///
/// Hard failures print one line per problem, the same shape as the soft
/// warnings below them — `Guidelines::validate` collapses them into a single
/// semicolon-joined `BrandiError::Invalid` for callers that just need a
/// one-line `Display`, but that read as an undifferentiated wall of text
/// here, unlike the one-per-line warning path for the exact same command.
pub fn guidelines_validate(path: &Path) -> Result<()> {
    match Guidelines::validate(path) {
        Ok(warnings) => {
            if matches!(
                crate::output::context().format,
                Format::Json | Format::Jsonl
            ) {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": "brandi-validation-v1",
                        "subject": "guidelines",
                        "valid": true,
                        "warnings": warnings
                    }))?
                );
                return Ok(());
            }
            if warnings.is_empty() {
                println!("guidelines OK");
            } else {
                for warning in warnings {
                    println!("warning: {warning}");
                }
            }
            Ok(())
        }
        Err(BrandiError::Invalid(joined)) => {
            let problems: Vec<&str> = joined.split("; ").collect();
            if matches!(
                crate::output::context().format,
                Format::Json | Format::Jsonl
            ) {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": "brandi-validation-v1",
                        "subject": "guidelines",
                        "valid": false,
                        "problems": problems
                    }))?
                );
            } else {
                for problem in &problems {
                    println!("problem: {problem}");
                }
            }
            Err(BrandiError::Invalid(
                "guidelines validation failed; see the problems above".to_string(),
            ))
        }
        Err(e) => Err(e),
    }
}

/// `brandi scan`: run the evidence-rich, milestone-driven scan pipeline.
pub fn scan(
    path: Option<&Path>,
    if_uninitialized: UninitializedAction,
    format: &Format,
) -> Result<()> {
    let root = match scan_pipeline::resolve_project_target(path)? {
        scan_pipeline::ProjectTarget::Configured(root) => root,
        scan_pipeline::ProjectTarget::Uninitialized(root) => {
            let choice = match if_uninitialized {
                UninitializedAction::Prompt => prompt_uninitialized_target(&root, format)?,
                choice => choice,
            };
            match choice {
                UninitializedAction::Init => {
                    scaffold_project(&root)?;
                    root
                }
                UninitializedAction::Skip => {
                    render_skipped_scan(&root, format)?;
                    return Ok(());
                }
                UninitializedAction::Prompt => unreachable!("prompt resolves to init or skip"),
            }
        }
    };
    let report = scan_pipeline::run(Some(&root))?;
    match format {
        Format::Human => println!("{}", scan_pipeline::render_human(&report)),
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(&report)?),
    }
    Ok(())
}

fn prompt_uninitialized_target(root: &Path, format: &Format) -> Result<UninitializedAction> {
    if *format != Format::Human
        || !std::io::stdin().is_terminal()
        || !crate::output::context().stderr_terminal
    {
        return Err(BrandiError::Invalid(format!(
            "target {} has no .brandi configuration; interactive confirmation is unavailable, so pass `--if-uninitialized init` or `--if-uninitialized skip`",
            root.display()
        )));
    }
    loop {
        crate::output::prompt(format!(
            "No .brandi configuration exists for {}. Run `brandi init` or skip this target? [i/S] ",
            root.display()
        ))?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        match parse_uninitialized_answer(&answer) {
            Some(choice) => return Ok(choice),
            None => crate::output::warning("Enter `i` to initialize or `s` to skip.")?,
        }
    }
}

fn parse_uninitialized_answer(answer: &str) -> Option<UninitializedAction> {
    match answer.trim().to_ascii_lowercase().as_str() {
        "i" | "init" | "y" | "yes" => Some(UninitializedAction::Init),
        "" | "s" | "skip" | "n" | "no" => Some(UninitializedAction::Skip),
        _ => None,
    }
}

fn render_skipped_scan(root: &Path, format: &Format) -> Result<()> {
    match format {
        Format::Human => println!(
            "Skipped target {} because .brandi is not initialized.",
            root.display()
        ),
        Format::Json | Format::Jsonl => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "brandi-scan-skip-v1",
                "status": "skipped",
                "target": root,
                "reason": "missing .brandi configuration"
            }))?
        ),
    }
    Ok(())
}

#[cfg(test)]
mod scan_prompt_tests {
    use super::*;

    #[test]
    fn prompt_answers_map_to_init_or_skip_with_skip_as_default() {
        for answer in ["i", "init", "y", "yes"] {
            assert_eq!(
                parse_uninitialized_answer(answer),
                Some(UninitializedAction::Init)
            );
        }
        for answer in ["", "s", "skip", "n", "no"] {
            assert_eq!(
                parse_uninitialized_answer(answer),
                Some(UninitializedAction::Skip)
            );
        }
        assert_eq!(parse_uninitialized_answer("maybe"), None);
    }
}

/// `brandi propose`: scan stylisable surfaces and print budgeted revisions.
pub fn propose(path: &Path, budget: usize) -> Result<()> {
    crate::output::event(&crate::output::OutputEvent::CommandStarted {
        command: "propose".into(),
        sequence: 0,
    })?;
    crate::output::verbose(format!(
        "phase: load brief and guidelines ({})",
        path.display()
    ))?;
    let brief = Brief::load(path)?;
    let guidelines = Guidelines::load(path)?;
    let progress = crate::output::ProgressGuard::start("Proposing brand revisions");
    let report = propose::propose_report(&brief, &guidelines, path, budget)?;
    progress.finish();
    crate::output::verbose(format!(
        "phase: scan complete — {} surfaces in {} files ({} considered)",
        report.surfaces_scanned, report.files_scanned, report.files_considered
    ))?;
    crate::output::verbose(format!(
        "phase: rules complete — {} findings",
        report.findings
    ))?;
    crate::output::verbose(format!(
        "phase: proposals complete — {} generated",
        report.proposals.len()
    ))?;
    match crate::output::context().format {
        Format::Json | Format::Jsonl => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "brandi-proposals-v3",
                "budget": budget,
                "generated": report.proposals.len(),
                "surfaces_scanned": report.surfaces_scanned,
                "files_considered": report.files_considered,
                "files_scanned": report.files_scanned,
                "findings": report.findings,
                "typography": report.typography,
                "proposals": report.proposals
            }))?
        ),
        Format::Human => println!("{}", propose::render_human(&report, budget)),
    }
    crate::output::event(&crate::output::OutputEvent::CommandFinished {
        status: "ok".into(),
        summary: format!("{} proposals", report.proposals.len()),
        sequence: 1,
    })?;
    Ok(())
}

/// `brandi check <FILE>`: lint one file and print a human report.
pub fn check(path: &Path, file: &Path) -> Result<()> {
    let brief = Brief::load(path)?;
    let guidelines = Guidelines::load(path)?;
    let findings = rules::lint_file(&brief, &guidelines, path, file)?;
    let surfaces = surface::scan_file(path, file)?;
    let report = report::build_report(path, findings, &surfaces);
    match crate::output::context().format {
        Format::Human => println!("{}", report::render_human(&report)),
        Format::Json | Format::Jsonl => println!("{}", report::render_json(&report)?),
    }
    Ok(())
}

/// `brandi lint`: lint the whole project, render the report, and return the
/// process exit code (1 when overall < fail_under, or when --strict and any
/// error-severity finding exists; otherwise 0).
pub fn lint(
    path: &Path,
    format: &Format,
    fail_under: u32,
    strict: bool,
    baseline_path: Option<&Path>,
) -> Result<i32> {
    let progress = crate::output::ProgressGuard::start("Linting brand surfaces");
    let brief = Brief::load(path)?;
    let guidelines = Guidelines::load(path)?;
    let scan = surface::scan_project(path)?;
    let findings = rules::run_rules_on_surfaces(&brief, &guidelines, path, &scan.surfaces);
    let baseline = baseline_path
        .map(|baseline| {
            let bytes = std::fs::read(baseline)?;
            let report = serde_json::from_slice(&bytes)?;
            Ok::<_, BrandiError>((baseline, report))
        })
        .transpose()?;
    let report = report::build_report_from_scan(
        path,
        findings,
        &scan,
        baseline.as_ref().map(|(path, report)| (*path, report)),
    );
    progress.finish();
    match format {
        Format::Human => println!("{}", report::render_human(&report)),
        Format::Json | Format::Jsonl => println!("{}", report::render_json(&report)?),
    }
    if !report.scan_complete
        || report.scores.overall < fail_under
        || (strict && report.scores.errors > 0)
        || report
            .baseline_delta
            .as_ref()
            .is_some_and(|delta| delta.new_findings > 0)
    {
        Ok(1)
    } else {
        Ok(0)
    }
}

pub fn evaluate(
    corpus: &Path,
    fail_under_precision: f64,
    fail_under_recall: f64,
    format: &Format,
) -> Result<i32> {
    let progress = crate::output::ProgressGuard::start("Evaluating extractor corpus");
    let result = crate::evaluation::evaluate_corpus(corpus)?;
    progress.finish();
    match format {
        Format::Human => println!("{}", crate::evaluation::render_human(&result)),
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(&result)?),
    }
    Ok(
        if result.precision + f64::EPSILON < fail_under_precision
            || result.recall + f64::EPSILON < fail_under_recall
        {
            1
        } else {
            0
        },
    )
}

/// `brandi assets check <IMAGES...>`: check images against the asset spec.
pub fn assets_check(path: &Path, images: &[PathBuf], kind: &AssetKindArg) -> Result<()> {
    let guidelines = Guidelines::load(path)?;
    let kind = match kind {
        AssetKindArg::Auto => assets::AssetKind::Auto,
        AssetKindArg::SocialCard => assets::AssetKind::SocialCard,
        AssetKindArg::Thumbnail => assets::AssetKind::Thumbnail,
        AssetKindArg::Icon => assets::AssetKind::Icon,
    };
    let result = assets::check_assets(images, &kind, &guidelines)?;
    match crate::output::context().format {
        Format::Human => println!("{result}"),
        Format::Json | Format::Jsonl => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "brandi-asset-check-v1",
                "result": result
            }))?
        ),
    }
    Ok(())
}

/// `brandi assets list`: list discovered brand assets.
pub fn assets_list(path: &Path) -> Result<()> {
    let result = assets::list_assets(path)?;
    match crate::output::context().format {
        Format::Human => println!("{result}"),
        Format::Json | Format::Jsonl => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "brandi-asset-list-v1",
                "result": result
            }))?
        ),
    }
    Ok(())
}

/// `brandi assets audit`: project-wide asset coherence and utilisation
/// audit (duplicates, icon-set drift, file-size budget, reference counts).
pub fn assets_audit(path: &Path, format: &Format) -> Result<()> {
    let progress = crate::output::ProgressGuard::start("Auditing brand assets");
    let guidelines = Guidelines::load(path)?;
    let audit = assets::audit_assets(path, &guidelines)?;
    progress.finish();
    match format {
        Format::Human => println!("{}", assets::render_audit(&audit)),
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(&audit)?),
    }
    Ok(())
}

/// `brandi social graph`: render the narrative graph.
pub fn social_graph(path: &Path) -> Result<()> {
    let brief = Brief::load(path)?;
    let graph = social::render_graph(&brief);
    match crate::output::context().format {
        Format::Human => println!("{graph}"),
        Format::Json | Format::Jsonl => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "brandi-social-graph-v1",
                "graph": graph
            }))?
        ),
    }
    Ok(())
}

/// `brandi social plan [--segment NAME]`: render a content plan.
pub fn social_plan(path: &Path, segment: Option<&str>) -> Result<()> {
    let brief = Brief::load(path)?;
    let plan = social::render_plan(&brief, segment);
    match crate::output::context().format {
        Format::Human => println!("{plan}"),
        Format::Json | Format::Jsonl => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "brandi-social-plan-v1",
                "segment": segment,
                "plan": plan
            }))?
        ),
    }
    Ok(())
}

pub fn social_tui(path: &Path) -> Result<()> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(BrandiError::Invalid(
            "interactive social account management requires a terminal; use `brandi social accounts list --format json` for automation".into(),
        ));
    }
    let current = std::env::current_exe()?;
    let sibling = current.with_file_name("brandi-tui");
    let executable = std::env::var_os("BRANDI_TUI")
        .map(PathBuf::from)
        .filter(|candidate| candidate.is_file())
        .or_else(|| sibling.is_file().then_some(sibling))
        .or_else(|| find_social_tui_on_path())
        .ok_or_else(|| {
            BrandiError::NotFound("brandi-tui; build `tui/` or set BRANDI_TUI".into())
        })?;
    let status = std::process::Command::new(executable)
        .arg("--path")
        .arg(path)
        .arg("--brandi")
        .arg(current)
        .status()?;
    if !status.success() {
        return Err(BrandiError::Invalid(format!(
            "social TUI exited with {status}"
        )));
    }
    Ok(())
}

fn find_social_tui_on_path() -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .map(|directory| directory.join("brandi-tui"))
        .find(|candidate| candidate.is_file())
}

pub fn social_accounts_list(path: &Path, format: &Format) -> Result<()> {
    Brief::load(path)?;
    let registry = social::load_accounts(path)?;
    render_social_accounts(&registry, format)
}

pub fn social_accounts_connect(
    path: &Path,
    input: social::ConnectSocialAccount,
    format: &Format,
) -> Result<()> {
    Brief::load(path)?;
    let registry = social::connect_account(path, input)?;
    render_social_accounts(&registry, format)
}

pub fn social_accounts_disconnect(
    path: &Path,
    id: &str,
    confirmation: &str,
    format: &Format,
) -> Result<()> {
    Brief::load(path)?;
    let registry = social::disconnect_account(path, id, confirmation)?;
    render_social_accounts(&registry, format)
}

fn render_social_accounts(registry: &social::SocialAccountRegistry, format: &Format) -> Result<()> {
    match format {
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(registry)?),
        Format::Human => {
            println!("Social accounts · {} linked", registry.accounts.len());
            if registry.accounts.is_empty() {
                println!("No accounts linked. Run `brandi social` to open the account workspace.");
            }
            for account in &registry.accounts {
                println!(
                    "{}  @{}  {}  source:content+metrics  surface:profile+publishing  credential:{} ({})",
                    account.provider,
                    account.handle,
                    account.display_name,
                    account.credential_env,
                    if account.credential_available { "ready" } else { "missing" }
                );
            }
        }
    }
    Ok(())
}

pub fn social_adb_status(path: &Path, format: &Format) -> Result<()> {
    let status = adb_social::status(path)?;
    match format {
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(&status)?),
        Format::Human => {
            println!(
                "ADB {} · {} device(s) · {} allowlisted target(s)",
                if status.adb_available {
                    "ready"
                } else {
                    "missing"
                },
                status.devices.len(),
                status.targets.len()
            );
            for device in status.devices {
                println!(
                    "  {} [{} · {}] {}",
                    device.serial, device.state, device.transport, device.model
                );
            }
            for target in status.targets {
                println!("  {} → {}", target.id, target.package);
            }
        }
    }
    Ok(())
}

fn print_wifi_connection(connection: &adb_social::WifiConnection, format: &Format) -> Result<()> {
    match format {
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(connection)?),
        Format::Human => println!(
            "{} {} · {}",
            connection.action, connection.endpoint, connection.message
        ),
    }
    Ok(())
}

pub fn social_adb_wifi_pair(
    pair_endpoint: &str,
    connect_endpoint: &str,
    code: Option<&str>,
    code_env: &str,
    format: &Format,
) -> Result<()> {
    let progress = crate::output::ProgressGuard::start("Pairing Android device");
    if code.is_some() {
        eprintln!(
            "warning: --code leaves the pairing code in shell history and process listings; \
             set {code_env} instead and drop --code"
        );
    }
    let pairing_code = code
        .map(str::to_string)
        .or_else(|| std::env::var(code_env).ok())
        .ok_or_else(|| {
            BrandiError::Invalid(format!(
                "pairing code missing; set {code_env} or pass --code"
            ))
        })?;
    let connection = adb_social::pair_wifi(pair_endpoint, &pairing_code, connect_endpoint)?;
    progress.finish();
    print_wifi_connection(&connection, format)
}

pub fn social_adb_wifi_connect(endpoint: &str, format: &Format) -> Result<()> {
    let progress = crate::output::ProgressGuard::start("Connecting Android device");
    let connection = adb_social::connect_wifi(endpoint)?;
    progress.finish();
    print_wifi_connection(&connection, format)
}

pub fn social_adb_wifi_disconnect(endpoint: &str, format: &Format) -> Result<()> {
    let progress = crate::output::ProgressGuard::start("Disconnecting Android device");
    let connection = adb_social::disconnect_wifi(endpoint)?;
    progress.finish();
    print_wifi_connection(&connection, format)
}

pub fn social_adb_wifi_bridge(
    device: &str,
    host: Option<&str>,
    port: u16,
    format: &Format,
) -> Result<()> {
    let progress = crate::output::ProgressGuard::start("Bridging Android device to Wi-Fi");
    let connection = adb_social::bridge_usb_to_wifi(device, host, port)?;
    progress.finish();
    print_wifi_connection(&connection, format)
}

pub fn social_adb_stage(
    path: &Path,
    id: &str,
    target: &str,
    device: Option<&str>,
    format: &Format,
) -> Result<()> {
    let progress = crate::output::ProgressGuard::start("Staging approved content");
    let identity =
        automation::authorize_local(path, automation::AuthorizationRole::DeviceOperator)?;
    let delivery = adb_social::stage(path, id, target, device, &identity)?;
    progress.finish();
    match format {
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(&delivery)?),
        Format::Human => {
            println!(
                "staged {} unchanged in {} on {}",
                delivery.draft_id, delivery.target, delivery.device
            );
            // This is the human review checkpoint before a real publish;
            // show what's actually queued, not just the ids around it.
            println!("content: {}", delivery.content);
        }
    }
    Ok(())
}

pub fn social_adb_publish(
    path: &Path,
    id: &str,
    target: &str,
    device: Option<&str>,
    confirm: &str,
    format: &Format,
) -> Result<()> {
    let identity =
        automation::authorize_local(path, automation::AuthorizationRole::DeviceOperator)?;
    automation::authorize_local(path, automation::AuthorizationRole::Publisher)?;
    let progress = crate::output::ProgressGuard::start("Publishing approved content");
    let delivery = adb_social::publish(path, id, target, device, confirm, &identity)?;
    progress.finish();
    match format {
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(&delivery)?),
        Format::Human => {
            println!(
                "published exact approved revision {} through {} on {}",
                delivery.draft_id, delivery.target, delivery.device
            );
            println!("content: {}", delivery.content);
        }
    }
    Ok(())
}

/// `brandi daemon run`: run the watch daemon in the foreground.
pub fn daemon_run(path: &Path) -> Result<()> {
    if crate::output::context().format == Format::Json {
        return Err(BrandiError::Invalid(
            "foreground daemon output is unbounded; use --format jsonl".into(),
        ));
    }
    crate::output::event(&crate::output::OutputEvent::CommandStarted {
        command: "daemon run".into(),
        sequence: 0,
    })?;
    daemon::run(path)?;
    crate::output::event(&crate::output::OutputEvent::CommandFinished {
        status: "stopped".into(),
        summary: "daemon stopped".into(),
        sequence: 1,
    })?;
    Ok(())
}

/// `brandi daemon start`: start the watch daemon in the background.
pub fn daemon_start(path: &Path) -> Result<()> {
    let progress = crate::output::ProgressGuard::start("Starting Brandi daemon");
    let result = daemon::start(path);
    progress.finish();
    result
}

/// `brandi daemon stop`: stop the background watch daemon.
pub fn daemon_stop(path: &Path) -> Result<()> {
    let progress = crate::output::ProgressGuard::start("Stopping Brandi daemon");
    let result = daemon::stop(path);
    progress.finish();
    result
}

/// `brandi daemon status`: print daemon status.
pub fn daemon_status(path: &Path) -> Result<()> {
    daemon::status(path)
}

pub fn tape_generate(
    path: &Path,
    goal: String,
    preset: String,
    slug: String,
    output: Option<&Path>,
    force: bool,
    format: &Format,
) -> Result<()> {
    let progress = crate::output::ProgressGuard::start("Generating terminal demonstration");
    let output_media = output
        .map(|p| p.with_extension("gif"))
        .unwrap_or_else(|| PathBuf::from(format!("demos/{slug}.gif")));
    let draft = tape::generate(
        path,
        tape::TapeRequest {
            goal,
            preset,
            slug,
            output: output_media.to_string_lossy().into_owned(),
        },
    )?;
    progress.finish();
    if let Some(destination) = output {
        let report = tape::save_validated(path, &draft, destination, force)?;
        if !report.valid {
            return Err(BrandiError::Invalid(format!(
                "generated tape failed validation: {}",
                serde_json::to_string(&report)?
            )));
        }
    }
    match format {
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(&draft)?),
        Format::Human => println!(
            "{}\nProvider: {}\n\n{}",
            draft.plan.title, draft.provider, draft.tape
        ),
    }
    Ok(())
}

pub fn tape_validate(path: &Path, file: &Path, format: &Format) -> Result<i32> {
    let report = tape::validate_file(path, file)?;
    match format {
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(&report)?),
        Format::Human => {
            println!("Tape {}", if report.valid { "valid" } else { "invalid" });
            for issue in &report.issues {
                println!("  {:?} [{}] {}", issue.level, issue.code, issue.message);
            }
        }
    }
    Ok(if report.valid { 0 } else { 1 })
}

pub fn tape_render(
    path: &Path,
    file: &Path,
    confirmation: Option<&str>,
    format: &Format,
) -> Result<()> {
    let preview = tape::preview(path, file)?;
    if let Some(confirmation) = confirmation {
        let progress = crate::output::ProgressGuard::start("Rendering terminal demonstration");
        tape::render(path, file, confirmation)?;
        progress.finish();
        println!("Rendered {}", preview.output.display());
        return Ok(());
    }
    match format {
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(&preview)?),
        Format::Human => {
            println!("Tape execution preview");
            println!("  tape: {}", preview.tape.display());
            println!("  output: {}", preview.output.display());
            println!("  sandbox: {}", preview.sandbox);
            for command in &preview.commands {
                println!("  command: {command}");
            }
            println!("  confirm: {}", preview.confirmation);
        }
    }
    Ok(())
}

pub fn promotion_plan(path: &Path, format: &Format) -> Result<()> {
    let plan = automation::build_plan(path)?;
    match format {
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(&plan)?),
        Format::Human => println!(
            "Promotion plan {}\nObjective: {}\nChannels: {}",
            plan.id,
            plan.objective,
            plan.items
                .iter()
                .map(|i| i.channel.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
    Ok(())
}

pub fn promotion_stats(path: &Path, format: &Format) -> Result<()> {
    let config = automation::PromotionConfig::load(path)?;
    let stats = automation::collect_metrics(path, &config)?;
    match format {
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(&stats)?),
        Format::Human => println!(
            "Git {} · {} commits · {} contributors · {} tags",
            stats.head, stats.commits_30d, stats.contributors_30d, stats.tags
        ),
    }
    Ok(())
}

pub fn promotion_sync(path: &Path) -> Result<()> {
    let progress = crate::output::ProgressGuard::start("Synchronising Padagonia records");
    let synced = automation::sync_padagonia(path)?;
    progress.finish();
    match crate::output::context().format {
        Format::Human => println!("synced {synced} Padagonia records"),
        Format::Json | Format::Jsonl => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "brandi-padagonia-sync-v1",
                "synced": synced
            }))?
        ),
    }
    Ok(())
}

pub fn promotion_milestones(path: &Path, format: &Format) -> Result<()> {
    let progress = crate::output::ProgressGuard::start("Ingesting release milestones");
    let drafts = automation::ingest_kaptaind_milestones(path)?;
    progress.finish();
    match format {
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(&drafts)?),
        Format::Human => println!(
            "ingested {} new Kaptaind milestone draft{}",
            drafts.len(),
            if drafts.len() == 1 { "" } else { "s" }
        ),
    }
    Ok(())
}

pub fn promotion_queue(path: &Path, format: &Format) -> Result<()> {
    let queue = automation::list_queue(path)?;
    match format {
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(&queue)?),
        Format::Human => {
            let entries: Vec<String> = queue
                .iter()
                .map(|draft| {
                    format!(
                        "{} [{:?}] {}\n  revision: {}\n  destination: {}\n  approval expires: {}\n  content: {}",
                        draft.id,
                        draft.status,
                        draft.channel,
                        draft.revision_hash,
                        draft.approved_destination.as_deref().unwrap_or("unbound"),
                        draft
                            .approval_expires_at
                            .map(|value| value.to_string())
                            .as_deref()
                            .unwrap_or("not approved"),
                        draft.content
                    )
                })
                .collect();
            println!("{} queued", entries.len());
            crate::output::emit_list(&entries, crate::output::DEFAULT_LIST_CAP)?;
        }
    }
    Ok(())
}

pub fn promotion_decide(
    path: &Path,
    id: &str,
    approve: bool,
    confirmation: &str,
    destination: Option<&str>,
    reason: Option<String>,
) -> Result<()> {
    let identity = automation::authorize_local(path, automation::AuthorizationRole::Approver)?;
    let draft = automation::decide(
        path,
        id,
        approve,
        &identity,
        confirmation,
        destination,
        reason,
    )?;
    match crate::output::context().format {
        Format::Human => println!("{} is now {:?}", draft.id, draft.status),
        Format::Json | Format::Jsonl => println!("{}", serde_json::to_string_pretty(&draft)?),
    }
    Ok(())
}

pub fn telegram_run(config: Option<&Path>) -> Result<()> {
    if crate::output::context().format == Format::Json {
        return Err(BrandiError::Invalid(
            "Telegram supervisor output is unbounded; use --format jsonl".into(),
        ));
    }
    crate::output::event(&crate::output::OutputEvent::CommandStarted {
        command: "telegram run".into(),
        sequence: 0,
    })?;
    automation::run_telegram_supervisor(&telegram_config_path(config))?;
    crate::output::event(&crate::output::OutputEvent::CommandFinished {
        status: "stopped".into(),
        summary: "Telegram supervisor stopped".into(),
        sequence: 1,
    })?;
    Ok(())
}

pub fn telegram_status(config: Option<&Path>) -> Result<()> {
    let config = telegram_config_path(config);
    let parsed: automation::TelegramConfig =
        serde_yaml::from_str(&std::fs::read_to_string(&config)?)?;
    automation::validate_telegram_config(&parsed)?;
    match crate::output::context().format {
        Format::Human => println!(
            "telegram {} mode · {} project routes ready",
            parsed.mode,
            parsed.projects.len()
        ),
        Format::Json | Format::Jsonl => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "brandi-telegram-status-v1",
                "mode": parsed.mode,
                "project_routes": parsed.projects.len(),
                "ready": true
            }))?
        ),
    }
    Ok(())
}

fn telegram_config_path(config: Option<&Path>) -> PathBuf {
    config.map(Path::to_path_buf).unwrap_or_else(|| {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            PathBuf::from(xdg).join("brandi/telegram.yaml")
        } else if let Some(home) = std::env::var_os("HOME") {
            PathBuf::from(home).join(".config/brandi/telegram.yaml")
        } else {
            PathBuf::from(".config/brandi/telegram.yaml")
        }
    })
}
