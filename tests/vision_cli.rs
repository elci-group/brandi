use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn critique_sends_image_and_context_without_credentials_in_arguments() {
    let dir = tempfile::tempdir().unwrap();
    brandi::brief::Brief::scaffold(dir.path()).unwrap();
    brandi::guidelines::Guidelines::scaffold(dir.path()).unwrap();
    fs::write(dir.path().join(".brandi/vision.yaml"), "endpoint: https://example.test/chat/completions\nmodel: test-vision\napi_key_env: BRANDI_TEST_KEY\n").unwrap();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([255, 45, 170, 255]))
        .save(dir.path().join("logo.png"))
        .unwrap();
    let bin = dir.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let curl = bin.join("curl");
    fs::write(&curl, r#"#!/usr/bin/python3
import sys, json
assert 'test-secret' not in ' '.join(sys.argv)
data = sys.stdin.read()
assert 'Authorization: Bearer test-secret' in data
assert 'data:image/png;base64,' in data
assert 'brief' in data and 'guidelines' in data and 'reference_count_hint' in data
print(json.dumps({'choices':[{'finish_reason':'stop','message':{'content':'Observed: strong silhouette. Revision: simplify fine detail for small sizes.'}}]}))
"#).unwrap();
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o755)).unwrap();
    let run = |image: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_brandi"));
        command.args(["assets", "critique"]);
        if let Some(image) = image {
            command.arg(image);
        }
        command
            .args(["--path", dir.path().to_str().unwrap(), "--format", "json"])
            .env("PATH", &bin)
            .env("BRANDI_TEST_KEY", "test-secret")
            .output()
            .unwrap()
    };
    let output = run(None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let review: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(review["path"], "logo.png");
    assert_eq!(review["model"], "test-vision");
    assert!(review["analysis"].as_str().unwrap().contains("silhouette"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("test-secret"));
    assert!(!run(Some("../outside.png")).status.success());
    let external = tempfile::NamedTempFile::new().unwrap();
    std::os::unix::fs::symlink(external.path(), dir.path().join("escape.png")).unwrap();
    assert!(!run(Some("escape.png")).status.success());
}
