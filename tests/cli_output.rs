use serde_json::Value;
use std::process::{Command, Output};

fn brandi(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_brandi"))
        .args(args)
        .env("NO_COLOR", "1")
        .env("CI", "1")
        .output()
        .expect("run brandi")
}

fn brandi_in(args: &[&str], current_dir: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_brandi"))
        .args(args)
        .current_dir(current_dir)
        .env("NO_COLOR", "1")
        .env("CI", "1")
        .output()
        .expect("run brandi")
}

#[test]
fn root_group_and_leaf_help_are_static_and_plain_when_requested() {
    let paths: &[&[&str]] = &[
        &[],
        &["assets"],
        &["social", "adb", "wifi"],
        &["lint"],
        &["social", "adb", "publish"],
        &["promotion", "approve"],
        &["tape", "render"],
    ];
    for path in paths {
        let mut args = vec!["--color", "never", "--animation", "never"];
        args.extend_from_slice(path);
        args.push("--help");
        let output = brandi(&args);
        assert!(output.status.success(), "help failed for {path:?}");
        assert!(!output.stdout.contains(&0x1b), "ANSI leaked for {path:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("Usage:"));
    }
}

#[test]
fn help_honors_forced_color_before_normal_policy_installation() {
    let output = Command::new(env!("CARGO_BIN_EXE_brandi"))
        .args(["--color", "always", "lint", "--help"])
        .env_remove("NO_COLOR")
        .output()
        .expect("run brandi help");
    assert!(output.status.success());
    assert!(output.stdout.contains(&0x1b));
}

#[test]
fn json_and_jsonl_are_valid_and_decoration_free() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().to_str().unwrap();
    let json = brandi(&["--format", "json", "init", "--path", root]);
    assert!(
        json.status.success(),
        "{}",
        String::from_utf8_lossy(&json.stderr)
    );
    assert!(!json.stdout.contains(&0x1b));
    let value: Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["schema_version"], "brandi-init-v1");

    let jsonl = brandi(&["--format", "jsonl", "init", "--path", root]);
    assert!(jsonl.status.success());
    assert_eq!(
        jsonl
            .stdout
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .count(),
        1
    );
    let value: Value = serde_json::from_slice(jsonl.stdout.strip_suffix(b"\n").unwrap()).unwrap();
    assert_eq!(value["changed"], false);
}

#[test]
fn brief_defaults_to_the_project_associated_with_cwd() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    assert!(brandi(&["init", "--path", root.to_str().unwrap()])
        .status
        .success());
    let nested = root.join("src/deep");
    std::fs::create_dir_all(&nested).unwrap();

    let output = brandi_in(&["brief"], &nested);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let yaml: serde_yaml::Value = serde_yaml::from_slice(&output.stdout).unwrap();
    assert_eq!(yaml["identity"]["product"]["name"], "Brandi");
    assert!(yaml.get("audience").is_some());
}

#[test]
fn scan_defaults_to_nearest_project_root_and_rejects_explicit_nested_paths() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("project");
    let nested = root.join("src/deep");
    std::fs::create_dir_all(&nested).unwrap();
    assert!(brandi(&["init", "--path", root.to_str().unwrap()])
        .status
        .success());

    let implicit = brandi_in(&["--format", "json", "scan"], &nested);
    assert!(
        implicit.status.success(),
        "{}",
        String::from_utf8_lossy(&implicit.stderr)
    );
    let value: Value = serde_json::from_slice(&implicit.stdout).unwrap();
    assert_eq!(
        value["root"],
        root.canonicalize().unwrap().to_string_lossy().as_ref()
    );

    let explicit = brandi(&["scan", "--path", nested.to_str().unwrap()]);
    assert!(!explicit.status.success());
    assert!(String::from_utf8_lossy(&explicit.stderr).contains("top-level project root"));
}

#[test]
fn scan_can_initialize_or_skip_an_unconfigured_target() {
    let temporary = tempfile::tempdir().unwrap();
    let skipped = temporary.path().join("skipped");
    let initialized = temporary.path().join("initialized");
    std::fs::create_dir_all(&skipped).unwrap();
    std::fs::create_dir_all(&initialized).unwrap();
    std::fs::write(skipped.join("Cargo.toml"), "[package]\nname='skip-me'\n").unwrap();
    std::fs::write(
        initialized.join("Cargo.toml"),
        "[package]\nname='init-me'\n",
    )
    .unwrap();

    let unavailable_prompt = brandi(&["scan", "--path", skipped.to_str().unwrap()]);
    assert!(!unavailable_prompt.status.success());
    assert!(String::from_utf8_lossy(&unavailable_prompt.stderr).contains("--if-uninitialized init"));

    let skip = brandi(&[
        "--format",
        "json",
        "scan",
        "--path",
        skipped.to_str().unwrap(),
        "--if-uninitialized",
        "skip",
    ]);
    assert!(skip.status.success());
    let value: Value = serde_json::from_slice(&skip.stdout).unwrap();
    assert_eq!(value["status"], "skipped");
    assert!(!skipped.join(".brandi").exists());

    let init = brandi(&[
        "--format",
        "json",
        "scan",
        "--path",
        initialized.to_str().unwrap(),
        "--if-uninitialized",
        "init",
    ]);
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let value: Value = serde_json::from_slice(&init.stdout).unwrap();
    assert_eq!(
        value["root"],
        initialized
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
    assert!(initialized.join(".brandi/identity.yaml").is_file());
}

#[test]
fn guidelines_accepts_a_nested_target_path_and_emits_structured_json() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    assert!(brandi(&["init", "--path", root.to_str().unwrap()])
        .status
        .success());
    let target = root.join("docs/guide.md");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "# Guide\n").unwrap();

    let output = brandi(&["--format", "json", "guidelines", target.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], "brandi-guidelines-v1");
    assert_eq!(
        value["project"],
        root.canonicalize().unwrap().to_string_lossy().as_ref()
    );
    assert_eq!(value["guidelines"]["voice"]["traits"][0], "precise");
}

#[test]
fn propose_json_includes_telemetry_fields() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().to_str().unwrap();
    let init = brandi(&["init", "--path", root]);
    assert!(init.status.success());
    std::fs::write(
        temporary.path().join("README.md"),
        "# Acme\n\nThis awesome tool helps teams.\n",
    )
    .unwrap();
    let output = brandi(&[
        "--format", "json", "propose", "--path", root, "--budget", "4",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], "brandi-proposals-v3");
    assert!(value.get("surfaces_scanned").is_some());
    assert!(value.get("files_scanned").is_some());
    assert!(value.get("files_considered").is_some());
    assert!(value.get("findings").is_some());
    assert!(value["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .all(|proposal| {
            proposal.get("operation").is_some() && proposal.get("priority").is_some()
        }));
    assert_eq!(value["typography"]["class"], "opportunity");
    assert_eq!(value["typography"]["role"], "typography");
    assert_eq!(value["typography"]["family"], "Inter");
    assert!(value["typography"]["inline_css"]
        .as_str()
        .unwrap()
        .contains("font-family"));
}

#[test]
fn social_accounts_are_project_links_sources_and_scan_surfaces_without_stored_tokens() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().to_str().unwrap();
    assert!(brandi(&["init", "--path", root]).status.success());
    let output = Command::new(env!("CARGO_BIN_EXE_brandi"))
        .args([
            "--format",
            "json",
            "social",
            "accounts",
            "connect",
            "--provider",
            "mastodon",
            "--handle",
            "brandi",
            "--display-name",
            "Brandi",
            "--profile-url",
            "https://social.example/@brandi",
            "--bio",
            "Brand coherence intelligence",
            "--credential-env",
            "BRANDI_TEST_SOCIAL_TOKEN",
            "--path",
            root,
        ])
        .env("BRANDI_TEST_SOCIAL_TOKEN", "not-written-to-disk")
        .env("NO_COLOR", "1")
        .env("CI", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let registry: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(registry["accounts"][0]["source"]["content"], true);
    assert_eq!(registry["accounts"][0]["surface"]["profile"], true);
    assert_eq!(registry["accounts"][0]["credential_available"], true);
    let persisted =
        std::fs::read_to_string(temporary.path().join(".brandi/social-accounts.json")).unwrap();
    assert!(!persisted.contains("not-written-to-disk"));

    let scan = Command::new(env!("CARGO_BIN_EXE_brandi"))
        .args(["--format", "json", "scan", "--path", root])
        .env("BRANDI_TEST_SOCIAL_TOKEN", "available-for-this-session")
        .env("NO_COLOR", "1")
        .env("CI", "1")
        .output()
        .unwrap();
    assert!(
        scan.status.success(),
        "{}",
        String::from_utf8_lossy(&scan.stderr)
    );
    let report: Value = serde_json::from_slice(&scan.stdout).unwrap();
    assert!(report["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .any(|surface| surface["kind"] == "social"));
}

#[test]
fn machine_failure_is_one_valid_document_with_a_stable_code() {
    let temporary = tempfile::tempdir().unwrap();
    let output = brandi(&[
        "--format",
        "json",
        "lint",
        "--path",
        temporary.path().to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert!(!output.stdout.contains(&0x1b));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], "brandi-error-v1");
    assert!(value["code"].as_str().unwrap().starts_with("BRD-"));
}

#[test]
fn quiet_suppresses_success_stdout_but_not_failures() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().to_str().unwrap();
    let output = brandi(&["--quiet", "daemon", "status", "--path", root]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());

    let failure = brandi(&[
        "--quiet",
        "lint",
        "--path",
        temporary.path().to_str().unwrap(),
    ]);
    assert!(!failure.status.success());
    assert!(!failure.stderr.is_empty());
}

#[test]
fn forced_width_wraps_human_output() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().to_str().unwrap();
    let output = brandi(&["--width", "20", "daemon", "status", "--path", root]);
    assert!(output.status.success());
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        assert!(line.chars().count() <= 20, "line exceeded width: {line:?}");
    }
}

#[test]
fn rich_barbara_fidelity_falls_back_without_changing_command_success() {
    let temporary = tempfile::tempdir().unwrap();
    let output = brandi(&[
        "--fidelity",
        "print",
        "--verbose",
        "daemon",
        "status",
        "--path",
        temporary.path().to_str().unwrap(),
    ]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("using 3form"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("not running"));
}
