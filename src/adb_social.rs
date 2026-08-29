//! Approval-gated Android social orchestration through the `adb` executable.

use crate::automation::{self, ContentDraft};
use crate::error::{BrandiError, Result};
use crate::process::{
    run_bounded, run_bounded_with_input, BoundedOutput, MAX_SUBPROCESS_OUTPUT_BYTES,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::IpAddr;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct AdbSocialConfig {
    pub default_device: String,
    pub targets: Vec<AdbTarget>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AdbTarget {
    pub id: String,
    pub package: String,
    pub mime_type: String,
    pub publish_selector: UiSelector,
    pub settle_ms: u64,
}

impl Default for AdbTarget {
    fn default() -> Self {
        Self {
            id: String::new(),
            package: String::new(),
            mime_type: "text/plain".into(),
            publish_selector: UiSelector::default(),
            settle_ms: 1_500,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct UiSelector {
    /// One of `resource_id`, `text`, or `content_desc`.
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdbDevice {
    pub serial: String,
    pub state: String,
    pub model: String,
    pub product: String,
    pub transport: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdbSocialStatus {
    pub adb_available: bool,
    pub devices: Vec<AdbDevice>,
    pub targets: Vec<AdbTarget>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdbDelivery {
    pub draft_id: String,
    pub target: String,
    pub package: String,
    pub device: String,
    pub action: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WifiConnection {
    pub action: String,
    pub endpoint: String,
    pub device: Option<String>,
    pub message: String,
}

#[derive(Debug, Serialize)]
struct AdbAuditEvent<'a> {
    timestamp: u64,
    draft_id: &'a str,
    target: &'a str,
    package: &'a str,
    device: &'a str,
    action: &'a str,
    result: &'a str,
    evidence: Vec<String>,
}

impl AdbSocialConfig {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(".brandi/social-adb.yaml");
        let config = if path.exists() {
            serde_yaml::from_str(&fs::read_to_string(path)?)?
        } else {
            Self::default()
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        let package = Regex::new(r"^[A-Za-z0-9_.]+$")
            .map_err(|error| BrandiError::Invalid(error.to_string()))?;
        let mut ids = HashSet::new();
        for target in &self.targets {
            if target.id.is_empty() || !ids.insert(target.id.clone()) {
                return Err(BrandiError::Invalid(
                    "ADB social target IDs must be non-empty and unique".into(),
                ));
            }
            if !package.is_match(&target.package) {
                return Err(BrandiError::Invalid(format!(
                    "ADB target {} has an invalid Android package",
                    target.id
                )));
            }
            if !matches!(
                target.publish_selector.kind.as_str(),
                "resource_id" | "text" | "content_desc"
            ) || target.publish_selector.value.is_empty()
            {
                return Err(BrandiError::Invalid(format!(
                    "ADB target {} needs a resource_id, text, or content_desc publish selector",
                    target.id
                )));
            }
            if target.settle_ms > 10_000 {
                return Err(BrandiError::Invalid(format!(
                    "ADB target {} settle_ms exceeds the 10 second safety bound",
                    target.id
                )));
            }
        }
        Ok(())
    }
}

pub fn status(root: &Path) -> Result<AdbSocialStatus> {
    let config = AdbSocialConfig::load(root)?;
    let devices = list_devices().unwrap_or_default();
    Ok(AdbSocialStatus {
        adb_available: program_exists("adb"),
        devices,
        targets: config.targets,
    })
}

pub fn list_devices() -> Result<Vec<AdbDevice>> {
    let output = adb_output(Command::new("adb").args(["devices", "-l"]))?;
    if !output.status.success() {
        return Err(command_error("adb devices", &output));
    }
    Ok(parse_devices(&String::from_utf8_lossy(&output.stdout)))
}

pub fn connect_wifi(endpoint: &str) -> Result<WifiConnection> {
    validate_endpoint(endpoint)?;
    let output = adb_output(Command::new("adb").args(["connect", endpoint]))?;
    if !output.status.success() {
        return Err(command_error("ADB Wi-Fi connect", &output));
    }
    let message = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if message.to_lowercase().contains("failed") {
        return Err(BrandiError::Network(format!(
            "ADB Wi-Fi connect: {message}"
        )));
    }
    Ok(WifiConnection {
        action: "connected".into(),
        endpoint: endpoint.into(),
        device: None,
        message,
    })
}

pub fn pair_wifi(
    pair_endpoint: &str,
    pairing_code: &str,
    connect_endpoint: &str,
) -> Result<WifiConnection> {
    validate_endpoint(pair_endpoint)?;
    validate_endpoint(connect_endpoint)?;
    if pairing_code.len() != 6
        || !pairing_code
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Err(BrandiError::Invalid(
            "ADB pairing code must contain exactly six digits".into(),
        ));
    }
    // Feed the short-lived pairing code over stdin so it is absent from argv,
    // process listings, shell history, logs, and the returned connection data.
    let input = format!("{pairing_code}\n");
    let output = run_bounded_with_input(
        Command::new("adb").args(["pair", pair_endpoint]),
        Some(input.as_bytes()),
        Duration::from_secs(30),
        MAX_SUBPROCESS_OUTPUT_BYTES,
    )?;
    ensure_process_limits("ADB Wi-Fi pair", &output)?;
    if !output.status.success() {
        return Err(command_error("ADB Wi-Fi pair", &output));
    }
    let pairing_message = command_message(&output);
    if !pairing_message.to_lowercase().contains("success") {
        return Err(BrandiError::Network(format!(
            "ADB Wi-Fi pair was not confirmed: {pairing_message}"
        )));
    }
    let mut connected = connect_wifi(connect_endpoint)?;
    connected.action = "paired_and_connected".into();
    connected.message = format!("{pairing_message}; {}", connected.message);
    Ok(connected)
}

pub fn disconnect_wifi(endpoint: &str) -> Result<WifiConnection> {
    validate_endpoint(endpoint)?;
    let output = adb_output(Command::new("adb").args(["disconnect", endpoint]))?;
    if !output.status.success() {
        return Err(command_error("ADB Wi-Fi disconnect", &output));
    }
    Ok(WifiConnection {
        action: "disconnected".into(),
        endpoint: endpoint.into(),
        device: None,
        message: String::from_utf8_lossy(&output.stdout).trim().into(),
    })
}

pub fn bridge_usb_to_wifi(serial: &str, host: Option<&str>, port: u16) -> Result<WifiConnection> {
    if port < 1024 {
        return Err(BrandiError::Invalid(
            "ADB TCP port must be between 1024 and 65535".into(),
        ));
    }
    let device = list_devices()?
        .into_iter()
        .find(|device| device.serial == serial && device.state == "device")
        .ok_or_else(|| BrandiError::NotFound(format!("online USB device {serial}")))?;
    if device.transport != "usb" {
        return Err(BrandiError::Invalid(format!(
            "device {serial} is not connected over USB"
        )));
    }
    let output = adb_output(adb(serial).args(["tcpip", &port.to_string()]))?;
    if !output.status.success() {
        return Err(command_error("ADB USB TCP bridge", &output));
    }
    let address = if let Some(host) = host {
        validate_host(host)?;
        host.to_string()
    } else {
        wifi_address(serial)?
    };
    let endpoint = match address.parse::<IpAddr>() {
        Ok(IpAddr::V6(_)) => format!("[{address}]:{port}"),
        _ => format!("{address}:{port}"),
    };
    let mut connection = connect_wifi(&endpoint)?;
    connection.action = "usb_bridged_to_wifi".into();
    connection.device = Some(serial.into());
    Ok(connection)
}

pub fn stage(
    root: &Path,
    draft_id: &str,
    target_id: &str,
    device: Option<&str>,
    _identity: &automation::ActionIdentity,
) -> Result<AdbDelivery> {
    let config = AdbSocialConfig::load(root)?;
    let target = find_target(&config, target_id)?;
    let destination = format!("adb:{target_id}");
    let draft = approved_draft(root, draft_id, &destination)?;
    reject_unsafe_shell_content(&draft.content)?;
    let serial = resolve_device(&config, device)?;
    let output = adb_output(adb(&serial).args([
        "shell",
        "am",
        "start",
        "-W",
        "-a",
        "android.intent.action.SEND",
        "-t",
        &target.mime_type,
        "--es",
        "android.intent.extra.TEXT",
        &draft.content,
        "-p",
        &target.package,
    ]))?;
    if !output.status.success() {
        return Err(command_error("ADB share intent", &output));
    }
    append_audit(
        root,
        &AdbAuditEvent {
            timestamp: now(),
            draft_id,
            target: target_id,
            package: &target.package,
            device: &serial,
            action: "stage",
            result: "success",
            evidence: vec!["android.intent.action.SEND".into()],
        },
    )?;
    Ok(AdbDelivery {
        draft_id: draft_id.into(),
        target: target_id.into(),
        package: target.package.clone(),
        device: serial,
        action: "staged".into(),
        content: draft.content,
    })
}

pub fn publish(
    root: &Path,
    draft_id: &str,
    target_id: &str,
    device: Option<&str>,
    confirm: &str,
    identity: &automation::ActionIdentity,
) -> Result<AdbDelivery> {
    // Public delivery requires exact confirmation of the immutable draft ID.
    if confirm != draft_id {
        return Err(BrandiError::Invalid(format!(
            "exact confirmation required: pass `--confirm {draft_id}`"
        )));
    }
    let staged = stage(root, draft_id, target_id, device, identity)?;
    let config = AdbSocialConfig::load(root)?;
    let target = find_target(&config, target_id)?;
    thread::sleep(Duration::from_millis(target.settle_ms));

    let dump = adb_output(adb(&staged.device).args([
        "shell",
        "uiautomator",
        "dump",
        "/sdcard/brandi-window.xml",
    ]))?;
    if !dump.status.success() {
        return Err(command_error("uiautomator dump", &dump));
    }
    let xml =
        adb_output(adb(&staged.device).args(["exec-out", "cat", "/sdcard/brandi-window.xml"]))?;
    if !xml.status.success() {
        return Err(command_error("uiautomator read", &xml));
    }
    let (x, y) = selector_center(
        &String::from_utf8_lossy(&xml.stdout),
        &target.publish_selector,
    )
    .ok_or_else(|| {
        BrandiError::NotFound(format!(
            "publish selector `{}` was not present; nothing was tapped",
            target.publish_selector.value
        ))
    })?;
    let tap = adb_output(adb(&staged.device).args([
        "shell",
        "input",
        "tap",
        &x.to_string(),
        &y.to_string(),
    ]))?;
    if !tap.status.success() {
        return Err(command_error("ADB publish tap", &tap));
    }
    automation::record_delivery(
        root,
        draft_id,
        true,
        identity,
        &format!("adb:{target_id}"),
        Some(format!(
            "ADB target {target_id} on device {}",
            staged.device
        )),
    )?;
    append_audit(
        root,
        &AdbAuditEvent {
            timestamp: now(),
            draft_id,
            target: target_id,
            package: &target.package,
            device: &staged.device,
            action: "publish",
            result: "success",
            evidence: vec![
                format!(
                    "selector:{}={}",
                    target.publish_selector.kind, target.publish_selector.value
                ),
                format!("tap:{x},{y}"),
            ],
        },
    )?;
    Ok(AdbDelivery {
        action: "published".into(),
        ..staged
    })
}

/// Characters that are unsafe to hand to `adb shell`: adb joins every
/// argument after `shell` into one string and passes it to a real shell on
/// the connected device, so control operators or command substitution in
/// otherwise-approved marketing copy could execute there rather than post
/// as literal text.
const UNSAFE_SHELL_CHARS: [char; 7] = [';', '|', '&', '`', '<', '>', '\\'];

fn reject_unsafe_shell_content(content: &str) -> Result<()> {
    if content.contains(UNSAFE_SHELL_CHARS.as_slice()) || content.contains("$(") {
        return Err(BrandiError::Invalid(
            "draft content contains characters unsafe to pass through `adb shell` \
             (any of ; | & ` < > \\ or $()); edit the draft to remove them before staging"
                .into(),
        ));
    }
    Ok(())
}

fn approved_draft(root: &Path, id: &str, destination: &str) -> Result<ContentDraft> {
    let draft = automation::list_queue(root)?
        .into_iter()
        .find(|draft| draft.id == id)
        .ok_or_else(|| BrandiError::NotFound(format!("content draft {id}")))?;
    if !draft.approval_is_valid_for(destination) {
        return Err(BrandiError::Invalid(format!(
            "draft {id} has no current approval for destination `{destination}`"
        )));
    }
    Ok(draft)
}

fn find_target<'a>(config: &'a AdbSocialConfig, id: &str) -> Result<&'a AdbTarget> {
    config
        .targets
        .iter()
        .find(|target| target.id == id)
        .ok_or_else(|| BrandiError::NotFound(format!("ADB social target {id}")))
}

fn resolve_device(config: &AdbSocialConfig, requested: Option<&str>) -> Result<String> {
    if let Some(serial) = requested.filter(|serial| !serial.is_empty()) {
        return Ok(serial.into());
    }
    if !config.default_device.is_empty() {
        return Ok(config.default_device.clone());
    }
    let online = list_devices()?
        .into_iter()
        .filter(|device| device.state == "device")
        .collect::<Vec<_>>();
    match online.as_slice() {
        [device] => Ok(device.serial.clone()),
        [] => Err(BrandiError::NotFound(
            "no online ADB device; connect one or pass --device".into(),
        )),
        _ => Err(BrandiError::Invalid(
            "multiple ADB devices are online; pass --device or set default_device".into(),
        )),
    }
}

fn adb(serial: &str) -> Command {
    let mut command = Command::new("adb");
    command.args(["-s", serial]);
    command
}

fn parse_devices(output: &str) -> Vec<AdbDevice> {
    output
        .lines()
        .skip_while(|line| !line.starts_with("List of devices"))
        .skip(1)
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 2 {
                return None;
            }
            let detail = |key: &str| {
                fields
                    .iter()
                    .find_map(|field| field.strip_prefix(&format!("{key}:")))
                    .unwrap_or("")
                    .to_string()
            };
            Some(AdbDevice {
                serial: fields[0].into(),
                state: fields[1].into(),
                model: detail("model"),
                product: detail("product"),
                transport: if fields.iter().any(|field| field.starts_with("usb:")) {
                    "usb"
                } else if fields[0].contains(':') {
                    "wifi"
                } else if fields[0].starts_with("emulator-") {
                    "emulator"
                } else {
                    "usb"
                }
                .into(),
            })
        })
        .collect()
}

fn validate_endpoint(endpoint: &str) -> Result<()> {
    let pattern = Regex::new(r"^(?:\[[0-9A-Fa-f:]+\]|[A-Za-z0-9._-]+):([0-9]{1,5})$")
        .map_err(|error| BrandiError::Invalid(error.to_string()))?;
    let captures = pattern.captures(endpoint).ok_or_else(|| {
        BrandiError::Invalid("ADB Wi-Fi endpoint must be host:port or [IPv6]:port".into())
    })?;
    let port = captures[1]
        .parse::<u32>()
        .map_err(|_| BrandiError::Invalid("ADB Wi-Fi endpoint has an invalid port".into()))?;
    if !(1..=65_535).contains(&port) {
        return Err(BrandiError::Invalid(
            "ADB Wi-Fi endpoint port must be between 1 and 65535".into(),
        ));
    }
    Ok(())
}

fn validate_host(host: &str) -> Result<()> {
    let hostname = Regex::new(r"^[A-Za-z0-9._-]+$")
        .map_err(|error| BrandiError::Invalid(error.to_string()))?;
    if host.parse::<IpAddr>().is_err() && !hostname.is_match(host) {
        return Err(BrandiError::Invalid(
            "ADB Wi-Fi host must be an IP address or hostname".into(),
        ));
    }
    Ok(())
}

fn wifi_address(serial: &str) -> Result<String> {
    let output = adb_output(adb(serial).args(["shell", "ip", "route"]))?;
    if !output.status.success() {
        return Err(command_error("read Android Wi-Fi address", &output));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    parse_route_address(&text).ok_or_else(|| {
        BrandiError::NotFound(
            "Android Wi-Fi address; connect the device to Wi-Fi or pass --host".into(),
        )
    })
}

fn parse_route_address(route: &str) -> Option<String> {
    let fields = route.split_whitespace().collect::<Vec<_>>();
    fields
        .windows(2)
        .find(|window| window[0] == "src" && window[1].parse::<IpAddr>().is_ok())
        .map(|window| window[1].to_string())
}

fn selector_center(xml: &str, selector: &UiSelector) -> Option<(u32, u32)> {
    let attribute = match selector.kind.as_str() {
        "resource_id" => "resource-id",
        "text" => "text",
        "content_desc" => "content-desc",
        _ => return None,
    };
    let nodes = Regex::new(r#"<node\b[^>]*>"#).ok()?;
    let bounds = Regex::new(r#"bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]""#).ok()?;
    for node in nodes.find_iter(xml).map(|match_| match_.as_str()) {
        let needle = format!(r#"{attribute}="{}""#, selector.value);
        if !node.contains(&needle) {
            continue;
        }
        let captures = bounds.captures(node)?;
        let left = captures[1].parse::<u32>().ok()?;
        let top = captures[2].parse::<u32>().ok()?;
        let right = captures[3].parse::<u32>().ok()?;
        let bottom = captures[4].parse::<u32>().ok()?;
        return Some(((left + right) / 2, (top + bottom) / 2));
    }
    None
}

fn append_audit(root: &Path, event: &AdbAuditEvent<'_>) -> Result<()> {
    let path = root.join(".brandi/state/adb-social.jsonl");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(event)?)?;
    Ok(())
}

fn adb_output(command: &mut Command) -> Result<BoundedOutput> {
    let output = run_bounded(
        command,
        Duration::from_secs(30),
        MAX_SUBPROCESS_OUTPUT_BYTES,
    )?;
    ensure_process_limits("ADB command", &output)?;
    Ok(output)
}

fn ensure_process_limits(action: &str, output: &BoundedOutput) -> Result<()> {
    if output.timed_out {
        return Err(BrandiError::Network(format!(
            "{action} exceeded the 30 second subprocess limit"
        )));
    }
    if output.truncated {
        return Err(BrandiError::Network(format!(
            "{action} exceeded the subprocess output limit"
        )));
    }
    Ok(())
}

fn command_error(action: &str, output: &BoundedOutput) -> BrandiError {
    let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
    BrandiError::Network(if message.is_empty() {
        format!("{action} exited with {}", output.status)
    } else {
        format!("{action}: {message}")
    })
}

fn command_message(output: &BoundedOutput) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

fn program_exists(program: &str) -> bool {
    !program.contains(std::path::MAIN_SEPARATOR)
        && std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path).any(|directory| directory.join(program).is_file())
        })
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_device_inventory() {
        let devices = parse_devices(
            "List of devices attached\nemulator-5554 device product:sdk model:Pixel_8 transport_id:1\nR58M device usb:1-2 product:phone model:Galaxy transport_id:2\n192.168.1.5:5555 device product:phone model:Pixel transport_id:3\nphone offline transport_id:4\n",
        );
        assert_eq!(devices.len(), 4);
        assert_eq!(devices[0].serial, "emulator-5554");
        assert_eq!(devices[0].model, "Pixel_8");
        assert_eq!(devices[0].transport, "emulator");
        assert_eq!(devices[1].transport, "usb");
        assert_eq!(devices[2].transport, "wifi");
        assert_eq!(devices[3].state, "offline");
    }

    #[test]
    fn finds_selector_center_without_tapping() {
        let xml = r#"<hierarchy><node text="Post" resource-id="com.app:id/post" content-desc="" bounds="[100,200][300,260]"></node></hierarchy>"#;
        let center = selector_center(
            xml,
            &UiSelector {
                kind: "resource_id".into(),
                value: "com.app:id/post".into(),
            },
        );
        assert_eq!(center, Some((200, 230)));
    }

    #[test]
    fn rejects_shell_metacharacters_in_staged_content() {
        assert!(reject_unsafe_shell_content("Ship the new release today").is_ok());
        assert!(reject_unsafe_shell_content("Prices start at $99/month").is_ok());
        for bad in [
            "ship it; rm -rf /data",
            "ship it && reboot",
            "ship it | sh",
            "ship it `whoami`",
            "ship it $(whoami)",
            "ship it > /dev/null",
            "ship it \\n more",
        ] {
            assert!(
                reject_unsafe_shell_content(bad).is_err(),
                "expected rejection for: {bad}"
            );
        }
    }

    #[test]
    fn publishing_requires_exact_confirmation_before_adb() {
        let root = tempfile::tempdir().unwrap();
        let error = publish(
            root.path(),
            "draft-7",
            "x",
            None,
            "wrong",
            &automation::ActionIdentity::test_local(1000),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("exact confirmation"));
    }

    #[test]
    fn validates_wifi_inputs_before_adb() {
        assert!(validate_endpoint("192.168.1.10:37123").is_ok());
        assert!(validate_endpoint("pixel.local:5555").is_ok());
        assert!(validate_endpoint("bad endpoint").is_err());
        let error = pair_wifi("bad", "123456", "also-bad").unwrap_err();
        assert!(error.to_string().contains("host:port"));
    }

    #[test]
    fn parses_usb_route_address() {
        assert_eq!(
            parse_route_address("default via 192.168.1.1 dev wlan0 src 192.168.1.42 metric 303"),
            Some("192.168.1.42".into())
        );
    }
}
