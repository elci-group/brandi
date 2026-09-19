//! TikTok Login Kit, owned-account evidence, and reviewable brand drafts.
//! Credentials and OAuth state live outside the project in the user's state directory.
//! Snapshots and rendered media live in the project's `.brandi/state/` directory.

use crate::automation::{curl_config_line, curl_with_config};
use crate::brief::Brief;
use crate::error::{BrandiError, Result};
use crate::guidelines::Guidelines;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const API: &str = "https://open.tiktokapis.com";
const SCOPES: &str = "user.info.basic,user.info.profile,user.info.stats,video.list,video.upload";

#[derive(Debug, Serialize, Deserialize)]
struct PendingLogin {
    state: String,
    redirect_uri: String,
    created_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Session {
    access_token: String,
    refresh_token: String,
    expires_at: i64,
    scopes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Video {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub create_time: i64,
    #[serde(default)]
    pub view_count: u64,
    #[serde(default)]
    pub like_count: u64,
    #[serde(default)]
    pub comment_count: u64,
    #[serde(default)]
    pub share_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub captured_at: DateTime<Utc>,
    pub open_id: String,
    pub display_name: String,
    pub username: String,
    pub bio: String,
    pub follower_count: u64,
    pub likes_count: u64,
    pub videos: Vec<Video>,
}

fn state_dir(root: &Path) -> Result<PathBuf> {
    let dir = root.join(".brandi/state/tiktok");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn secret_dir(root: &Path) -> Result<PathBuf> {
    let base = if let Some(xdg) = std::env::var_os("XDG_STATE_HOME") {
        PathBuf::from(xdg)
    } else {
        PathBuf::from(std::env::var_os("HOME").ok_or_else(|| {
            BrandiError::Invalid("HOME or XDG_STATE_HOME is required for TikTok login".into())
        })?)
        .join(".local/state")
    };
    if !base.is_absolute() {
        return Err(BrandiError::Invalid(
            "TikTok state home must be an absolute path".into(),
        ));
    }
    let digest = Sha256::digest(root.canonicalize()?.to_string_lossy().as_bytes());
    let dir = base.join("brandi/tiktok").join(format!("{digest:x}"));
    fs::create_dir_all(&dir)?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = path.with_extension("tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)?;
    fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))?;
    use std::io::Write;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(temp, path)?;
    Ok(())
}

fn encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn decode(value: &str) -> Result<String> {
    let mut bytes = Vec::new();
    let mut source = value.as_bytes().iter().copied();
    while let Some(byte) = source.next() {
        if byte == b'%' {
            let hex = [
                source
                    .next()
                    .ok_or_else(|| BrandiError::Invalid("invalid callback encoding".into()))?,
                source
                    .next()
                    .ok_or_else(|| BrandiError::Invalid("invalid callback encoding".into()))?,
            ];
            let text = std::str::from_utf8(&hex)
                .map_err(|_| BrandiError::Invalid("invalid callback encoding".into()))?;
            bytes.push(
                u8::from_str_radix(text, 16)
                    .map_err(|_| BrandiError::Invalid("invalid callback encoding".into()))?,
            );
        } else if byte == b'+' {
            bytes.push(b' ');
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).map_err(|_| BrandiError::Invalid("invalid callback encoding".into()))
}

fn param(url: &str, key: &str) -> Result<Option<String>> {
    for pair in url
        .split('?')
        .nth(1)
        .unwrap_or("")
        .split('#')
        .next()
        .unwrap_or("")
        .split('&')
    {
        if let Some((name, value)) = pair.split_once('=') {
            if name == key {
                return decode(value).map(Some);
            }
        }
    }
    Ok(None)
}

pub fn login_url(root: &Path, redirect_uri: &str) -> Result<Value> {
    if !redirect_uri.starts_with("https://") || redirect_uri.contains('#') {
        return Err(BrandiError::Invalid(
            "TikTok redirect URI must be a registered HTTPS URL".into(),
        ));
    }
    let client_key = std::env::var("TIKTOK_CLIENT_KEY")
        .map_err(|_| BrandiError::Invalid("set TIKTOK_CLIENT_KEY".into()))?;
    let mut random = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
    let state = random
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let pending = PendingLogin {
        state: state.clone(),
        redirect_uri: redirect_uri.into(),
        created_at: Utc::now().timestamp(),
    };
    private_write(
        &secret_dir(root)?.join("oauth.json"),
        &serde_json::to_vec(&pending)?,
    )?;
    let url = format!("https://www.tiktok.com/v2/auth/authorize/?client_key={}&scope={}&response_type=code&redirect_uri={}&state={}", encode(&client_key), encode(SCOPES), encode(redirect_uri), state);
    Ok(
        json!({"authorization_url": url, "instruction": "Open this URL, choose email on TikTok's own login page, approve the requested scopes, then pass the final redirected URL on stdin to `brandi social tiktok login-complete`."}),
    )
}

fn api_post(url: &str, token: Option<&str>, body: &str, form: bool) -> Result<Value> {
    let mut config = curl_config_line("url", url);
    config += &curl_config_line(
        "header",
        if form {
            "Content-Type: application/x-www-form-urlencoded"
        } else {
            "Content-Type: application/json"
        },
    );
    if let Some(token) = token {
        config += &curl_config_line("header", &format!("Authorization: Bearer {token}"));
    }
    config += &curl_config_line("data-binary", body);
    let output =
        curl_with_config(&["-sS", "--fail-with-body", "-X", "POST"], &config).ok_or_else(|| {
            BrandiError::Network(
                "TikTok request failed; check connectivity and granted scopes".into(),
            )
        })?;
    let value: Value = serde_json::from_slice(&output.stdout)?;
    if let Some(code) = value.pointer("/error/code").and_then(Value::as_str) {
        if code != "ok" {
            return Err(BrandiError::Network(format!("TikTok API: {code}")));
        }
    }
    Ok(value)
}

fn api_get(url: &str, token: &str) -> Result<Value> {
    let mut config = curl_config_line("url", url);
    config += &curl_config_line("header", &format!("Authorization: Bearer {token}"));
    let output = curl_with_config(&["-sS", "--fail-with-body"], &config).ok_or_else(|| {
        BrandiError::Network("TikTok request failed; check connectivity and granted scopes".into())
    })?;
    let value: Value = serde_json::from_slice(&output.stdout)?;
    if let Some(code) = value.pointer("/error/code").and_then(Value::as_str) {
        if code != "ok" {
            return Err(BrandiError::Network(format!("TikTok API: {code}")));
        }
    }
    Ok(value)
}

pub fn login_complete(root: &Path, callback: &str) -> Result<Value> {
    let pending_path = secret_dir(root)?.join("oauth.json");
    let pending: PendingLogin = serde_json::from_slice(&fs::read(&pending_path)?)?;
    if Utc::now().timestamp() - pending.created_at > 600
        || !callback.starts_with(&format!("{}?", pending.redirect_uri))
    {
        return Err(BrandiError::Invalid(
            "expired login or callback URL does not match the registered redirect".into(),
        ));
    }
    let received_state = param(callback, "state")?
        .ok_or_else(|| BrandiError::Invalid("callback has no state".into()))?;
    if received_state != pending.state {
        return Err(BrandiError::Invalid("TikTok login state mismatch".into()));
    }
    let code = param(callback, "code")?
        .ok_or_else(|| BrandiError::Invalid("callback has no authorization code".into()))?;
    let key = std::env::var("TIKTOK_CLIENT_KEY")
        .map_err(|_| BrandiError::Invalid("set TIKTOK_CLIENT_KEY".into()))?;
    let secret = std::env::var("TIKTOK_CLIENT_SECRET")
        .map_err(|_| BrandiError::Invalid("set TIKTOK_CLIENT_SECRET".into()))?;
    let body = format!(
        "client_key={}&client_secret={}&code={}&grant_type=authorization_code&redirect_uri={}",
        encode(&key),
        encode(&secret),
        encode(&code),
        encode(&pending.redirect_uri)
    );
    let response = api_post(&format!("{API}/v2/oauth/token/"), None, &body, true)?;
    let token = response
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            BrandiError::Network("TikTok token exchange did not return an access token".into())
        })?;
    let session = Session {
        access_token: token.into(),
        refresh_token: response
            .get("refresh_token")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        expires_at: Utc::now().timestamp()
            + response
                .get("expires_in")
                .and_then(Value::as_i64)
                .unwrap_or(0),
        scopes: response
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
    };
    private_write(
        &secret_dir(root)?.join("session.json"),
        &serde_json::to_vec(&session)?,
    )?;
    fs::remove_file(pending_path)?;
    Ok(json!({"connected": true, "scopes": session.scopes, "expires_at": session.expires_at}))
}

fn access_token(root: &Path) -> Result<String> {
    if let Ok(token) = std::env::var("TIKTOK_ACCESS_TOKEN") {
        if !token.is_empty() {
            return Ok(token);
        }
    }
    let path = secret_dir(root)?.join("session.json");
    let mut session: Session = serde_json::from_slice(&fs::read(&path)?)?;
    if session.expires_at <= Utc::now().timestamp() + 60 {
        if session.refresh_token.is_empty() {
            return Err(BrandiError::Invalid(
                "TikTok session expired; sign in again or set TIKTOK_ACCESS_TOKEN".into(),
            ));
        }
        let key = std::env::var("TIKTOK_CLIENT_KEY").map_err(|_| {
            BrandiError::Invalid("set TIKTOK_CLIENT_KEY to refresh the TikTok session".into())
        })?;
        let secret = std::env::var("TIKTOK_CLIENT_SECRET").map_err(|_| {
            BrandiError::Invalid("set TIKTOK_CLIENT_SECRET to refresh the TikTok session".into())
        })?;
        let body = format!(
            "client_key={}&client_secret={}&grant_type=refresh_token&refresh_token={}",
            encode(&key),
            encode(&secret),
            encode(&session.refresh_token)
        );
        let response = api_post(&format!("{API}/v2/oauth/token/"), None, &body, true)?;
        session.access_token = response
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                BrandiError::Network("TikTok refresh did not return an access token".into())
            })?
            .into();
        session.refresh_token = response
            .get("refresh_token")
            .and_then(Value::as_str)
            .unwrap_or(&session.refresh_token)
            .into();
        session.expires_at = Utc::now().timestamp()
            + response
                .get("expires_in")
                .and_then(Value::as_i64)
                .unwrap_or(0);
        session.scopes = response
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or(&session.scopes)
            .into();
        private_write(&path, &serde_json::to_vec(&session)?)?;
    }
    Ok(session.access_token)
}

pub fn sync(root: &Path) -> Result<Value> {
    let token = access_token(root)?;
    let user_response = api_get(&format!("{API}/v2/user/info/?fields=open_id,display_name,username,bio_description,follower_count,likes_count"), &token)?;
    let user = user_response
        .pointer("/data/user")
        .ok_or_else(|| BrandiError::Network("TikTok did not return profile data".into()))?;
    let mut videos = Vec::new();
    let mut cursor = None;
    for _ in 0..5 {
        let body = if let Some(cursor) = cursor {
            json!({"max_count": 20, "cursor": cursor})
        } else {
            json!({"max_count": 20})
        };
        let response = api_post(&format!("{API}/v2/video/list/?fields=id,title,create_time,view_count,like_count,comment_count,share_count"), Some(&token), &body.to_string(), false)?;
        let data = response
            .get("data")
            .ok_or_else(|| BrandiError::Network("TikTok did not return video data".into()))?;
        for raw in data
            .get("videos")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            videos.push(serde_json::from_value::<Video>(raw.clone())?);
        }
        if !data
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            break;
        }
        cursor = data.get("cursor").and_then(Value::as_i64);
        if cursor.is_none() {
            break;
        }
    }
    let follower_count = user
        .get("follower_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            BrandiError::Invalid(
                "TikTok did not grant user.info.stats; follower trends require that scope".into(),
            )
        })?;
    let snapshot = Snapshot {
        captured_at: Utc::now(),
        open_id: user
            .get("open_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        display_name: user
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        username: user
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        bio: user
            .get("bio_description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        follower_count,
        likes_count: user.get("likes_count").and_then(Value::as_u64).unwrap_or(0),
        videos,
    };
    let dir = state_dir(root)?.join("snapshots");
    fs::create_dir_all(&dir)?;
    let filename = format!("{}.json", snapshot.captured_at.format("%Y%m%dT%H%M%S%3fZ"));
    private_write(&dir.join(&filename), &serde_json::to_vec_pretty(&snapshot)?)?;
    Ok(
        json!({"snapshot": filename, "profile": snapshot.display_name, "followers": snapshot.follower_count, "videos": snapshot.videos.len()}),
    )
}

pub fn analyze(root: &Path) -> Result<Value> {
    let dir = state_dir(root)?.join("snapshots");
    let mut paths = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    paths.sort();
    let snapshots = paths
        .iter()
        .map(|path| {
            fs::read(path).map_err(BrandiError::from).and_then(|bytes| {
                serde_json::from_slice::<Snapshot>(&bytes).map_err(BrandiError::from)
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let latest = snapshots
        .last()
        .ok_or_else(|| BrandiError::NotFound("no TikTok snapshots; run sync first".into()))?;
    let mut posts = latest.videos.clone();
    posts.sort_by_key(|video| std::cmp::Reverse(video.view_count));
    let post_analysis = posts.iter().map(|video| json!({"id": video.id, "title": video.title, "views": video.view_count, "likes": video.like_count, "comments": video.comment_count, "shares": video.share_count, "engagement_rate": if video.view_count == 0 { None } else { Some((video.like_count + video.comment_count + video.share_count) as f64 / video.view_count as f64) }})).collect::<Vec<_>>();
    let trend = snapshots.iter().map(|snapshot| {
        let (views, interactions) = snapshot.videos.iter().fold((0u64, 0u64), |(views, interactions), video| (views + video.view_count, interactions + video.like_count + video.comment_count + video.share_count));
        json!({"at": snapshot.captured_at, "followers": snapshot.follower_count, "follower_change": snapshot.follower_count as i64 - snapshots.first().unwrap().follower_count as i64, "sampled_views": views, "sampled_interactions": interactions, "sampled_engagement_rate": if views == 0 { None } else { Some(interactions as f64 / views as f64) }})
    }).collect::<Vec<_>>();
    Ok(
        json!({"as_of": latest.captured_at, "post_analysis": post_analysis, "trend": trend, "limits": "Trend points begin when sync starts; video history covers at most 100 recent public posts. Engagement is (likes + comments + shares) / views for sampled posts."}),
    )
}

fn brand(root: &Path) -> Result<(Brief, Guidelines)> {
    Ok((Brief::load(root)?, Guidelines::load(root)?))
}

pub fn draft_reply(root: &Path, incoming: &str, third_party: bool) -> Result<Value> {
    let (brief, guidelines) = brand(root)?;
    let incoming = incoming.trim();
    if incoming.is_empty() || incoming.chars().count() > 2000 {
        return Err(BrandiError::Invalid(
            "message must contain 1–2000 characters".into(),
        ));
    }
    let name = &brief.identity.product.name;
    let mission = &brief.identity.product.mission;
    let topic = incoming
        .split_whitespace()
        .take(12)
        .collect::<Vec<_>>()
        .join(" ");
    let draft = if third_party {
        format!("From {name}: your point about “{topic}” resonates with our focus on {mission} What have you found works in practice?")
    } else {
        format!("Thanks for your note about “{topic}”. At {name}, we're focused on {mission} What outcome would be most useful to you?")
    };
    let banned = guidelines
        .prohibited
        .categories
        .values()
        .flatten()
        .filter(|term| draft.to_lowercase().contains(&term.to_lowercase()))
        .cloned()
        .collect::<Vec<_>>();
    Ok(
        json!({"draft": draft, "context": incoming, "channel": if third_party { "third_party_comment" } else { "message_response" }, "voice_traits": guidelines.voice.traits, "prohibited_matches": banned, "review_required": true, "publishing": "Copy into TikTok after review; general comment/DM write access is not available in the public developer API."}),
    )
}

pub fn profile_plan(root: &Path, season: Option<&str>) -> Result<Value> {
    let (brief, guidelines) = brand(root)?;
    let season = season.unwrap_or("evergreen").trim();
    if season.is_empty()
        || season.len() > 64
        || !season
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == ' ' || ch == '-')
    {
        return Err(BrandiError::Invalid(
            "season must contain 1–64 characters".into(),
        ));
    }
    let product = &brief.identity.product;
    let bio = if season.eq_ignore_ascii_case("evergreen") {
        product.tagline.clone()
    } else {
        format!("{} · {season}", product.tagline)
    };
    let accent = if guidelines.visual.palette.secondary.is_empty() {
        &guidelines.visual.palette.primary
    } else {
        let index = season
            .bytes()
            .fold(0usize, |sum, byte| sum.wrapping_add(byte as usize))
            % guidelines.visual.palette.secondary.len();
        &guidelines.visual.palette.secondary[index]
    };
    Ok(
        json!({"season": season, "display_name": product.name, "bio_draft": bio, "visual_direction": {"primary": guidelines.visual.palette.primary, "accent": accent, "seasonal_cue": season, "geometry": guidelines.visual.visual_language.geometry}, "review_required": true, "application": "Apply profile changes in TikTok after review; the public developer API does not expose profile editing."}),
    )
}

/// Capture a device screen to an ignored project-local PNG. The device must
/// already be connected and authorized in ADB.
pub fn capture_screen(root: &Path, device: Option<&str>) -> Result<Value> {
    let mut command = Command::new("adb");
    if let Some(device) = device {
        if device.is_empty()
            || !device
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-._:".contains(&byte))
        {
            return Err(BrandiError::Invalid("invalid ADB device serial".into()));
        }
        command.args(["-s", device]);
    }
    command.args(["exec-out", "screencap", "-p"]);
    let output =
        crate::process::run_bounded(&mut command, Duration::from_secs(15), 16 * 1024 * 1024)?;
    if !output.status.success()
        || output.timed_out
        || output.truncated
        || !output.stdout.starts_with(b"\x89PNG\r\n\x1a\n")
    {
        return Err(BrandiError::Invalid(
            "ADB did not return a complete PNG screen capture".into(),
        ));
    }
    let path = state_dir(root)?.join(format!(
        "capture-{}.png",
        Utc::now().format("%Y%m%dT%H%M%S%3fZ")
    ));
    private_write(&path, &output.stdout)?;
    Ok(json!({"capture": path, "bytes": output.stdout.len()}))
}

/// Render a short portrait video from a screenshot, with optional audio.
/// The result is a local review artifact and is never uploaded automatically.
pub fn content(root: &Path, screenshot: &Path, audio: Option<&Path>) -> Result<Value> {
    let root = root.canonicalize()?;
    let screenshot = screenshot.canonicalize()?;
    if !screenshot.starts_with(&root)
        || !matches!(
            screenshot.extension().and_then(|value| value.to_str()),
            Some("png" | "jpg" | "jpeg")
        )
    {
        return Err(BrandiError::Invalid(
            "screenshot must be a PNG or JPEG inside the project".into(),
        ));
    }
    image::ImageReader::open(&screenshot)?
        .with_guessed_format()?
        .decode()?;
    let audio = if let Some(path) = audio {
        let path = path.canonicalize()?;
        if !path.starts_with(&root)
            || !matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("mp3" | "wav" | "m4a")
            )
        {
            return Err(BrandiError::Invalid(
                "audio must be MP3, WAV, or M4A inside the project".into(),
            ));
        }
        Some(path)
    } else {
        None
    };
    let (brief, guidelines) = brand(&root)?;
    let output = state_dir(&root)?.join(format!(
        "content-{}.mp4",
        Utc::now().format("%Y%m%dT%H%M%S%3fZ")
    ));
    let mut command = Command::new("ffmpeg");
    command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-loop",
            "1",
            "-framerate",
            "30",
            "-i",
        ])
        .arg(&screenshot);
    if let Some(audio) = &audio {
        command.arg("-i").arg(audio);
    }
    command.args(["-vf", "scale=1080:1920:force_original_aspect_ratio=decrease,pad=1080:1920:(ow-iw)/2:(oh-ih)/2:black,format=yuv420p", "-c:v", "libx264", "-preset", "veryfast", "-t", "8", "-movflags", "+faststart"]);
    if audio.is_some() {
        command.args(["-c:a", "aac", "-shortest"]);
    } else {
        command.arg("-an");
    }
    command.arg(&output);
    let render = crate::process::run_bounded(&mut command, Duration::from_secs(90), 32 * 1024)?;
    if !render.status.success() || render.timed_out || render.truncated {
        return Err(BrandiError::Invalid(format!(
            "ffmpeg could not render TikTok content: {}",
            String::from_utf8_lossy(&render.stderr)
        )));
    }
    let caption = format!(
        "{} — {}",
        brief.identity.product.name, brief.identity.product.tagline
    );
    Ok(
        json!({"video": output, "screenshot": screenshot, "audio": audio, "caption_draft": caption, "palette": guidelines.visual.palette.primary, "review_required": true, "next_step": "Review the video and caption, then use `brandi social tiktok upload` to send the video to TikTok's inbox for final editing and posting."}),
    )
}

/// Upload an approved local MP4 as a TikTok inbox draft. The creator finishes
/// editing and publishing inside TikTok; this is not direct posting.
pub fn upload(root: &Path, video: &Path, confirm: &str) -> Result<Value> {
    let root = root.canonicalize()?;
    let video = video.canonicalize()?;
    if !video.starts_with(&root)
        || video.extension().and_then(|value| value.to_str()) != Some("mp4")
    {
        return Err(BrandiError::Invalid(
            "video must be a project-local MP4".into(),
        ));
    }
    if video.file_name().and_then(|value| value.to_str()) != Some(confirm) {
        return Err(BrandiError::Invalid(
            "--confirm must exactly match the MP4 filename".into(),
        ));
    }
    let size = fs::metadata(&video)?.len();
    if !(1..=64 * 1024 * 1024).contains(&size) {
        return Err(BrandiError::Invalid(
            "inbox upload supports MP4 files up to 64 MiB".into(),
        ));
    }
    let token = access_token(&root)?;
    let body = json!({"source_info": {"source": "FILE_UPLOAD", "video_size": size, "chunk_size": size, "total_chunk_count": 1}});
    let response = api_post(
        &format!("{API}/v2/post/publish/inbox/video/init/"),
        Some(&token),
        &body.to_string(),
        false,
    )?;
    let url = response
        .pointer("/data/upload_url")
        .and_then(Value::as_str)
        .ok_or_else(|| BrandiError::Network("TikTok did not return an upload URL".into()))?;
    if !url.starts_with("https://open-upload.tiktokapis.com/video/?") {
        return Err(BrandiError::Network(
            "TikTok returned an unexpected upload host".into(),
        ));
    }
    let mut config = curl_config_line("url", url);
    config += &curl_config_line("header", "Content-Type: video/mp4");
    config += &curl_config_line(
        "header",
        &format!("Content-Range: bytes 0-{}/{size}", size - 1),
    );
    config += &curl_config_line("header", &format!("Content-Length: {size}"));
    // Use a config directive for the signed URL and file path: neither is
    // exposed in process arguments or application logs.
    config += &curl_config_line("data-binary", &format!("@{}", video.display()));
    let transfer = crate::process::run_bounded_with_input(
        Command::new("curl").args([
            "-sS",
            "--fail",
            "--connect-timeout",
            "5",
            "--max-time",
            "180",
            "-X",
            "PUT",
            "-K",
            "-",
        ]),
        Some(config.as_bytes()),
        Duration::from_secs(185),
        32 * 1024,
    )?;
    if !transfer.status.success() || transfer.timed_out || transfer.truncated {
        return Err(BrandiError::Network("TikTok video transfer failed".into()));
    }
    Ok(
        json!({"uploaded": true, "publish_id": response.pointer("/data/publish_id"), "next_step": "Open the TikTok inbox notification, review the draft, and post it there."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oauth_encoding_round_trip() {
        let value = "a+b /?=✓";
        assert_eq!(decode(&encode(value)).unwrap(), value);
    }
    #[test]
    fn callback_parameter_parses_encoded_values() {
        assert_eq!(
            param("https://example.test/?code=a%2Bb&state=s", "code")
                .unwrap()
                .as_deref(),
            Some("a+b")
        );
    }
    #[test]
    fn engagement_uses_views_as_denominator() {
        let video = Video {
            id: "1".into(),
            title: "x".into(),
            create_time: 0,
            view_count: 100,
            like_count: 10,
            comment_count: 2,
            share_count: 3,
        };
        assert_eq!(
            (video.like_count + video.comment_count + video.share_count) as f64
                / video.view_count as f64,
            0.15
        );
    }

    #[test]
    fn analysis_tracks_follower_and_engagement_changes() {
        let project = tempfile::tempdir().unwrap();
        let dir = state_dir(project.path()).unwrap().join("snapshots");
        fs::create_dir_all(&dir).unwrap();
        let video = Video {
            id: "v1".into(),
            title: "Launch".into(),
            create_time: 1,
            view_count: 100,
            like_count: 10,
            comment_count: 2,
            share_count: 3,
        };
        for (name, followers) in [("a.json", 100), ("b.json", 125)] {
            let snapshot = Snapshot {
                captured_at: Utc::now(),
                open_id: "owner".into(),
                display_name: "Brand".into(),
                username: "brand".into(),
                bio: "".into(),
                follower_count: followers,
                likes_count: 10,
                videos: vec![video.clone()],
            };
            fs::write(dir.join(name), serde_json::to_vec(&snapshot).unwrap()).unwrap();
        }
        let report = analyze(project.path()).unwrap();
        assert_eq!(report["trend"][1]["follower_change"], 25);
        assert_eq!(report["post_analysis"][0]["engagement_rate"], 0.15);
    }

    #[test]
    fn drafts_use_the_brand_and_season_changes_the_profile_plan() {
        let project = tempfile::tempdir().unwrap();
        Brief::scaffold(project.path()).unwrap();
        Guidelines::scaffold(project.path()).unwrap();
        let reply = draft_reply(project.path(), "Can you explain the scanner?", false).unwrap();
        assert!(reply["draft"].as_str().unwrap().contains("scanner"));
        assert_eq!(reply["review_required"], true);
        let evergreen = profile_plan(project.path(), None).unwrap();
        let winter = profile_plan(project.path(), Some("winter")).unwrap();
        assert_ne!(evergreen["bio_draft"], winter["bio_draft"]);
        assert_eq!(winter["season"], "winter");
    }
}
