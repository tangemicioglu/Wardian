use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;

use crate::providers::readiness::provider_readiness;
use crate::providers::ProviderFactory;

const CATALOG_CACHE_TTL: Duration = Duration::from_secs(300);
const PROVIDER_COMMAND_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ProviderModelOption {
    pub id: String,
    pub display_name: String,
    pub effort_options: Vec<String>,
    pub default_effort: Option<String>,
    pub is_default: bool,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ProviderModelCatalog {
    pub provider: String,
    pub version: Option<String>,
    pub source: String,
    pub models: Vec<ProviderModelOption>,
    pub refresh_error: Option<String>,
}

impl ProviderModelCatalog {
    fn unavailable(provider: &str, message: impl Into<String>) -> Self {
        Self {
            provider: provider.to_string(),
            version: None,
            source: "unavailable".to_string(),
            models: Vec::new(),
            refresh_error: Some(message.into()),
        }
    }
}

#[derive(Clone)]
struct CachedCatalog {
    catalog: ProviderModelCatalog,
    refreshed_at: Instant,
}

static MODEL_CATALOG_CACHE: Lazy<Mutex<HashMap<String, CachedCatalog>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Lists the models the installed provider currently exposes to this user.
///
/// The result is intentionally provider-owned: Codex and OpenCode return their
/// live catalogues, while Claude and Gemini expose version-compatible aliases
/// that their own CLIs keep current. The short cache prevents a grid of cards
/// from launching duplicate provider discovery processes.
pub async fn model_catalog(provider: &str, force_refresh: bool) -> ProviderModelCatalog {
    let provider = provider.trim().to_ascii_lowercase();
    if !is_user_facing_provider(&provider) {
        return ProviderModelCatalog::unavailable(&provider, "unsupported provider");
    }

    if !force_refresh {
        if let Ok(cache) = MODEL_CATALOG_CACHE.lock() {
            if let Some(cached) = cache.get(&provider) {
                if cached.refreshed_at.elapsed() < CATALOG_CACHE_TTL {
                    return cached.catalog.clone();
                }
            }
        }
    }

    let catalog = discover_model_catalog(&provider).await;
    if let Ok(mut cache) = MODEL_CATALOG_CACHE.lock() {
        if catalog.models.is_empty() {
            if let Some(cached) = cache.get(&provider) {
                let mut retained_catalog = cached.catalog.clone();
                retained_catalog.version = catalog.version.or(retained_catalog.version);
                retained_catalog.refresh_error = catalog.refresh_error;
                return retained_catalog;
            }
        }
        cache.insert(
            provider,
            CachedCatalog {
                catalog: catalog.clone(),
                refreshed_at: Instant::now(),
            },
        );
    }
    catalog
}

async fn discover_model_catalog(provider: &str) -> ProviderModelCatalog {
    let readiness = provider_readiness(provider);
    if !readiness.available {
        return ProviderModelCatalog::unavailable(
            provider,
            readiness
                .reason
                .unwrap_or_else(|| "provider is not installed".to_string()),
        );
    }

    let version = provider_command_output(provider, &["--version"])
        .await
        .ok()
        .and_then(|output| first_nonempty_line(&output));

    match provider {
        "codex" => match provider_command_output(provider, &["debug", "models"]).await {
            Ok(output) => match parse_codex_catalog(&output) {
                Ok(models) if !models.is_empty() => ProviderModelCatalog {
                    provider: provider.to_string(),
                    version,
                    source: "live_catalog".to_string(),
                    models,
                    refresh_error: None,
                },
                Ok(_) => ProviderModelCatalog::unavailable(
                    provider,
                    "Codex returned no selectable models",
                ),
                Err(error) => ProviderModelCatalog {
                    provider: provider.to_string(),
                    version,
                    source: "unavailable".to_string(),
                    models: Vec::new(),
                    refresh_error: Some(error),
                },
            },
            Err(error) => ProviderModelCatalog {
                provider: provider.to_string(),
                version,
                source: "unavailable".to_string(),
                models: Vec::new(),
                refresh_error: Some(error),
            },
        },
        "opencode" => discover_opencode_catalog(provider, version).await,
        "antigravity" => discover_line_catalog(provider, version, &["models"]).await,
        "claude" => {
            discover_alias_catalog(
                provider,
                version,
                &[
                    ("sonnet", "Sonnet"),
                    ("opus", "Opus"),
                    ("haiku", "Haiku"),
                    ("fable", "Fable"),
                ],
                &["low", "medium", "high", "xhigh", "max"],
            )
            .await
        }
        "gemini" => {
            discover_alias_catalog(
                provider,
                version,
                &[
                    ("auto", "Auto"),
                    ("pro", "Pro"),
                    ("flash", "Flash"),
                    ("flash-lite", "Flash Lite"),
                ],
                &[],
            )
            .await
        }
        _ => ProviderModelCatalog::unavailable(provider, "unsupported provider"),
    }
}

async fn discover_opencode_catalog(
    provider: &str,
    version: Option<String>,
) -> ProviderModelCatalog {
    let refreshed =
        discover_line_catalog(provider, version.clone(), &["models", "--refresh"]).await;
    if !refreshed.models.is_empty() {
        return refreshed;
    }

    let mut fallback = discover_line_catalog(provider, version, &["models"]).await;
    if !fallback.models.is_empty() {
        fallback.refresh_error = Some(
            "this OpenCode version does not support model refresh; showing its configured models"
                .to_string(),
        );
    }
    fallback
}

async fn discover_alias_catalog(
    provider: &str,
    version: Option<String>,
    aliases: &[(&str, &str)],
    effort_options: &[&str],
) -> ProviderModelCatalog {
    let help = provider_command_output(provider, &["--help"])
        .await
        .unwrap_or_default();
    if !help.contains("--model") {
        return ProviderModelCatalog {
            provider: provider.to_string(),
            version,
            source: "unavailable".to_string(),
            models: Vec::new(),
            refresh_error: Some(
                "this installed provider version does not expose --model".to_string(),
            ),
        };
    }

    let supported_efforts = if help.contains("--effort") {
        effort_options
    } else {
        &[]
    };
    ProviderModelCatalog {
        provider: provider.to_string(),
        version,
        source: "provider_aliases".to_string(),
        models: aliases
            .iter()
            .map(|(id, display_name)| ProviderModelOption {
                id: (*id).to_string(),
                display_name: (*display_name).to_string(),
                effort_options: supported_efforts
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect(),
                default_effort: None,
                is_default: *id == "auto",
            })
            .collect(),
        refresh_error: None,
    }
}

async fn discover_line_catalog(
    provider: &str,
    version: Option<String>,
    arguments: &[&str],
) -> ProviderModelCatalog {
    match provider_command_output(provider, arguments).await {
        Ok(output) => {
            let models = parse_line_catalog(&output);
            if models.is_empty() {
                ProviderModelCatalog {
                    provider: provider.to_string(),
                    version,
                    source: "unavailable".to_string(),
                    models,
                    refresh_error: Some("provider returned no selectable models".to_string()),
                }
            } else {
                ProviderModelCatalog {
                    provider: provider.to_string(),
                    version,
                    source: "live_catalog".to_string(),
                    models,
                    refresh_error: None,
                }
            }
        }
        Err(error) => ProviderModelCatalog {
            provider: provider.to_string(),
            version,
            source: "unavailable".to_string(),
            models: Vec::new(),
            refresh_error: Some(error),
        },
    }
}

async fn provider_command_output(provider: &str, arguments: &[&str]) -> Result<String, String> {
    let provider = ProviderFactory::resolve(provider)?;
    let (program, base_arguments) = provider.get_executable();
    let mut command = crate::utils::process::new_silent_command(&program);
    command
        .args(base_arguments)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let child = command
        .spawn()
        .map_err(|error| format!("failed to start provider discovery: {error}"))?;
    let output = tokio::time::timeout(PROVIDER_COMMAND_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| "provider discovery timed out".to_string())?
        .map_err(|error| format!("provider discovery failed: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = first_nonempty_line(stderr.as_ref())
            .unwrap_or_else(|| "unknown provider error".to_string());
        return Err(format!("provider discovery failed: {detail}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn parse_codex_catalog(output: &str) -> Result<Vec<ProviderModelOption>, String> {
    let parsed: serde_json::Value = serde_json::from_str(output)
        .map_err(|error| format!("invalid Codex model catalogue: {error}"))?;
    let models = parsed
        .get("models")
        .or_else(|| parsed.get("data"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "Codex model catalogue did not contain models".to_string())?;

    Ok(models
        .iter()
        .filter_map(|model| {
            let id = model
                .get("slug")
                .or_else(|| model.get("id"))
                .or_else(|| model.get("model"))
                .and_then(serde_json::Value::as_str)?
                .trim();
            if id.is_empty() {
                return None;
            }
            if matches!(id, "codex-auto-review" | "gpt-5.6-sol-wm") {
                return None;
            }
            let display_name = model
                .get("display_name")
                .or_else(|| model.get("displayName"))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(id)
                .to_string();
            let effort_options = model
                .get("supported_reasoning_levels")
                .or_else(|| model.get("supportedReasoningEfforts"))
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|effort| {
                    effort
                        .get("effort")
                        .or_else(|| effort.get("reasoningEffort"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                })
                .collect();
            let default_effort = model
                .get("default_reasoning_level")
                .or_else(|| model.get("defaultReasoningEffort"))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            Some(ProviderModelOption {
                id: id.to_string(),
                display_name,
                effort_options,
                default_effort,
                is_default: model
                    .get("isDefault")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or_else(|| {
                        model.get("priority").and_then(serde_json::Value::as_u64) == Some(1)
                    }),
            })
        })
        .collect())
}

fn parse_line_catalog(output: &str) -> Vec<ProviderModelOption> {
    let mut seen = std::collections::HashSet::new();
    output
        .lines()
        .map(strip_ansi)
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty() && !line.contains(char::is_whitespace))
        .filter(|line| seen.insert(line.clone()))
        .map(|id| ProviderModelOption {
            display_name: id.clone(),
            id,
            effort_options: Vec::new(),
            default_effort: None,
            is_default: false,
        })
        .collect()
}

fn strip_ansi(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            output.push(ch);
            continue;
        }
        if chars.next() != Some('[') {
            continue;
        }
        for code in chars.by_ref() {
            if code.is_ascii_alphabetic() {
                break;
            }
        }
    }
    output
}

fn first_nonempty_line(value: &str) -> Option<String> {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn is_user_facing_provider(provider: &str) -> bool {
    matches!(
        provider,
        "claude" | "codex" | "gemini" | "antigravity" | "opencode"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_codex_catalogue_shape() {
        let output = r#"{
          "models": [
            {
              "slug": "gpt-5.6-sol",
              "display_name": "GPT-5.6-Sol",
              "default_reasoning_level": "medium",
              "supported_reasoning_levels": [{"effort": "low"}, {"effort": "high"}],
              "priority": 1
            },
            {
              "slug": "codex-auto-review",
              "display_name": "Codex Auto Review",
              "default_reasoning_level": "medium",
              "supported_reasoning_levels": [{"effort": "low"}, {"effort": "medium"}]
            },
            {
              "slug": "gpt-5.6-sol-wm",
              "display_name": "GPT-5.6-Sol-WM",
              "default_reasoning_level": "low",
              "supported_reasoning_levels": [{"effort": "low"}]
            }
          ]
        }"#;

        assert_eq!(
            parse_codex_catalog(output).unwrap(),
            vec![ProviderModelOption {
                id: "gpt-5.6-sol".to_string(),
                display_name: "GPT-5.6-Sol".to_string(),
                effort_options: vec!["low".to_string(), "high".to_string()],
                default_effort: Some("medium".to_string()),
                is_default: true,
            }],
        );
    }

    #[test]
    fn line_catalogue_strips_ansi_and_deduplicates_models() {
        let models = parse_line_catalog(
            "\u{1b}[31mopenai/gpt-5.6\u{1b}[0m\nopenai/gpt-5.6\nopencode/free\n",
        );

        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["openai/gpt-5.6", "opencode/free"],
        );
    }
}
