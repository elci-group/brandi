//! Command-line interface definition (clap derive).

use crate::output::{AnimationChoice, ColorChoice, Fidelity, UnicodeChoice};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::fmt;
use std::path::PathBuf;

fn cli_styles() -> clap::builder::Styles {
    use clap::builder::styling::{AnsiColor, Effects, Styles};
    Styles::styled()
        .header(AnsiColor::Cyan.on_default() | Effects::BOLD)
        .usage(AnsiColor::Cyan.on_default() | Effects::BOLD)
        .literal(AnsiColor::Green.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::Yellow.on_default())
        .error(AnsiColor::Red.on_default() | Effects::BOLD)
        .valid(AnsiColor::Green.on_default())
        .invalid(AnsiColor::Red.on_default())
}

/// brandi — brand coherence intelligence layer.
#[derive(Parser, Debug)]
#[command(
    name = "brandi",
    version,
    about = "Brand coherence intelligence layer: lint user-facing surfaces against your brand brief and guidelines",
    styles = cli_styles()
)]
pub struct Cli {
    #[command(flatten)]
    pub output: OutputArgs,
    #[command(subcommand)]
    pub command: Commands,
}

/// Presentation controls shared by every command and nested command.
#[derive(Args, Clone, Debug)]
pub struct OutputArgs {
    /// Output encoding. JSON is always static and decoration-free.
    #[arg(long, global = true, value_enum, default_value_t = Format::Human)]
    pub format: Format,
    /// Colour policy for human output.
    #[arg(long, global = true, value_enum, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,
    /// Animation policy for human output.
    #[arg(long, global = true, value_enum, default_value_t = AnimationChoice::Auto)]
    pub animation: AnimationChoice,
    /// Requested presentation fidelity.
    #[arg(long, global = true, value_enum, default_value_t = Fidelity::Auto)]
    pub fidelity: Fidelity,
    /// Unicode glyph policy.
    #[arg(long, global = true, value_enum, default_value_t = UnicodeChoice::Auto)]
    pub unicode: UnicodeChoice,
    /// Override terminal width for deterministic output and support bundles.
    #[arg(long, global = true, value_parser = clap::value_parser!(u16).range(20..=1000))]
    pub width: Option<u16>,
    /// Suppress normal human stdout; errors and warnings remain on stderr.
    #[arg(long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,
    /// Emit renderer and phase diagnostics on stderr.
    #[arg(long, global = true)]
    pub verbose: bool,
}

/// What `scan` should do when its target has no `.brandi/` configuration.
#[derive(ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[value(rename_all = "kebab-case")]
pub enum UninitializedAction {
    #[default]
    Prompt,
    Init,
    Skip,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Plan, generate, independently review, and evidence brand assets
    Direct {
        /// Project root, or portfolio root when --portfolio is set
        #[arg(long)]
        path: Option<PathBuf>,
        /// Run across every Brandi-compliant project (defaults to HOME)
        #[arg(long)]
        portfolio: bool,
        /// Brief subsection (dot path, JSON pointer, or "all")
        #[arg(long, default_value = "identity.product")]
        section: String,
        /// Number of generation actions per objective (1..64)
        #[arg(long)]
        runs: Option<usize>,
        /// Breadth/refinement scheduling strategy
        #[arg(long, value_enum)]
        strategy: Option<crate::direct::Strategy>,
        /// Preferred generation route, repeatable as provider/model
        #[arg(long = "route")]
        routes: Vec<String>,
        /// Preferred batch-review route as provider/model
        #[arg(long)]
        reviewer: Option<String>,
        /// Maximum simultaneous provider calls (1..8)
        #[arg(long)]
        max_concurrency: Option<usize>,
        /// Hard estimated provider-cost ceiling in USD
        #[arg(long)]
        max_cost_usd: Option<f64>,
        /// Source image to include as evidence (repeatable, project-relative)
        #[arg(long = "source")]
        sources: Vec<PathBuf>,
        /// Write and present the immutable plan without provider calls
        #[arg(long)]
        preflight_only: bool,
        /// Optional Kaptaind analysis/Aim-of-Change/push post-flight
        #[arg(long, value_enum, default_value = "none")]
        kaptaind: crate::direct::KaptaindMode,
        /// Exact token printed by pre-flight for commit/push modes
        #[arg(long)]
        confirm_postflight: Option<String>,
    },
    /// Revise all discovered visual and non-code textual brand assets
    Reshoot {
        /// Project root, or portfolio root when --portfolio is set
        #[arg(long)]
        path: Option<PathBuf>,
        /// Run across every Brandi-compliant project (defaults to HOME)
        #[arg(long)]
        portfolio: bool,
    },
    /// Scaffold a `.brandi/` brief and guidelines directory
    Init {
        /// Project root
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Inspect the brand brief: who the product is and who it's for
    Brief {
        /// Project path; defaults to the nearest Brandi project at or above cwd
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
        #[command(subcommand)]
        command: Option<BriefCommands>,
    },
    /// Inspect the brand guidelines: how it sounds, looks, and what
    /// language it forbids
    Guidelines {
        /// Project path; defaults to the nearest Brandi project at or above cwd
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
        #[command(subcommand)]
        command: Option<GuidelinesCommands>,
    },
    /// Scan the project and list discovered surfaces
    Scan {
        /// Top-level project root; defaults to the nearest project at or above cwd
        #[arg(long)]
        path: Option<PathBuf>,
        /// Missing `.brandi/` policy; prompt requires an interactive terminal
        #[arg(long, value_enum, default_value_t = UninitializedAction::Prompt)]
        if_uninitialized: UninitializedAction,
    },
    /// Propose aesthetic revisions for stylisable brand surfaces
    Propose {
        /// Project root
        #[arg(long, default_value = ".")]
        path: PathBuf,
        /// Maximum number of generated revisions to spend
        #[arg(long, default_value_t = 12)]
        budget: usize,
    },
    /// Lint a single file against the brief and guidelines
    Check {
        /// File to check
        file: PathBuf,
        /// Project root
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Lint the whole project against the brief and guidelines
    Lint {
        /// Project root
        #[arg(long, default_value = ".")]
        path: PathBuf,
        /// Fail (exit 1) when the overall score is below N
        #[arg(long, default_value_t = 0)]
        fail_under: u32,
        /// Fail (exit 1) when any error-severity finding exists
        #[arg(long)]
        strict: bool,
        /// Previous JSON report used only to calculate new/resolved findings
        #[arg(long)]
        baseline: Option<PathBuf>,
    },
    /// Evaluate extractor precision and recall against a versioned corpus
    Evaluate {
        #[arg(long, default_value = "evaluation/v1/corpus.json")]
        corpus: PathBuf,
        #[arg(long, default_value_t = 0.95)]
        fail_under_precision: f64,
        #[arg(long, default_value_t = 0.90)]
        fail_under_recall: f64,
    },
    /// Check and list brand image assets
    Assets {
        #[command(subcommand)]
        command: AssetsCommands,
    },
    /// Interactive account workspace, narrative planning, and delivery
    Social {
        #[command(subcommand)]
        command: Option<SocialCommands>,
        /// Project root used by the default interactive account workspace
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Manage the brandi watch daemon
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
    },
    /// Generate, validate, and explicitly render VHS tapes
    Tape {
        #[command(subcommand)]
        command: TapeCommands,
    },
    /// Build promotion plans, metrics, and approval queues
    Promotion {
        #[command(subcommand)]
        command: PromotionCommands,
    },
    /// Run the Telegram hub/fleet supervisor
    Telegram {
        #[command(subcommand)]
        command: TelegramCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum TapeCommands {
    /// Generate a structured tape draft; writes only when --output is supplied
    Generate {
        #[arg(long, default_value = ".")]
        path: PathBuf,
        #[arg(long, default_value = "Show how Brandi prevents identity drift")]
        goal: String,
        #[arg(long, default_value = "product-tour")]
        preset: String,
        #[arg(long, default_value = "brandi-demo")]
        slug: String,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        force: bool,
    },
    /// Run deterministic safety checks and native `vhs validate`
    Validate {
        file: PathBuf,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Preview a validated tape; execute only with its exact confirmation token
    Render {
        file: PathBuf,
        #[arg(long, default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        confirm: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum PromotionCommands {
    /// Generate a dynamic promotion plan
    Plan {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Capture Git/GitHub metric snapshots
    Stats {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Replay the durable Padagonia outbox
    Sync {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Ingest Kaptaind release indexes into the approval queue
    Milestones {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// List the current approval queue
    Queue {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Approve an exact queued revision
    Approve {
        id: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
        /// Exact draft ID or revision hash shown by `promotion queue`
        #[arg(long)]
        confirm: String,
        /// Bound publication destination, e.g. telegram:-100123 or adb:x
        #[arg(long)]
        destination: String,
    },
    /// Reject a queued revision
    Reject {
        id: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
        /// Exact draft ID or revision hash shown by `promotion queue`
        #[arg(long)]
        confirm: String,
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum TelegramCommands {
    /// Run the hub or fleet supervisor in the foreground
    Run {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Validate configuration and report worker readiness
    Status {
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
pub enum BriefCommands {
    /// Validate brief files and report problems
    Validate {
        /// Project root
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub enum GuidelinesCommands {
    /// Validate guidelines files and report problems
    Validate {
        /// Project root
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub enum AssetsCommands {
    /// Check image assets against the guidelines' asset spec
    Check {
        /// Image files to check
        #[arg(required = true)]
        images: Vec<PathBuf>,
        /// Which asset spec to check against
        #[arg(long, value_enum, default_value_t = AssetKindArg::Auto)]
        kind: AssetKindArg,
        /// Project root
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// List discovered brand assets
    List {
        /// Project root
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Project-wide asset coherence and utilisation audit: palette
    /// adherence, file-size budget, duplicate/near-duplicate detection,
    /// icon-set consistency, and reference counting
    Audit {
        /// Project root
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub enum SocialCommands {
    /// Render the narrative graph (capabilities → audiences → formats)
    Graph {
        /// Project root
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Render a content plan, optionally for one audience segment
    Plan {
        /// Audience segment name to plan for
        #[arg(long)]
        segment: Option<String>,
        /// Project root
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// List, connect, and disconnect project-linked social accounts
    Accounts {
        #[command(subcommand)]
        command: SocialAccountCommands,
    },
    /// Orchestrate approval-gated social delivery through Android Debug Bridge
    Adb {
        #[command(subcommand)]
        command: AdbSocialCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum SocialAccountCommands {
    /// List linked accounts and their source/surface and credential state
    List {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Link a provider-issued credential without storing its value
    Connect {
        #[arg(long)]
        provider: String,
        #[arg(long)]
        handle: String,
        #[arg(long)]
        display_name: String,
        #[arg(long)]
        profile_url: String,
        #[arg(long, default_value = "")]
        bio: String,
        /// Environment variable containing the provider-issued credential
        #[arg(long)]
        credential_env: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Remove a project link; requires the exact account id as confirmation
    Disconnect {
        id: String,
        #[arg(long)]
        confirm: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub enum AdbSocialCommands {
    /// List configured targets and connected devices without changing either
    Status {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Open an approved draft in an allowlisted Android app composer
    Stage {
        id: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        device: Option<String>,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Stage and tap the configured publish control after exact confirmation
    Publish {
        id: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        device: Option<String>,
        #[arg(long)]
        confirm: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Pair, connect, disconnect, or bridge Android devices over Wi-Fi
    Wifi {
        #[command(subcommand)]
        command: AdbWifiCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum AdbWifiCommands {
    /// Pair with Android Wireless debugging and connect its debug endpoint
    Pair {
        #[arg(long)]
        pair_endpoint: String,
        #[arg(long)]
        connect_endpoint: String,
        /// Pairing code; prefer the environment variable for shell-history safety
        #[arg(long)]
        code: Option<String>,
        #[arg(long, default_value = "BRANDI_ADB_PAIR_CODE")]
        code_env: String,
    },
    /// Connect an already paired Android Wireless debugging endpoint
    Connect { endpoint: String },
    /// Disconnect one explicit Android Wireless debugging endpoint
    Disconnect { endpoint: String },
    /// Switch one online USB device to TCP mode and connect it over Wi-Fi
    Bridge {
        #[arg(long)]
        device: String,
        #[arg(long)]
        host: Option<String>,
        #[arg(long, default_value_t = 5555)]
        port: u16,
    },
}

#[derive(Subcommand, Debug)]
pub enum DaemonCommands {
    /// Run the daemon in the foreground
    Run {
        /// Project root
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Start the daemon in the background
    Start {
        /// Project root
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Stop the background daemon
    Stop {
        /// Project root
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Print daemon status
    Status {
        /// Project root
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

/// Output format for `brandi lint`.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Human,
    Json,
    Jsonl,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Format::Human => "human",
            Format::Json => "json",
            Format::Jsonl => "jsonl",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod creative_command_tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn direct_portfolio_defaults_its_root_later() {
        let cli = Cli::try_parse_from(["brandi", "direct", "--portfolio"]).unwrap();
        assert_eq!(cli.output.format, Format::Human);
        match cli.command {
            Commands::Direct {
                path, portfolio, ..
            } => {
                assert!(path.is_none());
                assert!(portfolio);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn reshoot_accepts_a_designated_portfolio_root() {
        let cli = Cli::try_parse_from([
            "brandi",
            "reshoot",
            "--portfolio",
            "--path",
            "/srv/projects",
            "--format",
            "json",
        ])
        .unwrap();
        assert_eq!(cli.output.format, Format::Json);
        match cli.command {
            Commands::Reshoot { path, portfolio } => {
                assert_eq!(path, Some(PathBuf::from("/srv/projects")));
                assert!(portfolio);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn brief_and_guidelines_default_to_display_with_optional_target_paths() {
        let brief = Cli::try_parse_from(["brandi", "brief"]).unwrap();
        match brief.command {
            Commands::Brief { path, command } => {
                assert!(path.is_none());
                assert!(command.is_none());
            }
            other => panic!("unexpected command: {other:?}"),
        }

        let guidelines = Cli::try_parse_from(["brandi", "guidelines", "/srv/project/src"]).unwrap();
        match guidelines.command {
            Commands::Guidelines { path, command } => {
                assert_eq!(path, Some(PathBuf::from("/srv/project/src")));
                assert!(command.is_none());
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn brief_and_guidelines_validate_subcommands_remain_available() {
        let brief =
            Cli::try_parse_from(["brandi", "brief", "validate", "--path", "/srv/project"]).unwrap();
        assert!(matches!(
            brief.command,
            Commands::Brief {
                path: None,
                command: Some(BriefCommands::Validate { path })
            } if path == PathBuf::from("/srv/project")
        ));

        let guidelines = Cli::try_parse_from(["brandi", "guidelines", "validate"]).unwrap();
        assert!(matches!(
            guidelines.command,
            Commands::Guidelines {
                path: None,
                command: Some(GuidelinesCommands::Validate { .. })
            }
        ));
    }

    #[test]
    fn every_leaf_inherits_the_global_presentation_flags() {
        fn visit(command: &clap::Command, leaves: &mut usize) {
            let children: Vec<_> = command
                .get_subcommands()
                .filter(|child| child.get_name() != "help")
                .collect();
            if children.is_empty() {
                *leaves += 1;
                let ids: std::collections::BTreeSet<_> = command
                    .get_arguments()
                    .map(|arg| arg.get_id().as_str())
                    .collect();
                for required in [
                    "format",
                    "color",
                    "animation",
                    "fidelity",
                    "unicode",
                    "width",
                    "quiet",
                    "verbose",
                ] {
                    assert!(
                        ids.contains(required),
                        "{} is missing global flag --{}",
                        command.get_name(),
                        required
                    );
                }
            } else {
                for child in children {
                    visit(child, leaves);
                }
            }
        }

        let mut command = Cli::command();
        command.build();
        let mut leaves = 0;
        visit(&command, &mut leaves);
        assert_eq!(leaves, 41);
    }

    #[test]
    fn social_without_a_subcommand_selects_the_tui() {
        let cli = Cli::try_parse_from(["brandi", "social", "--path", "/project"]).unwrap();
        match cli.command {
            Commands::Social { command, path } => {
                assert!(command.is_none());
                assert_eq!(path, PathBuf::from("/project"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}

/// `--kind` argument for `brandi assets check` (mapped to `assets::AssetKind`).
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
#[value(rename_all = "kebab-case")]
pub enum AssetKindArg {
    Auto,
    SocialCard,
    Thumbnail,
    Icon,
}

impl fmt::Display for AssetKindArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            AssetKindArg::Auto => "auto",
            AssetKindArg::SocialCard => "social-card",
            AssetKindArg::Thumbnail => "thumbnail",
            AssetKindArg::Icon => "icon",
        };
        f.write_str(s)
    }
}
