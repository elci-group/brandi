//! Digital signage (out-of-home) campaign planning with real-time
//! price-quoted budgeting.
//!
//! Currently supports the caasie.io cost-per-play strategy for the UK (GB)
//! market. Budgets are built from per-board price quotes — live when a
//! credential is available, configured estimates under `--preflight`.

use crate::automation::{curl_config_line, curl_with_config};
use crate::error::{BrandiError, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SIGNAGE_PLAN_SCHEMA: &str = "brandi-signage-plan-v1";
pub const SIGNAGE_VALIDATION_SCHEMA: &str = "brandi-signage-validation-v1";
const CAASIE_QUOTES_SCHEMA: &str = "caasie-quotes-v1";
const DEFAULT_CAASIE_API_BASE: &str = "https://api.caasie.co";
const DEFAULT_CREDENTIAL_ENV: &str = "CAASIE_API_TOKEN";
const DEFAULT_ESTIMATE_GBP_PER_PLAY: f64 = 0.05;
const SUPPORTED_MARKET: &str = "GB";

/// DOOH buying strategy. Only caasie is supported in this pass.
#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[value(rename_all = "kebab-case")]
pub enum SignageStrategy {
    #[default]
    Caasie,
}

impl fmt::Display for SignageStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SignageStrategy::Caasie => "caasie",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SignageConfig {
    /// ISO country code of the target market; only `GB` is enabled.
    pub market: String,
    pub objective: String,
    pub dates: CampaignDates,
    /// Hard ceiling in GBP; also settable per-run via `--max-budget-gbp`.
    pub max_budget_gbp: Option<f64>,
    pub boards: Vec<BoardSpec>,
    pub caasie: CaasieConfig,
}

impl Default for SignageConfig {
    fn default() -> Self {
        Self {
            market: SUPPORTED_MARKET.into(),
            objective: String::new(),
            dates: CampaignDates::default(),
            max_budget_gbp: None,
            boards: Vec::new(),
            caasie: CaasieConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CampaignDates {
    /// Inclusive campaign start, `YYYY-MM-DD`.
    pub start: String,
    /// Inclusive campaign end, `YYYY-MM-DD`.
    pub end: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BoardSpec {
    /// Vendor board identifier.
    pub board_id: String,
    /// Venue class, e.g. `bus-shelter`, `retail`, `roadside`.
    pub venue: String,
    pub city: String,
    pub plays_per_day: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CaasieConfig {
    /// Quote endpoint base; caasie does not publish a fixed public API, so
    /// the deployment-specific base stays configurable.
    pub api_base: String,
    /// Name of the environment variable holding the API credential; the
    /// value itself is never persisted.
    pub credential_env: String,
    /// Per-board GBP estimate used by `--preflight` instead of live quotes.
    pub estimate_price_gbp_per_play: f64,
}

impl Default for CaasieConfig {
    fn default() -> Self {
        Self {
            api_base: DEFAULT_CAASIE_API_BASE.into(),
            credential_env: DEFAULT_CREDENTIAL_ENV.into(),
            estimate_price_gbp_per_play: DEFAULT_ESTIMATE_GBP_PER_PLAY,
        }
    }
}

impl SignageConfig {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(".brandi/signage.yaml");
        if !path.exists() {
            return Err(BrandiError::NotFound(format!(
                "no signage configuration at {}; create it (see examples/signage.yaml)",
                path.display()
            )));
        }
        let mut config: Self = serde_yaml::from_str(&fs::read_to_string(&path)?)?;
        if config.market.is_empty() {
            config.market = SUPPORTED_MARKET.into();
        }
        if config.caasie.api_base.is_empty() {
            config.caasie.api_base = DEFAULT_CAASIE_API_BASE.into();
        }
        if config.caasie.credential_env.is_empty() {
            config.caasie.credential_env = DEFAULT_CREDENTIAL_ENV.into();
        }
        if config.caasie.estimate_price_gbp_per_play <= 0.0 {
            config.caasie.estimate_price_gbp_per_play = DEFAULT_ESTIMATE_GBP_PER_PLAY;
        }
        Ok(config)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Quote {
    pub board_id: String,
    pub currency: String,
    pub price_per_play: f64,
    pub quoted_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlannedBoard {
    pub board: BoardSpec,
    pub plays: u64,
    pub price_per_play_gbp: f64,
    pub subtotal_gbp: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignagePlan {
    pub schema_version: &'static str,
    pub market: String,
    pub strategy: String,
    pub objective: String,
    pub start_date: String,
    pub end_date: String,
    pub days: u64,
    pub boards: Vec<PlannedBoard>,
    pub total_plays: u64,
    pub total_gbp: f64,
    pub quote_source: String,
    pub quoted_at: String,
    pub max_budget_gbp: Option<f64>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignageValidation {
    pub schema_version: &'static str,
    pub valid: bool,
    pub errors: Vec<String>,
    pub market: String,
    pub board_count: usize,
    pub days: Option<u64>,
    pub credential_env: String,
    pub credential_present: bool,
    pub max_budget_gbp: Option<f64>,
}

fn now_iso() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    chrono::DateTime::from_timestamp(secs, 0)
        .unwrap_or(chrono::DateTime::UNIX_EPOCH)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn round_pence(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn campaign_days(config: &SignageConfig) -> std::result::Result<u64, String> {
    let start = chrono::NaiveDate::parse_from_str(&config.dates.start, "%Y-%m-%d")
        .map_err(|_| format!("dates.start is not YYYY-MM-DD: {:?}", config.dates.start))?;
    let end = chrono::NaiveDate::parse_from_str(&config.dates.end, "%Y-%m-%d")
        .map_err(|_| format!("dates.end is not YYYY-MM-DD: {:?}", config.dates.end))?;
    if end < start {
        return Err(format!(
            "dates.end ({}) is before dates.start ({})",
            config.dates.end, config.dates.start
        ));
    }
    Ok((end - start).num_days() as u64 + 1)
}

fn validate_config(config: &SignageConfig) -> Vec<String> {
    let mut errors = Vec::new();
    if config.market != SUPPORTED_MARKET {
        errors.push(format!(
            "market {:?} is not supported by the caasie strategy in this version (supported: {SUPPORTED_MARKET})",
            config.market
        ));
    }
    if let Err(message) = campaign_days(config) {
        errors.push(message);
    }
    if config.boards.is_empty() {
        errors.push("boards is empty; configure at least one board".into());
    }
    let mut seen = std::collections::HashSet::new();
    for board in &config.boards {
        if board.board_id.is_empty() {
            errors.push("a board has an empty board_id".into());
        } else if !seen.insert(board.board_id.clone()) {
            errors.push(format!("duplicate board_id {:?}", board.board_id));
        }
        if board.plays_per_day == 0 {
            errors.push(format!("board {:?} has plays_per_day of 0", board.board_id));
        }
    }
    if let Some(ceiling) = config.max_budget_gbp {
        if !ceiling.is_finite() || ceiling <= 0.0 {
            errors.push(format!("max_budget_gbp must be positive, got {ceiling}"));
        }
    }
    errors
}

/// Validate configuration without any network access and report credential
/// presence by environment-variable name only.
pub fn validate(root: &Path) -> Result<SignageValidation> {
    let config = match SignageConfig::load(root) {
        Ok(config) => config,
        Err(error) => {
            return Ok(SignageValidation {
                schema_version: SIGNAGE_VALIDATION_SCHEMA,
                valid: false,
                errors: vec![error.to_string()],
                market: String::new(),
                board_count: 0,
                days: None,
                credential_env: DEFAULT_CREDENTIAL_ENV.into(),
                credential_present: false,
                max_budget_gbp: None,
            })
        }
    };
    let errors = validate_config(&config);
    Ok(SignageValidation {
        schema_version: SIGNAGE_VALIDATION_SCHEMA,
        valid: errors.is_empty(),
        errors,
        market: config.market.clone(),
        board_count: config.boards.len(),
        days: campaign_days(&config).ok(),
        credential_env: config.caasie.credential_env.clone(),
        credential_present: std::env::var(&config.caasie.credential_env).is_ok(),
        max_budget_gbp: config.max_budget_gbp,
    })
}

/// Strictly parse a caasie quote response, requiring one finite, non-negative
/// GBP price for every requested board.
fn parse_quotes(body: &[u8], board_ids: &[String]) -> Result<Vec<Quote>> {
    let response: serde_json::Value = serde_json::from_slice(body)
        .map_err(|e| BrandiError::Network(format!("invalid caasie quote response JSON: {e}")))?;
    if response["schema_version"].as_str() != Some(CAASIE_QUOTES_SCHEMA) {
        return Err(BrandiError::Network(format!(
            "caasie quote response has unsupported schema_version {:?} (want {CAASIE_QUOTES_SCHEMA})",
            response["schema_version"]
        )));
    }
    if response["currency"].as_str() != Some("GBP") {
        return Err(BrandiError::Network(format!(
            "caasie quote response currency must be GBP, got {:?}",
            response["currency"]
        )));
    }
    let quoted_at = response["quoted_at"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(now_iso);
    let entries = response["quotes"]
        .as_array()
        .ok_or_else(|| BrandiError::Network("caasie quote response has no quotes array".into()))?;
    let mut quotes = Vec::with_capacity(entries.len());
    for entry in entries {
        let board_id = entry["board_id"]
            .as_str()
            .ok_or_else(|| BrandiError::Network("caasie quote entry missing board_id".into()))?;
        let price = entry["price_per_play"].as_f64().ok_or_else(|| {
            BrandiError::Network(format!(
                "caasie quote for {board_id} missing price_per_play"
            ))
        })?;
        if !price.is_finite() || price < 0.0 {
            return Err(BrandiError::Network(format!(
                "caasie quote for {board_id} has invalid price_per_play {price}"
            )));
        }
        quotes.push(Quote {
            board_id: board_id.to_string(),
            currency: "GBP".into(),
            price_per_play: price,
            quoted_at: quoted_at.clone(),
        });
    }
    for board_id in board_ids {
        if !quotes.iter().any(|quote| &quote.board_id == board_id) {
            return Err(BrandiError::Network(format!(
                "caasie quote response is missing board {board_id:?}"
            )));
        }
    }
    Ok(quotes)
}

/// Fetch live per-play GBP quotes for the configured boards. Secrets travel
/// through a piped `curl -K` config, never through argv.
fn fetch_quotes(config: &SignageConfig, credential: &str) -> Result<Vec<Quote>> {
    let board_ids: Vec<String> = config
        .boards
        .iter()
        .map(|board| board.board_id.clone())
        .collect();
    let body = serde_json::json!({
        "market": config.market,
        "boards": board_ids,
    })
    .to_string();
    let url = format!("{}/v1/quotes", config.caasie.api_base.trim_end_matches('/'));
    let mut curl_cfg = String::new();
    curl_cfg += &curl_config_line("url", &url);
    curl_cfg += &curl_config_line("header", "Content-Type: application/json");
    curl_cfg += &curl_config_line("header", &format!("Authorization: Bearer {credential}"));
    curl_cfg += &curl_config_line("data-binary", &body);
    let output = curl_with_config(&["-sS", "--fail-with-body", "-X", "POST"], &curl_cfg)
        .ok_or_else(|| BrandiError::Network("caasie quote request failed or timed out".into()))?;
    if !output.status.success() {
        return Err(BrandiError::Network(format!(
            "caasie quote request returned status {}; {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    parse_quotes(&output.stdout, &board_ids)
}

/// Build a signage plan. With `preflight`, per-board estimates are used and
/// no network access happens; otherwise live caasie quotes price the budget.
pub fn build_plan(
    config: &SignageConfig,
    strategy: SignageStrategy,
    max_budget_gbp: Option<f64>,
    preflight: bool,
) -> Result<SignagePlan> {
    match strategy {
        SignageStrategy::Caasie => {}
    }
    let errors = validate_config(config);
    if !errors.is_empty() {
        return Err(BrandiError::Invalid(format!(
            "invalid signage configuration: {}",
            errors.join("; ")
        )));
    }
    let days = campaign_days(config).map_err(BrandiError::Invalid)?;
    let ceiling = max_budget_gbp.or(config.max_budget_gbp);
    if let Some(ceiling) = ceiling {
        if !ceiling.is_finite() || ceiling <= 0.0 {
            return Err(BrandiError::Invalid(format!(
                "max budget must be a positive GBP amount, got {ceiling}"
            )));
        }
    }

    let (quotes, quote_source, mut warnings) = if preflight {
        let quoted_at = now_iso();
        (
            config
                .boards
                .iter()
                .map(|board| Quote {
                    board_id: board.board_id.clone(),
                    currency: "GBP".into(),
                    price_per_play: config.caasie.estimate_price_gbp_per_play,
                    quoted_at: quoted_at.clone(),
                })
                .collect::<Vec<_>>(),
            "estimate".to_string(),
            vec![
                "prices are configured estimates; run without --preflight for live caasie quotes"
                    .to_string(),
            ],
        )
    } else {
        let credential_env = &config.caasie.credential_env;
        let credential = std::env::var(credential_env).map_err(|_| {
            BrandiError::Invalid(format!(
                "live caasie quotes need the {credential_env} environment variable; set it or pass --preflight"
            ))
        })?;
        (
            fetch_quotes(config, &credential)?,
            "live".to_string(),
            Vec::new(),
        )
    };

    let mut boards = Vec::with_capacity(config.boards.len());
    let mut total_plays = 0u64;
    let mut total = 0.0f64;
    for spec in &config.boards {
        let quote = quotes
            .iter()
            .find(|quote| quote.board_id == spec.board_id)
            .ok_or_else(|| {
                BrandiError::Network(format!("no quote returned for board {:?}", spec.board_id))
            })?;
        let plays = days * spec.plays_per_day as u64;
        let subtotal = round_pence(quote.price_per_play * plays as f64);
        total += subtotal;
        total_plays += plays;
        boards.push(PlannedBoard {
            board: spec.clone(),
            plays,
            price_per_play_gbp: quote.price_per_play,
            subtotal_gbp: subtotal,
        });
    }
    let total_gbp = round_pence(total);
    if let Some(ceiling) = ceiling {
        if total_gbp > ceiling {
            return Err(BrandiError::Invalid(format!(
                "quoted total £{total_gbp:.2} exceeds the maximum budget £{ceiling:.2} by £{:.2}",
                total_gbp - ceiling
            )));
        }
    }
    let quoted_at = quotes
        .first()
        .map(|quote| quote.quoted_at.clone())
        .unwrap_or_else(now_iso);
    Ok(SignagePlan {
        schema_version: SIGNAGE_PLAN_SCHEMA,
        market: config.market.clone(),
        strategy: strategy.to_string(),
        objective: config.objective.clone(),
        start_date: config.dates.start.clone(),
        end_date: config.dates.end.clone(),
        days,
        boards,
        total_plays,
        total_gbp,
        quote_source,
        quoted_at,
        max_budget_gbp: ceiling,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> SignageConfig {
        SignageConfig {
            objective: "UK launch".into(),
            dates: CampaignDates {
                start: "2026-10-01".into(),
                end: "2026-10-07".into(),
            },
            max_budget_gbp: None,
            boards: vec![
                BoardSpec {
                    board_id: "gb-lon-001".into(),
                    venue: "bus-shelter".into(),
                    city: "London".into(),
                    plays_per_day: 100,
                },
                BoardSpec {
                    board_id: "gb-mnc-002".into(),
                    venue: "retail".into(),
                    city: "Manchester".into(),
                    plays_per_day: 50,
                },
            ],
            ..SignageConfig::default()
        }
    }

    fn write_config(root: &Path, yaml: &str) {
        fs::create_dir_all(root.join(".brandi")).unwrap();
        fs::write(root.join(".brandi/signage.yaml"), yaml).unwrap();
    }

    #[test]
    fn load_applies_defaults_and_parses_boards() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            r#"
objective: UK launch
dates: { start: 2026-10-01, end: 2026-10-03 }
boards:
  - board_id: gb-lon-001
    venue: bus-shelter
    city: London
    plays_per_day: 100
"#,
        );
        let config = SignageConfig::load(dir.path()).unwrap();
        assert_eq!(config.market, "GB");
        assert_eq!(config.boards.len(), 1);
        assert_eq!(config.boards[0].board_id, "gb-lon-001");
        assert_eq!(config.caasie.api_base, DEFAULT_CAASIE_API_BASE);
        assert_eq!(config.caasie.credential_env, DEFAULT_CREDENTIAL_ENV);
        assert_eq!(config.caasie.estimate_price_gbp_per_play, 0.05);
    }

    #[test]
    fn load_errors_when_configuration_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            SignageConfig::load(dir.path()),
            Err(BrandiError::NotFound(_))
        ));
    }

    #[test]
    fn preflight_plan_priced_from_estimates_without_credentials() {
        let config = test_config();
        let plan = build_plan(&config, SignageStrategy::Caasie, None, true).unwrap();
        assert_eq!(plan.schema_version, SIGNAGE_PLAN_SCHEMA);
        assert_eq!(plan.quote_source, "estimate");
        assert_eq!(plan.days, 7);
        // gb-lon-001: 7d * 100 plays * £0.05 = £35.00
        assert_eq!(plan.boards[0].plays, 700);
        assert_eq!(plan.boards[0].subtotal_gbp, 35.0);
        // gb-mnc-002: 7d * 50 plays * £0.05 = £17.50
        assert_eq!(plan.boards[1].subtotal_gbp, 17.50);
        assert_eq!(plan.total_plays, 1050);
        assert_eq!(plan.total_gbp, 52.50);
        assert_eq!(plan.warnings.len(), 1);
    }

    #[test]
    fn quoted_total_enforces_the_budget_ceiling() {
        let config = SignageConfig {
            max_budget_gbp: Some(40.0),
            ..test_config()
        };
        let error = build_plan(&config, SignageStrategy::Caasie, None, true).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("exceeds the maximum budget"), "{message}");
        assert!(message.contains("£12.50"), "{message}");
        // A CLI override replaces the configured ceiling.
        let plan = build_plan(&config, SignageStrategy::Caasie, Some(60.0), true).unwrap();
        assert_eq!(plan.total_gbp, 52.50);
        assert_eq!(plan.max_budget_gbp, Some(60.0));
    }

    #[test]
    fn unsupported_markets_are_rejected() {
        let config = SignageConfig {
            market: "US".into(),
            ..test_config()
        };
        let error = build_plan(&config, SignageStrategy::Caasie, None, true).unwrap_err();
        assert!(error.to_string().contains("not supported"), "{error}");
    }

    #[test]
    fn invalid_configurations_are_rejected() {
        let mut config = test_config();
        config.boards[1].board_id = "gb-lon-001".into();
        config.dates.end = "2026-09-01".into();
        let error = build_plan(&config, SignageStrategy::Caasie, None, true).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("duplicate board_id"), "{message}");
        assert!(message.contains("before dates.start"), "{message}");
    }

    #[test]
    fn live_plan_requires_the_credential_environment_variable() {
        std::env::remove_var(DEFAULT_CREDENTIAL_ENV);
        let config = test_config();
        let error = build_plan(&config, SignageStrategy::Caasie, None, false).unwrap_err();
        assert!(
            error.to_string().contains(DEFAULT_CREDENTIAL_ENV),
            "{error}"
        );
    }

    #[test]
    fn quote_responses_are_validated_strictly() {
        let boards = vec!["gb-lon-001".to_string()];
        let valid = serde_json::json!({
            "schema_version": "caasie-quotes-v1",
            "currency": "GBP",
            "quoted_at": "2026-09-16T12:00:00Z",
            "quotes": [{"board_id": "gb-lon-001", "price_per_play": 0.12}],
        });
        let quotes = parse_quotes(valid.to_string().as_bytes(), &boards).unwrap();
        assert_eq!(quotes[0].price_per_play, 0.12);
        assert_eq!(quotes[0].currency, "GBP");

        let wrong_currency = serde_json::json!({
            "schema_version": "caasie-quotes-v1",
            "currency": "AUD",
            "quotes": [{"board_id": "gb-lon-001", "price_per_play": 0.12}],
        });
        assert!(parse_quotes(wrong_currency.to_string().as_bytes(), &boards).is_err());

        let negative = serde_json::json!({
            "schema_version": "caasie-quotes-v1",
            "currency": "GBP",
            "quotes": [{"board_id": "gb-lon-001", "price_per_play": -1.0}],
        });
        assert!(parse_quotes(negative.to_string().as_bytes(), &boards).is_err());

        let missing_board = serde_json::json!({
            "schema_version": "caasie-quotes-v1",
            "currency": "GBP",
            "quotes": [{"board_id": "gb-other-9", "price_per_play": 0.12}],
        });
        assert!(parse_quotes(missing_board.to_string().as_bytes(), &boards).is_err());

        let wrong_schema = serde_json::json!({"schema_version": "other-v9"});
        assert!(parse_quotes(wrong_schema.to_string().as_bytes(), &boards).is_err());
    }
}
