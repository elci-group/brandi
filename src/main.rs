//! brandi CLI entrypoint.

use brandi::cli::{
    AdbSocialCommands, AdbWifiCommands, AssetsCommands, BriefCommands, Cli, Commands,
    DaemonCommands, GuidelinesCommands, PromotionCommands, SocialAccountCommands, SocialCommands,
    TapeCommands, TelegramCommands,
};
use brandi::commands;
use brandi::error::Result;
use clap::{CommandFactory, FromArgMatches};

fn main() {
    let cli = parse_cli();
    brandi::output::install(brandi::output::OutputContext::resolve(&cli.output));
    match run(cli) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            // No "Error:"/"ERROR:" prefix: brandi's own voice.yaml forbids
            // exactly that for the same reason it flags it in every other
            // project — the nonzero exit code already signals failure.
            let _ = brandi::output::emit_failure(&e);
            eprintln!(
                "{}\n{}\ncode: {} · https://docs.elci.group/brandi/errors/{}",
                brandi::output::styled(e.to_string(), brandi::output::Severity::Error),
                e.action(),
                e.code(),
                e.code()
            );
            std::process::exit(2);
        }
    }
}

/// Pre-resolve the two presentation controls that affect Clap's own generated
/// help and parse diagnostics. Full policy resolution follows normal parsing.
fn parse_cli() -> Cli {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let option = |name: &str| -> Option<String> {
        args.iter().enumerate().find_map(|(index, value)| {
            let text = value.to_string_lossy();
            if let Some(value) = text.strip_prefix(&format!("--{name}=")) {
                Some(value.to_string())
            } else if text == format!("--{name}") {
                args.get(index + 1)
                    .map(|next| next.to_string_lossy().into_owned())
            } else {
                None
            }
        })
    };
    let color = match option("color").as_deref() {
        Some("always") => clap::ColorChoice::Always,
        Some("never") => clap::ColorChoice::Never,
        _ if std::env::var_os("NO_COLOR").is_some() => clap::ColorChoice::Never,
        _ => clap::ColorChoice::Auto,
    };
    let width = option("width").and_then(|value| value.parse::<usize>().ok());
    let mut command = Cli::command().color(color);
    if let Some(width) = width {
        command = command.term_width(width);
    }
    let matches = command.get_matches_from(args);
    Cli::from_arg_matches(&matches).expect("Clap matches must reconstruct Cli")
}

/// Dispatch a parsed command line. Returns the process exit code.
fn run(cli: Cli) -> Result<i32> {
    let format = cli.output.format;
    let code = match cli.command {
        Commands::Direct {
            path,
            portfolio,
            section,
            runs,
            strategy,
            routes,
            reviewer,
            max_concurrency,
            max_cost_usd,
            sources,
            preflight_only,
            kaptaind,
            confirm_postflight,
        } => commands::direct_run(
            path.as_deref(),
            portfolio,
            &brandi::direct::DirectOptions {
                section,
                runs,
                strategy,
                routes,
                reviewer,
                max_concurrency,
                max_cost_usd,
                sources,
                preflight_only,
                kaptaind,
                confirm_postflight,
            },
            &format,
        )?,
        Commands::Reshoot { path, portfolio } => commands::creative_run(
            brandi::creative::CreativeMode::Reshoot,
            path.as_deref(),
            portfolio,
            &format,
        )?,
        Commands::Init { path } => {
            commands::init(&path)?;
            0
        }
        Commands::Brief { path, command } => match command {
            Some(BriefCommands::Validate { path }) => {
                commands::brief_validate(&path)?;
                0
            }
            None => {
                commands::brief_show(path.as_deref(), &format)?;
                0
            }
        },
        Commands::Guidelines { path, command } => match command {
            Some(GuidelinesCommands::Validate { path }) => {
                commands::guidelines_validate(&path)?;
                0
            }
            None => {
                commands::guidelines_show(path.as_deref(), &format)?;
                0
            }
        },
        Commands::Scan {
            path,
            if_uninitialized,
        } => {
            commands::scan(path.as_deref(), if_uninitialized, &format)?;
            0
        }
        Commands::Propose { path, budget } => {
            commands::propose(&path, budget)?;
            0
        }
        Commands::Check { file, path } => {
            commands::check(&path, &file)?;
            0
        }
        Commands::Lint {
            path,
            fail_under,
            strict,
            baseline,
        } => commands::lint(&path, &format, fail_under, strict, baseline.as_deref())?,
        Commands::Evaluate {
            corpus,
            fail_under_precision,
            fail_under_recall,
        } => commands::evaluate(&corpus, fail_under_precision, fail_under_recall, &format)?,
        Commands::Assets { command } => match command {
            AssetsCommands::Check { images, kind, path } => {
                commands::assets_check(&path, &images, &kind)?;
                0
            }
            AssetsCommands::List { path } => {
                commands::assets_list(&path)?;
                0
            }
            AssetsCommands::Audit { path } => {
                commands::assets_audit(&path, &format)?;
                0
            }
        },
        Commands::Social { command, path } => match command {
            None => {
                commands::social_tui(&path)?;
                0
            }
            Some(SocialCommands::Graph { path }) => {
                commands::social_graph(&path)?;
                0
            }
            Some(SocialCommands::Plan { segment, path }) => {
                commands::social_plan(&path, segment.as_deref())?;
                0
            }
            Some(SocialCommands::Accounts { command }) => match command {
                SocialAccountCommands::List { path } => {
                    commands::social_accounts_list(&path, &format)?;
                    0
                }
                SocialAccountCommands::Connect {
                    provider,
                    handle,
                    display_name,
                    profile_url,
                    bio,
                    credential_env,
                    path,
                } => {
                    commands::social_accounts_connect(
                        &path,
                        brandi::social::ConnectSocialAccount {
                            provider,
                            handle,
                            display_name,
                            profile_url,
                            bio,
                            credential_env,
                        },
                        &format,
                    )?;
                    0
                }
                SocialAccountCommands::Disconnect { id, confirm, path } => {
                    commands::social_accounts_disconnect(&path, &id, &confirm, &format)?;
                    0
                }
            },
            Some(SocialCommands::Adb { command }) => match command {
                AdbSocialCommands::Status { path } => {
                    commands::social_adb_status(&path, &format)?;
                    0
                }
                AdbSocialCommands::Stage {
                    id,
                    target,
                    device,
                    path,
                } => {
                    commands::social_adb_stage(&path, &id, &target, device.as_deref(), &format)?;
                    0
                }
                AdbSocialCommands::Publish {
                    id,
                    target,
                    device,
                    confirm,
                    path,
                } => {
                    commands::social_adb_publish(
                        &path,
                        &id,
                        &target,
                        device.as_deref(),
                        &confirm,
                        &format,
                    )?;
                    0
                }
                AdbSocialCommands::Wifi { command } => match command {
                    AdbWifiCommands::Pair {
                        pair_endpoint,
                        connect_endpoint,
                        code,
                        code_env,
                    } => {
                        commands::social_adb_wifi_pair(
                            &pair_endpoint,
                            &connect_endpoint,
                            code.as_deref(),
                            &code_env,
                            &format,
                        )?;
                        0
                    }
                    AdbWifiCommands::Connect { endpoint } => {
                        commands::social_adb_wifi_connect(&endpoint, &format)?;
                        0
                    }
                    AdbWifiCommands::Disconnect { endpoint } => {
                        commands::social_adb_wifi_disconnect(&endpoint, &format)?;
                        0
                    }
                    AdbWifiCommands::Bridge { device, host, port } => {
                        commands::social_adb_wifi_bridge(&device, host.as_deref(), port, &format)?;
                        0
                    }
                },
            },
        },
        Commands::Daemon { command } => match command {
            DaemonCommands::Run { path } => {
                commands::daemon_run(&path)?;
                0
            }
            DaemonCommands::Start { path } => {
                commands::daemon_start(&path)?;
                0
            }
            DaemonCommands::Stop { path } => {
                commands::daemon_stop(&path)?;
                0
            }
            DaemonCommands::Status { path } => {
                commands::daemon_status(&path)?;
                0
            }
        },
        Commands::Tape { command } => match command {
            TapeCommands::Generate {
                path,
                goal,
                preset,
                slug,
                output,
                force,
            } => {
                commands::tape_generate(
                    &path,
                    goal,
                    preset,
                    slug,
                    output.as_deref(),
                    force,
                    &format,
                )?;
                0
            }
            TapeCommands::Validate { file, path } => {
                commands::tape_validate(&path, &file, &format)?
            }
            TapeCommands::Render {
                file,
                path,
                confirm,
            } => {
                commands::tape_render(&path, &file, confirm.as_deref(), &format)?;
                0
            }
        },
        Commands::Promotion { command } => match command {
            PromotionCommands::Plan { path } => {
                commands::promotion_plan(&path, &format)?;
                0
            }
            PromotionCommands::Stats { path } => {
                commands::promotion_stats(&path, &format)?;
                0
            }
            PromotionCommands::Sync { path } => {
                commands::promotion_sync(&path)?;
                0
            }
            PromotionCommands::Milestones { path } => {
                commands::promotion_milestones(&path, &format)?;
                0
            }
            PromotionCommands::Queue { path } => {
                commands::promotion_queue(&path, &format)?;
                0
            }
            PromotionCommands::Approve {
                id,
                path,
                confirm,
                destination,
            } => {
                commands::promotion_decide(&path, &id, true, &confirm, Some(&destination), None)?;
                0
            }
            PromotionCommands::Reject {
                id,
                path,
                confirm,
                reason,
            } => {
                commands::promotion_decide(&path, &id, false, &confirm, None, reason)?;
                0
            }
        },
        Commands::Telegram { command } => match command {
            TelegramCommands::Run { config } => {
                commands::telegram_run(config.as_deref())?;
                0
            }
            TelegramCommands::Status { config } => {
                commands::telegram_status(config.as_deref())?;
                0
            }
        },
    };
    Ok(code)
}
