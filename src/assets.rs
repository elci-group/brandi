//! Brand asset checks: images, dimensions, palette coverage.

use crate::error::{BrandiError, Result};
use crate::guidelines::Guidelines;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Which asset spec to check an image against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    /// Infer the spec from filename/content.
    Auto,
    SocialCard,
    Thumbnail,
    Icon,
}

/// Upper bound on sampled (non-transparent) pixels per image, so huge
/// images stay cheap. The sampling stride is derived from this.
const MAX_SAMPLES: u64 = 20_000;
const MAX_IMAGE_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMAGE_DIMENSION: u32 = 8_192;
const MAX_IMAGE_PIXELS: u64 = 40_000_000;
const MAX_IMAGE_ALLOCATION_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ASSETS_PER_COMMAND: usize = 64;

/// Brand-colour adherence below this fraction triggers a palette
/// recommendation. Heuristic: when fewer than half the pixels sit on the
/// palette, the asset visibly diverges from the brand.
const MIN_BRAND_ADHERENCE: f64 = 0.5;

/// Image extensions collected by `list_assets`.
const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "svg"];

/// Check the given image files against the guidelines' asset spec for `kind`;
/// returns a human-readable report string.
///
/// Every image is decoded and sampled (at most ~`MAX_SAMPLES` opaque
/// pixels): dominant colors, palette adherence, and whitespace are measured
/// and compared against `guidelines.visual`. Sections are separated by blank
/// lines. A missing file is `BrandiError::NotFound`, an undecodable file is
/// `BrandiError::Image`, and an empty `paths` slice is
/// `BrandiError::Invalid("no images given")`.
pub fn check_assets(
    paths: &[PathBuf],
    kind: &AssetKind,
    guidelines: &Guidelines,
) -> Result<String> {
    if paths.is_empty() {
        return Err(BrandiError::Invalid("no images given".to_string()));
    }
    if paths.len() > MAX_ASSETS_PER_COMMAND {
        return Err(BrandiError::Invalid(format!(
            "asset check accepts at most {MAX_ASSETS_PER_COMMAND} images per command"
        )));
    }
    let palette = guidelines.visual.palette.all_colors();
    let mut sections = Vec::with_capacity(paths.len());
    for path in paths {
        sections.push(check_one(path, kind, guidelines, &palette)?);
    }
    Ok(sections.join("\n\n"))
}

/// List the brand assets discovered under `root`; returns a human-readable
/// listing.
///
/// Walks `root`, pruning anything `crate::surface::is_ignored` rejects
/// (`.git/`, `target/`, ...), and prints one `<relative path> — <WxH|vector>`
/// line per image plus a summary. An empty result reads
/// `no image assets found`.
pub fn list_assets(root: &Path) -> Result<String> {
    let mut assets: Vec<String> = Vec::new();
    let entries = WalkDir::new(root).into_iter().filter_entry(|e| {
        e.path() == root || !crate::surface::is_ignored(rel_path(root, e.path()))
    });
    for entry in entries {
        let entry = entry.map_err(|e| match e.into_io_error() {
            Some(io) => BrandiError::Io(io),
            None => BrandiError::Io(std::io::Error::other("walk error")),
        })?;
        if entry.file_type().is_dir() {
            continue;
        }
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let Some(ext) = ext else { continue };
        if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        let desc = if ext == "svg" {
            "vector".to_string()
        } else {
            let (w, h) = image::image_dimensions(path)?;
            format!("{w}x{h}")
        };
        assets.push(format!("{} — {desc}", rel_path(root, path).display()));
    }
    assets.sort();
    if assets.is_empty() {
        return Ok("no image assets found".to_string());
    }
    let total = assets.len();
    let cap = crate::output::DEFAULT_LIST_CAP;
    if assets.len() > cap {
        assets.truncate(cap);
        assets.push(format!(
            "… +{} more (use --format json for the full list)",
            total - cap
        ));
    }
    Ok(format!("{}\n{total} assets", assets.join("\n")))
}

/// Measured properties of one decoded raster image.
struct ImageAnalysis {
    width: u32,
    height: u32,
    dominant: Vec<(u8, u8, u8)>,
    adherence: f64,
    whitespace: f64,
    /// Average-hash perceptual fingerprint (64 bit) for duplicate detection.
    ahash: u64,
}

/// Decode one raster image and measure it: dimensions, dominant colors,
/// palette adherence, whitespace, and the perceptual hash. Fully transparent
/// pixels carry no brand signal and are skipped; at most ~`MAX_SAMPLES`
/// opaque pixels are sampled.
fn analyze_image(path: &Path, palette: &[(u8, u8, u8)], tolerance: u8) -> Result<ImageAnalysis> {
    // `image::open` decodes fully before anything here gets a chance to
    // apply the MAX_SAMPLES stride, so a maliciously large declared
    // width/height would otherwise force a big allocation regardless of
    // how few pixels are actually sampled afterward. Bound width, height,
    // and total allocation explicitly rather than relying only on the
    // crate's default 512MiB `max_alloc`, which not every decoder honors.
    let bytes = path.metadata()?.len();
    let (declared_width, declared_height) = image::image_dimensions(path)?;
    validate_image_budget(path, bytes, declared_width, declared_height)?;
    let mut reader = image::ImageReader::open(path)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_IMAGE_ALLOCATION_BYTES);
    reader.limits(limits);
    let rgba = reader.decode()?.to_rgba8();
    let (w, h) = rgba.dimensions();

    // Sample pixels with a flat stride chosen so at most ~MAX_SAMPLES pixels
    // are considered.
    let total = u64::from(w) * u64::from(h);
    let step = total.div_ceil(MAX_SAMPLES).max(1) as usize;
    let mut pixels: Vec<(u8, u8, u8)> = Vec::new();
    for px in rgba.pixels().step_by(step) {
        if px[3] == 0 {
            continue;
        }
        pixels.push((px[0], px[1], px[2]));
    }
    let samples = pixels.len() as u64;

    // Dominant colors: quantize each channel to 5 bits, count buckets, keep
    // the top 5 (count desc, color asc for a stable order).
    let mut buckets: HashMap<(u8, u8, u8), u64> = HashMap::new();
    for &c in &pixels {
        *buckets.entry(quantize(c)).or_insert(0) += 1;
    }
    let mut ranked: Vec<((u8, u8, u8), u64)> = buckets.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let most_dominant = ranked.first().map(|(c, _)| *c);
    let dominant: Vec<(u8, u8, u8)> = ranked.iter().take(5).map(|(c, _)| *c).collect();

    // Brand-colour adherence: share of samples within `tolerance`
    // (max per-channel abs diff) of their nearest palette color.
    let within_palette = pixels
        .iter()
        .filter(|&&c| nearest_distance(c, palette).is_some_and(|d| d <= tolerance))
        .count() as u64;
    let adherence = within_palette as f64 / samples.max(1) as f64;

    // Whitespace: share of samples that are near-white, or sit within
    // tolerance of the dominant bucket color *when that color is itself
    // light*. Matching any dominant color regardless of brightness would
    // count a large uniform black background as "whitespace" — this is a
    // breathing-room metric, not a color-uniformity metric.
    let whitespace_count = pixels
        .iter()
        .filter(|&&c| {
            is_near_white(c)
                || most_dominant
                    .is_some_and(|d| is_light_background(d) && channel_distance(c, d) <= tolerance)
        })
        .count() as u64;
    let whitespace = whitespace_count as f64 / samples.max(1) as f64;

    Ok(ImageAnalysis {
        width: w,
        height: h,
        dominant,
        adherence,
        whitespace,
        ahash: ahash(&rgba),
    })
}

fn validate_image_budget(path: &Path, bytes: u64, width: u32, height: u32) -> Result<()> {
    if bytes > MAX_IMAGE_FILE_BYTES {
        return Err(BrandiError::Invalid(format!(
            "{} exceeds the {} byte encoded-image limit",
            path.display(),
            MAX_IMAGE_FILE_BYTES
        )));
    }
    let pixels = u64::from(width) * u64::from(height);
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION || pixels > MAX_IMAGE_PIXELS {
        return Err(BrandiError::Invalid(format!(
            "{} declares {}x{} (limit {} per side and {} pixels)",
            path.display(),
            width,
            height,
            MAX_IMAGE_DIMENSION,
            MAX_IMAGE_PIXELS
        )));
    }
    Ok(())
}

/// Analyze one image and render its report section.
fn check_one(
    path: &Path,
    kind: &AssetKind,
    guidelines: &Guidelines,
    palette: &[(u8, u8, u8)],
) -> Result<String> {
    if !path.exists() {
        return Err(BrandiError::NotFound(path.display().to_string()));
    }
    let tolerance = guidelines.visual.color_tolerance;
    let analysis = analyze_image(path, palette, tolerance)?;
    let (w, h) = (analysis.width, analysis.height);
    let adherence = analysis.adherence;
    let whitespace = analysis.whitespace;

    // Resolve the effective kind: the requested one, or — for `Auto` —
    // the same filename-first classifier `detect`/`audit` use, so a file
    // like `logo-1024.png` resolves the same way regardless of which
    // command looked at it, instead of `check`'s old geometry-only guess
    // silently skipping validation that `audit` would have applied.
    let effective = match kind {
        AssetKind::Auto => {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default();
            asset_class_to_kind(classify_asset(path, w, h, &ext))
        }
        explicit => Some(*explicit),
    };

    // Dimensions are only checked for kinds the guidelines have a spec for.
    let mut dim_line = format!("{w}x{h}");
    let mut dim_failure: Option<String> = None;
    let expected = match effective {
        Some(AssetKind::SocialCard) => Some(&guidelines.visual.assets.social_card),
        Some(AssetKind::Thumbnail) => Some(&guidelines.visual.assets.thumbnail),
        _ => None,
    };
    if let Some(exp) = expected {
        if w == exp.width && h == exp.height {
            dim_line = format!("{dim_line} ✓ (expected {}x{})", exp.width, exp.height);
        } else {
            dim_line = format!("{dim_line} ✗ (expected {}x{})", exp.width, exp.height);
            dim_failure = Some(format!(
                "resize to {}x{} (currently {w}x{h})",
                exp.width, exp.height
            ));
        }
    }

    let min_ws = f64::from(guidelines.visual.assets.min_whitespace);
    let ws_ok = whitespace >= min_ws;
    let ws_line = format!(
        "{}% (min {}%) {}",
        pct(whitespace),
        pct(min_ws),
        if ws_ok { "✓" } else { "✗" }
    );
    let brand_line = format!("{}% within palette", pct(adherence));
    let dominant_line = if analysis.dominant.is_empty() {
        "(none)".to_string()
    } else {
        analysis
            .dominant
            .iter()
            .map(|&c| hex(c))
            .collect::<Vec<_>>()
            .join(" ")
    };

    // One concrete recommendation, only when a metric fails; dimensions
    // first, then palette, then whitespace.
    let recommendation = dim_failure.or_else(|| {
        if adherence < MIN_BRAND_ADHERENCE {
            Some(format!(
                "colors diverge from brand palette ({}% within palette)",
                pct(adherence)
            ))
        } else if !ws_ok {
            Some(format!(
                "increase whitespace (currently {}%, min {}%)",
                pct(whitespace),
                pct(min_ws)
            ))
        } else {
            None
        }
    });

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let mut section = format!(
        "Asset: {name} (kind: {})\n\n{:<13} {}\n{:<13} {}\n{:<13} {}\n{:<13} {}\n\n\
         Manual review: typography, composition, message clarity.",
        kind_label(effective),
        "Dimensions",
        dim_line,
        "Brand colours",
        brand_line,
        "Whitespace",
        ws_line,
        "Dominant",
        dominant_line,
    );
    if let Some(rec) = recommendation {
        section.push_str(&format!("\nRecommendation: {rec}"));
    }
    Ok(section)
}

/// Path of `path` relative to `root` (falls back to `path` itself).
fn rel_path<'a>(root: &Path, path: &'a Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}

/// Quantize a color to 5 bits per channel (keep the top bits).
fn quantize(c: (u8, u8, u8)) -> (u8, u8, u8) {
    (c.0 & 0xF8, c.1 & 0xF8, c.2 & 0xF8)
}

/// Distance between two colors: max per-channel absolute difference.
fn channel_distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> u8 {
    a.0.abs_diff(b.0)
        .max(a.1.abs_diff(b.1))
        .max(a.2.abs_diff(b.2))
}

/// Distance to the nearest palette color (`None` when the palette is empty).
fn nearest_distance(c: (u8, u8, u8), palette: &[(u8, u8, u8)]) -> Option<u8> {
    palette.iter().map(|&p| channel_distance(c, p)).min()
}

/// Near-white heuristic: all channels at or above 240.
fn is_near_white(c: (u8, u8, u8)) -> bool {
    c.0 >= 240 && c.1 >= 240 && c.2 >= 240
}

/// A color bright enough that a viewer would read it as breathing room
/// rather than dense content, if it turns out to be the dominant color.
fn is_light_background(c: (u8, u8, u8)) -> bool {
    (u32::from(c.0) + u32::from(c.1) + u32::from(c.2)) / 3 >= 200
}

/// Format a fraction as a rounded percentage.
fn pct(frac: f64) -> u32 {
    (frac * 100.0).round() as u32
}

/// Format a color as `#rrggbb` (lowercase).
fn hex(c: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// Infer the asset kind from geometry when `Auto` is requested:
/// |aspect − 1200/630| < 0.05 → social card, |aspect − 16/9| < 0.05 →
/// thumbnail, square and ≤ 512px → icon; otherwise unknown (`None`).
fn infer_kind(w: u32, h: u32) -> Option<AssetKind> {
    if h == 0 {
        return None;
    }
    let aspect = f64::from(w) / f64::from(h);
    const SOCIAL: f64 = 1200.0 / 630.0;
    const THUMBNAIL: f64 = 16.0 / 9.0;
    if (aspect - SOCIAL).abs() < 0.05 {
        Some(AssetKind::SocialCard)
    } else if (aspect - THUMBNAIL).abs() < 0.05 {
        Some(AssetKind::Thumbnail)
    } else if w == h && w <= 512 {
        Some(AssetKind::Icon)
    } else {
        None
    }
}

/// Display label for the resolved kind.
fn kind_label(kind: Option<AssetKind>) -> &'static str {
    match kind {
        Some(AssetKind::SocialCard) => "social-card",
        Some(AssetKind::Thumbnail) => "thumbnail",
        Some(AssetKind::Icon) => "icon",
        Some(AssetKind::Auto) | None => "unknown",
    }
}

// ---------------------------------------------------------------------------
// Asset detection, coherence, and utilisation (project-wide audit)
// ---------------------------------------------------------------------------

/// Broad classification of a discovered asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetClass {
    Logo,
    Icon,
    Favicon,
    SocialCard,
    Thumbnail,
    Screenshot,
    Banner,
    Illustration,
    Photo,
    /// No filename or geometry signal; a generic image.
    Image,
}

impl AssetClass {
    pub fn label(self) -> &'static str {
        match self {
            AssetClass::Logo => "logo",
            AssetClass::Icon => "icon",
            AssetClass::Favicon => "favicon",
            AssetClass::SocialCard => "social-card",
            AssetClass::Thumbnail => "thumbnail",
            AssetClass::Screenshot => "screenshot",
            AssetClass::Banner => "banner",
            AssetClass::Illustration => "illustration",
            AssetClass::Photo => "photo",
            AssetClass::Image => "image",
        }
    }
}

/// One image discovered by project-wide detection.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DetectedAsset {
    /// Path relative to the scanned root.
    pub path: String,
    pub class: AssetClass,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub ext: String,
}

/// Text file extensions searched for asset references.
const REFERENCE_EXTENSIONS: [&str; 20] = [
    "md", "markdown", "rst", "html", "htm", "css", "scss", "rs", "ts", "tsx", "js", "jsx", "py",
    "go", "json", "yaml", "yml", "toml", "txt", "svg",
];

/// Hamming distance at or below which two perceptual hashes count as
/// near-duplicates (64-bit aHash: <= ~9% of bits differing).
const NEAR_DUPLICATE_DISTANCE: u32 = 6;

/// Raster images above this many pixels are flagged as very large
/// regardless of class.
const VERY_LARGE_PIXELS: u64 = 12_000_000;

/// Detect and classify every image asset under `root`.
///
/// Walks `root` (pruning `crate::surface::is_ignored` directories), records
/// each image's relative path, class, dimensions (0x0 for SVG vectors),
/// file size in bytes, and extension.
pub fn detect_assets(root: &Path) -> Result<Vec<DetectedAsset>> {
    let mut out = Vec::new();
    let entries = WalkDir::new(root).into_iter().filter_entry(|e| {
        e.path() == root || !crate::surface::is_ignored(rel_path(root, e.path()))
    });
    for entry in entries {
        let entry = entry.map_err(|e| match e.into_io_error() {
            Some(io) => BrandiError::Io(io),
            None => BrandiError::Io(std::io::Error::other("walk error")),
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(ext) = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
        else {
            continue;
        };
        if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        let (w, h) = if ext == "svg" {
            (0, 0)
        } else {
            image::image_dimensions(path).unwrap_or((0, 0))
        };
        let rel = rel_path(root, path);
        out.push(DetectedAsset {
            path: rel.display().to_string(),
            class: classify_asset(rel, w, h, &ext),
            width: w,
            height: h,
            bytes: entry.metadata().map(|m| m.len()).unwrap_or(0),
            ext,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Classify one asset from its filename first, geometry second. Filename
/// tokens win because they carry intent (`logo-mark.png` is a logo whatever
/// its size); geometry is the fallback for untyped names.
fn classify_asset(rel: &Path, w: u32, h: u32, ext: &str) -> AssetClass {
    let name = rel
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&name);
    let has = |token: &str| stem.contains(token);

    if has("favicon") {
        return AssetClass::Favicon;
    }
    if has("logo") {
        return AssetClass::Logo;
    }
    if has("screenshot") || has("screen-shot") {
        return AssetClass::Screenshot;
    }
    if has("social") || has("card") || stem == "og" || has("og-image") || has("og_image") {
        return AssetClass::SocialCard;
    }
    if has("thumb") {
        return AssetClass::Thumbnail;
    }
    if has("banner") || has("hero") {
        return AssetClass::Banner;
    }
    if has("icon") {
        return AssetClass::Icon;
    }
    if has("illustration") {
        return AssetClass::Illustration;
    }
    if has("photo") {
        return AssetClass::Photo;
    }
    // Geometry fallbacks (raster only).
    if w > 0 && h > 0 {
        match infer_kind(w, h) {
            Some(AssetKind::SocialCard) => return AssetClass::SocialCard,
            Some(AssetKind::Thumbnail) => return AssetClass::Thumbnail,
            Some(AssetKind::Icon) => return AssetClass::Icon,
            _ => {}
        }
        if ext == "jpg" || ext == "jpeg" {
            return AssetClass::Photo;
        }
    }
    AssetClass::Image
}

/// Narrow a broad `AssetClass` down to the `AssetKind`s `check` has a
/// dimension spec for; everything else (logo, banner, photo, ...) has
/// nothing to check against and resolves to `None`, same as before.
fn asset_class_to_kind(class: AssetClass) -> Option<AssetKind> {
    match class {
        AssetClass::SocialCard => Some(AssetKind::SocialCard),
        AssetClass::Thumbnail => Some(AssetKind::Thumbnail),
        AssetClass::Icon | AssetClass::Favicon => Some(AssetKind::Icon),
        _ => None,
    }
}

/// One audited asset with its issues.
#[derive(Debug, serde::Serialize)]
pub struct AuditedAsset {
    pub path: String,
    pub class: AssetClass,
    /// "WxH" for raster, "vector" for SVG.
    pub dimensions: String,
    pub bytes: u64,
    /// Number of project text files referencing this asset's basename.
    pub references: usize,
    /// Palette adherence percent; `None` for SVG/undecodable files.
    pub palette_adherence: Option<u32>,
    pub issues: Vec<String>,
}

/// Set-level coherence figures.
#[derive(Debug, serde::Serialize)]
pub struct AuditSummary {
    pub total: usize,
    pub on_brand: usize,
    pub off_brand: usize,
    pub unreferenced: usize,
    pub duplicate_groups: usize,
    pub over_budget: usize,
    /// 100 minus issue penalties, saturating at 0.
    pub score: u32,
}

/// The full project-wide asset audit.
#[derive(Debug, serde::Serialize)]
pub struct AssetAudit {
    pub root: String,
    pub assets: Vec<AuditedAsset>,
    /// Set-level coherence findings (duplicates, icon-set drift).
    pub coherence: Vec<String>,
    pub summary: AuditSummary,
}

/// Audit every asset under `root` for coherence and utilisation.
///
/// Per asset: palette adherence (raster), file size vs
/// `visual.assets.max_file_kb`, resolution fit for its class (social cards
/// and thumbnails must meet the guidelines' dimensions at 1x and should not
/// exceed 2x; icons above 1024px are wasteful), and reference counting
/// across project text files (zero references = unreferenced). Set level:
/// exact duplicates (content hash), near-duplicates (perceptual aHash,
/// Hamming distance <= `NEAR_DUPLICATE_DISTANCE`), and icon-set consistency
/// (shared sizes and formats).
///
/// Score: 100 minus per-issue penalties (off-palette 8, over budget 6,
/// undersized 10, oversized/very large 5, unreferenced 3, each duplicate
/// copy 5, each near-duplicate pair 4, icon-set inconsistency 6),
/// saturating at 0.
pub fn audit_assets(root: &Path, guidelines: &Guidelines) -> Result<AssetAudit> {
    let detected = detect_assets(root)?;
    let references = reference_counts(root, &detected);
    let palette = guidelines.visual.palette.all_colors();
    let tolerance = guidelines.visual.color_tolerance;
    let spec = &guidelines.visual.assets;
    let min_adherence = f64::from(spec.coherence_min_palette) / 100.0;
    let budget = u64::from(spec.max_file_kb) * 1024;

    let mut assets: Vec<AuditedAsset> = Vec::new();
    let mut analyses: Vec<Option<(u64, f64)>> = Vec::new(); // (ahash, adherence) per asset
    let mut penalty: u32 = 0;

    for asset in &detected {
        let mut issues: Vec<(String, u32)> = Vec::new();
        let refs = references.get(&asset.path).copied().unwrap_or(0);

        let mut adherence_pct = None;
        let mut analysis_info = None;
        if asset.ext != "svg" {
            match analyze_image(root.join(&asset.path).as_path(), &palette, tolerance) {
                Ok(a) => {
                    let pct_val = pct(a.adherence);
                    adherence_pct = Some(pct_val);
                    analysis_info = Some((a.ahash, a.adherence));
                    if a.adherence < min_adherence {
                        issues.push((
                            format!(
                                "off-palette ({}% < {}% adherence)",
                                pct_val, spec.coherence_min_palette
                            ),
                            8,
                        ));
                    }
                }
                Err(_) => issues.push(("undecodable image data".to_string(), 5)),
            }

            if asset.bytes > budget {
                issues.push((
                    format!(
                        "over file budget ({} KB > {} KB)",
                        asset.bytes / 1024,
                        spec.max_file_kb
                    ),
                    6,
                ));
            }
            if let Some(res_issue) = resolution_issue(asset, spec) {
                issues.push(res_issue);
            }
        } else if asset.bytes > budget {
            issues.push((
                format!(
                    "over file budget ({} KB > {} KB)",
                    asset.bytes / 1024,
                    spec.max_file_kb
                ),
                6,
            ));
        }
        if refs == 0 {
            issues.push(("unreferenced".to_string(), 3));
        }

        penalty += issues.iter().map(|(_, p)| p).sum::<u32>();
        assets.push(AuditedAsset {
            path: asset.path.clone(),
            class: asset.class,
            dimensions: if asset.ext == "svg" {
                "vector".to_string()
            } else {
                format!("{}x{}", asset.width, asset.height)
            },
            bytes: asset.bytes,
            references: refs,
            palette_adherence: adherence_pct,
            issues: issues.into_iter().map(|(s, _)| s).collect(),
        });
        analyses.push(analysis_info);
    }

    // Set-level coherence.
    let mut coherence: Vec<String> = Vec::new();
    let dup_groups = exact_duplicate_groups(root, &detected);
    for group in &dup_groups {
        coherence.push(format!("duplicate assets: {}", group.join(" = ")));
        penalty += 5 * (group.len() as u32 - 1);
    }
    for (a, b, dist) in near_duplicate_pairs(&detected, &analyses) {
        coherence.push(format!("near duplicate: {a} ≈ {b} (distance {dist})"));
        penalty += 4;
    }
    if let Some(drift) = icon_set_drift(&detected) {
        coherence.push(drift);
        penalty += 6;
    }

    let on_brand = assets
        .iter()
        .filter(|a| {
            a.palette_adherence
                .is_some_and(|p| p >= spec.coherence_min_palette)
        })
        .count();
    let off_brand = assets
        .iter()
        .filter(|a| {
            a.palette_adherence
                .is_some_and(|p| p < spec.coherence_min_palette)
        })
        .count();
    let unreferenced = assets.iter().filter(|a| a.references == 0).count();
    let over_budget = assets.iter().filter(|a| a.bytes > budget).count();

    Ok(AssetAudit {
        root: root.display().to_string(),
        summary: AuditSummary {
            total: assets.len(),
            on_brand,
            off_brand,
            unreferenced,
            duplicate_groups: dup_groups.len(),
            over_budget,
            score: 100u32.saturating_sub(penalty),
        },
        assets,
        coherence,
    })
}

/// Resolution-fit issue for an asset, or `None` when the size is sensible.
/// Returns the issue text with its penalty.
fn resolution_issue(
    asset: &DetectedAsset,
    spec: &crate::guidelines::AssetSpec,
) -> Option<(String, u32)> {
    let (w, h) = (asset.width, asset.height);
    if w == 0 || h == 0 {
        return None;
    }
    let expected = match asset.class {
        AssetClass::SocialCard => Some((spec.social_card.width, spec.social_card.height)),
        AssetClass::Thumbnail => Some((spec.thumbnail.width, spec.thumbnail.height)),
        _ => None,
    };
    if let Some((ew, eh)) = expected {
        if w < ew || h < eh {
            return Some((
                format!(
                    "undersized for {} ({w}x{h} < {ew}x{eh})",
                    asset.class.label()
                ),
                10,
            ));
        }
        if w > ew * 2 || h > eh * 2 {
            return Some((
                format!(
                    "oversized for {} ({w}x{h} > 2x {ew}x{eh})",
                    asset.class.label()
                ),
                5,
            ));
        }
        return None;
    }
    if asset.class == AssetClass::Icon && w > 1024 {
        return Some((format!("oversized for an icon ({w}x{h} > 1024px)"), 5));
    }
    if u64::from(w) * u64::from(h) > VERY_LARGE_PIXELS {
        let mp = (u64::from(w) * u64::from(h)) / 1_000_000;
        return Some((format!("very large ({w}x{h}, {mp} MP)"), 5));
    }
    None
}

/// Count, for each detected asset, how many project text files mention its
/// basename. SVGs are excluded from the search set (an SVG can contain
/// another asset's name without being a real usage).
fn reference_counts(root: &Path, assets: &[DetectedAsset]) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = assets.iter().map(|a| (a.path.clone(), 0)).collect();
    let names: Vec<(&str, &str)> = assets
        .iter()
        .filter_map(|a| {
            a.path
                .rsplit('/')
                .next()
                .map(|base| (a.path.as_str(), base))
        })
        .collect();
    let entries = WalkDir::new(root).into_iter().filter_entry(|e| {
        e.path() == root || !crate::surface::is_ignored(rel_path(root, e.path()))
    });
    for entry in entries.filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if !REFERENCE_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        // Never count an asset's own name inside itself.
        let rel = rel_path(root, path).display().to_string();
        if counts.contains_key(&rel) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        for (asset_path, basename) in &names {
            if content.contains(basename) {
                *counts.entry(asset_path.to_string()).or_insert(0) += 1;
            }
        }
    }
    counts
}

/// Groups of assets with byte-identical content (FNV-1a over file bytes).
fn exact_duplicate_groups(root: &Path, assets: &[DetectedAsset]) -> Vec<Vec<String>> {
    let mut by_hash: HashMap<u64, Vec<String>> = HashMap::new();
    for asset in assets {
        let Ok(bytes) = std::fs::read(root.join(&asset.path)) else {
            continue;
        };
        by_hash
            .entry(fnv1a(&bytes))
            .or_default()
            .push(asset.path.clone());
    }
    let mut groups: Vec<Vec<String>> = by_hash.into_values().filter(|g| g.len() > 1).collect();
    groups.sort();
    groups
}

/// Pairs of raster assets whose perceptual hashes are within
/// `NEAR_DUPLICATE_DISTANCE`, excluding byte-identical files (those are
/// reported as duplicates). Flat images hash degenerately (all-0/all-1
/// bits) and are skipped — two unrelated solid-color images would
/// otherwise always pair.
fn near_duplicate_pairs(
    assets: &[DetectedAsset],
    analyses: &[Option<(u64, f64)>],
) -> Vec<(String, String, u32)> {
    let mut pairs = Vec::new();
    for i in 0..assets.len() {
        let Some((ha, _)) = analyses[i] else { continue };
        if ha == 0 || ha == u64::MAX {
            continue;
        }
        for j in (i + 1)..assets.len() {
            let Some((hb, _)) = analyses[j] else { continue };
            if hb == 0 || hb == u64::MAX {
                continue;
            }
            let dist = (ha ^ hb).count_ones();
            if dist <= NEAR_DUPLICATE_DISTANCE {
                pairs.push((assets[i].path.clone(), assets[j].path.clone(), dist));
            }
        }
    }
    pairs
}

/// Icon-set drift: icons that mix sizes or formats look assembled, not
/// designed. `None` when the set (0 or 1 icon) is trivially consistent.
fn icon_set_drift(assets: &[DetectedAsset]) -> Option<String> {
    let icons: Vec<&DetectedAsset> = assets
        .iter()
        .filter(|a| {
            matches!(
                a.class,
                AssetClass::Icon | AssetClass::Favicon | AssetClass::Logo
            )
        })
        .collect();
    if icons.len() < 2 {
        return None;
    }
    let mut sizes: Vec<String> = icons
        .iter()
        .filter(|a| a.width > 0)
        .map(|a| format!("{}x{}", a.width, a.height))
        .collect();
    sizes.sort();
    sizes.dedup();
    let mut exts: Vec<&str> = icons.iter().map(|a| a.ext.as_str()).collect();
    exts.sort();
    exts.dedup();
    let mut drift: Vec<String> = Vec::new();
    if sizes.len() > 1 {
        drift.push(format!("sizes {}", sizes.join(", ")));
    }
    if exts.len() > 1 {
        drift.push(format!("formats {}", exts.join(", ")));
    }
    if drift.is_empty() {
        None
    } else {
        Some(format!(
            "icon/logo set is inconsistent: {}",
            drift.join("; ")
        ))
    }
}

/// Average-hash perceptual fingerprint: 8x8 luma thumbnail, one bit per
/// pixel (1 = at or above the mean).
fn ahash(rgba: &image::RgbaImage) -> u64 {
    let small = image::imageops::resize(rgba, 8, 8, image::imageops::FilterType::Triangle);
    let mut lumas = [0u16; 64];
    let mut sum = 0u32;
    for (i, px) in small.pixels().enumerate() {
        // ITU-R BT.601 luma.
        let l = (299 * u32::from(px[0]) + 587 * u32::from(px[1]) + 114 * u32::from(px[2])) / 1000;
        lumas[i] = l as u16;
        sum += l;
    }
    let mean = (sum / 64) as u16;
    let mut hash = 0u64;
    for (i, &l) in lumas.iter().enumerate() {
        if l >= mean {
            hash |= 1 << i;
        }
    }
    hash
}

/// FNV-1a 64-bit hash of file content for exact duplicate detection.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Render detected assets as a table (`brandi assets detect`).
pub fn render_detected(assets: &[DetectedAsset]) -> String {
    if assets.is_empty() {
        return "no image assets found".to_string();
    }
    let mut lines: Vec<String> = assets
        .iter()
        .map(|a| {
            let dims = if a.ext == "svg" {
                "vector".to_string()
            } else {
                format!("{}x{}", a.width, a.height)
            };
            format!(
                "{} — {} · {} · {} KB",
                a.path,
                a.class.label(),
                dims,
                a.bytes / 1024
            )
        })
        .collect();
    let raster = assets.iter().filter(|a| a.ext != "svg").count();
    lines.push(format!(
        "{} assets ({} raster, {} vector)",
        assets.len(),
        raster,
        assets.len() - raster
    ));
    lines.join("\n")
}

/// Render a full audit for humans (`brandi assets audit`).
pub fn render_audit(audit: &AssetAudit) -> String {
    if audit.assets.is_empty() {
        return format!("Asset audit — {}\n\nno image assets found", audit.root);
    }
    let mut out = format!("Asset audit — {}\n", audit.root);
    let cap = crate::output::DEFAULT_LIST_CAP;
    for asset in audit.assets.iter().take(cap) {
        let palette = asset
            .palette_adherence
            .map(|p| format!(" · palette {p}%"))
            .unwrap_or_default();
        out.push_str(&format!(
            "\n{} — {} · {} · {} KB · {} refs{palette}",
            asset.path,
            asset.class.label(),
            asset.dimensions,
            asset.bytes / 1024,
            asset.references,
        ));
        for issue in &asset.issues {
            out.push_str(&format!("\n  ⚠ {issue}"));
        }
        out.push('\n');
    }
    if audit.assets.len() > cap {
        out.push_str(&format!(
            "\n… +{} more (use --format json for the full list)\n",
            audit.assets.len() - cap
        ));
    }
    if !audit.coherence.is_empty() {
        out.push_str("\nCoherence\n");
        for finding in &audit.coherence {
            out.push_str(&format!("  ⚠ {finding}\n"));
        }
    }
    let s = &audit.summary;
    out.push_str(&format!(
        "\nSummary: {} assets · {} on-brand · {} off-brand · {} unreferenced · {} duplicate groups · {} over budget\n\
         Asset coherence: {}/100",
        s.total, s.on_brand, s.off_brand, s.unreferenced, s.duplicate_groups, s.over_budget, s.score
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scaffold guidelines in `dir` and load them back.
    fn test_guidelines(dir: &Path) -> Guidelines {
        Guidelines::scaffold(dir).unwrap();
        Guidelines::load(dir).unwrap()
    }

    /// Save a solid-color PNG into `dir` and return its path.
    fn save_solid(dir: &Path, name: &str, w: u32, h: u32, rgb: [u8; 3]) -> PathBuf {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb(rgb));
        let path = dir.join(name);
        img.save(&path).unwrap();
        path
    }

    /// The report line starting with `label`.
    fn line<'a>(report: &'a str, label: &str) -> &'a str {
        report
            .lines()
            .find(|l| l.starts_with(label))
            .unwrap_or_else(|| panic!("no line starting with {label:?} in:\n{report}"))
    }

    #[test]
    fn solid_brand_color_full_adherence_but_zero_whitespace() {
        // A saturated solid-color fill is 100% on-palette but has no actual
        // breathing room; it must not be reported as "whitespace" just
        // because every pixel matches the single dominant color.
        let tmp = tempfile::tempdir().unwrap();
        let guidelines = test_guidelines(tmp.path());
        let img = save_solid(tmp.path(), "icon.png", 64, 64, [0xFF, 0x2D, 0xAA]);
        let out = check_assets(&[img], &AssetKind::Auto, &guidelines).unwrap();
        assert!(out.contains("(kind: icon)"), "got:\n{out}");
        assert!(
            line(&out, "Brand colours").contains("100% within palette"),
            "got:\n{out}"
        );
        assert!(
            line(&out, "Whitespace").contains("0% (min 25%) ✗"),
            "got:\n{out}"
        );
        assert!(out.contains("increase whitespace"), "got:\n{out}");
    }

    #[test]
    fn dark_dominant_background_does_not_count_as_whitespace() {
        // Regression test: the whitespace metric used to be "closeness to
        // the single most common pixel color" with no brightness check, so
        // a dark-themed screenshot with a large uniform black background
        // could pass a min_whitespace gate meant to enforce real breathing
        // room.
        let tmp = tempfile::tempdir().unwrap();
        let guidelines = test_guidelines(tmp.path());
        let img = save_solid(tmp.path(), "dark.png", 64, 64, [0x10, 0x10, 0x10]);
        let out = check_assets(&[img], &AssetKind::Auto, &guidelines).unwrap();
        assert!(
            line(&out, "Whitespace").contains("0% (min 25%) ✗"),
            "got:\n{out}"
        );
    }

    #[test]
    fn off_palette_recommends_palette() {
        let tmp = tempfile::tempdir().unwrap();
        let guidelines = test_guidelines(tmp.path());
        let img = save_solid(tmp.path(), "red.png", 64, 64, [255, 0, 0]);
        let out = check_assets(&[img], &AssetKind::Auto, &guidelines).unwrap();
        assert!(
            line(&out, "Brand colours").contains("0% within palette"),
            "got:\n{out}"
        );
        assert!(
            out.contains("Recommendation: colors diverge from brand palette"),
            "got:\n{out}"
        );
    }

    #[test]
    fn white_with_small_square_high_whitespace() {
        let tmp = tempfile::tempdir().unwrap();
        let guidelines = test_guidelines(tmp.path());
        let mut img = image::RgbImage::from_pixel(100, 100, image::Rgb([255, 255, 255]));
        for y in 0..10 {
            for x in 0..10 {
                img.put_pixel(x, y, image::Rgb([0xFF, 0x2D, 0xAA]));
            }
        }
        let path = tmp.path().join("mostly-white.png");
        img.save(&path).unwrap();
        let out = check_assets(&[path], &AssetKind::Auto, &guidelines).unwrap();
        assert!(
            line(&out, "Whitespace").contains("(min 25%) ✓"),
            "got:\n{out}"
        );
        assert!(!out.contains("increase whitespace"), "got:\n{out}");
    }

    #[test]
    fn infers_social_card_and_checks_dimensions() {
        let tmp = tempfile::tempdir().unwrap();
        let guidelines = test_guidelines(tmp.path());
        let img = save_solid(tmp.path(), "card.png", 1200, 630, [0xFF, 0x2D, 0xAA]);
        let out = check_assets(&[img], &AssetKind::Auto, &guidelines).unwrap();
        assert!(out.contains("(kind: social-card)"), "got:\n{out}");
        assert_eq!(
            line(&out, "Dimensions"),
            "Dimensions    1200x630 ✓ (expected 1200x630)"
        );
    }

    #[test]
    fn non_standard_dimensions_stay_unknown_without_checkmark() {
        let tmp = tempfile::tempdir().unwrap();
        let guidelines = test_guidelines(tmp.path());
        let img = save_solid(tmp.path(), "misc.png", 800, 600, [0xFF, 0x2D, 0xAA]);
        let out = check_assets(&[img], &AssetKind::Auto, &guidelines).unwrap();
        assert!(out.contains("(kind: unknown)"), "got:\n{out}");
        assert_eq!(line(&out, "Dimensions"), "Dimensions    800x600");
    }

    #[test]
    fn infers_thumbnail_and_checks_dimensions() {
        let tmp = tempfile::tempdir().unwrap();
        let guidelines = test_guidelines(tmp.path());
        let img = save_solid(tmp.path(), "thumb.png", 1280, 720, [0xFF, 0x2D, 0xAA]);
        let out = check_assets(&[img], &AssetKind::Auto, &guidelines).unwrap();
        assert!(out.contains("(kind: thumbnail)"), "got:\n{out}");
        assert_eq!(
            line(&out, "Dimensions"),
            "Dimensions    1280x720 ✓ (expected 1280x720)"
        );
    }

    #[test]
    fn auto_kind_uses_filename_first_classification_like_audit_does() {
        // Regression test: `check --kind auto` used to infer kind from
        // geometry alone, so a file whose name clearly says what it is but
        // whose dimensions don't match the aspect-ratio heuristic (a
        // resized/cropped social card, say) silently skipped dimension
        // validation entirely — while `detect`/`audit` would have
        // classified it correctly from the filename. Both must now agree.
        let tmp = tempfile::tempdir().unwrap();
        let guidelines = test_guidelines(tmp.path());
        let img = save_solid(
            tmp.path(),
            "social-card-preview.png",
            800,
            600,
            [0xFF, 0x2D, 0xAA],
        );
        let out = check_assets(&[img], &AssetKind::Auto, &guidelines).unwrap();
        assert!(out.contains("(kind: social-card)"), "got:\n{out}");
        assert_eq!(
            line(&out, "Dimensions"),
            "Dimensions    800x600 ✗ (expected 1200x630)",
            "got:\n{out}"
        );
        assert!(out.contains("resize to 1200x630"), "got:\n{out}");

        let detected_class = classify_asset(Path::new("social-card-preview.png"), 800, 600, "png");
        assert_eq!(detected_class, AssetClass::SocialCard);
    }

    #[test]
    fn explicit_kind_checks_dims_and_recommends_resize() {
        let tmp = tempfile::tempdir().unwrap();
        let guidelines = test_guidelines(tmp.path());
        let img = save_solid(tmp.path(), "small.png", 100, 100, [0xFF, 0x2D, 0xAA]);
        let out = check_assets(&[img], &AssetKind::SocialCard, &guidelines).unwrap();
        assert!(out.contains("(kind: social-card)"), "got:\n{out}");
        assert_eq!(
            line(&out, "Dimensions"),
            "Dimensions    100x100 ✗ (expected 1200x630)"
        );
        assert!(
            out.contains("Recommendation: resize to 1200x630 (currently 100x100)"),
            "got:\n{out}"
        );
    }

    #[test]
    fn fully_transparent_image_reports_no_samples() {
        let tmp = tempfile::tempdir().unwrap();
        let guidelines = test_guidelines(tmp.path());
        let img = image::RgbaImage::from_pixel(32, 32, image::Rgba([0, 0, 0, 0]));
        let path = tmp.path().join("transparent.png");
        img.save(&path).unwrap();
        let out = check_assets(&[path], &AssetKind::Auto, &guidelines).unwrap();
        assert!(line(&out, "Dominant").contains("(none)"), "got:\n{out}");
        assert!(
            line(&out, "Brand colours").contains("0% within palette"),
            "got:\n{out}"
        );
    }

    #[test]
    fn missing_file_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let guidelines = test_guidelines(tmp.path());
        let missing = tmp.path().join("nope.png");
        match check_assets(&[missing], &AssetKind::Auto, &guidelines) {
            Err(BrandiError::NotFound(msg)) => assert!(msg.contains("nope.png"), "got: {msg}"),
            other => panic!("expected BrandiError::NotFound, got {other:?}"),
        }
    }

    #[test]
    fn empty_paths_is_invalid_error() {
        let tmp = tempfile::tempdir().unwrap();
        let guidelines = test_guidelines(tmp.path());
        match check_assets(&[], &AssetKind::Auto, &guidelines) {
            Err(BrandiError::Invalid(msg)) => assert_eq!(msg, "no images given"),
            other => panic!("expected BrandiError::Invalid, got {other:?}"),
        }
    }

    #[test]
    fn undecodable_file_is_image_error() {
        let tmp = tempfile::tempdir().unwrap();
        let guidelines = test_guidelines(tmp.path());
        let path = tmp.path().join("fake.png");
        std::fs::write(&path, b"this is not a png").unwrap();
        match check_assets(&[path], &AssetKind::Auto, &guidelines) {
            Err(BrandiError::Image(_)) => {}
            other => panic!("expected BrandiError::Image, got {other:?}"),
        }
    }

    #[test]
    fn list_assets_finds_images_and_skips_ignored_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        save_solid(tmp.path(), "logo.png", 32, 32, [0xFF, 0x2D, 0xAA]);
        std::fs::write(
            tmp.path().join("icon.svg"),
            r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#,
        )
        .unwrap();
        std::fs::create_dir_all(tmp.path().join("target")).unwrap();
        save_solid(tmp.path(), "target/hidden.png", 32, 32, [0, 0, 0]);

        let out = list_assets(tmp.path()).unwrap();
        assert!(out.contains("logo.png — 32x32"), "got:\n{out}");
        assert!(out.contains("icon.svg — vector"), "got:\n{out}");
        assert!(!out.contains("hidden.png"), "got:\n{out}");
        assert!(
            line(&out, "2 assets").starts_with("2 assets"),
            "got:\n{out}"
        );
    }

    #[test]
    fn list_assets_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(list_assets(tmp.path()).unwrap(), "no image assets found");
    }

    #[test]
    fn image_budget_rejects_encoded_dimension_and_pixel_bombs() {
        let path = Path::new("bomb.png");
        assert!(validate_image_budget(path, MAX_IMAGE_FILE_BYTES + 1, 1, 1).is_err());
        assert!(validate_image_budget(path, 1, MAX_IMAGE_DIMENSION + 1, 1).is_err());
        assert!(validate_image_budget(path, 1, 8_000, 8_000).is_err());
        assert!(validate_image_budget(path, 1, 1_200, 630).is_ok());
    }

    #[test]
    fn asset_batch_is_bounded() {
        let tmp = tempfile::tempdir().unwrap();
        let guidelines = test_guidelines(tmp.path());
        let paths = (0..=MAX_ASSETS_PER_COMMAND)
            .map(|index| PathBuf::from(format!("image-{index}.png")))
            .collect::<Vec<_>>();
        assert!(check_assets(&paths, &AssetKind::Auto, &guidelines).is_err());
    }
}
