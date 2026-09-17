//! Per-model cost, tier, and cache economics used to enrich routing decisions.
//!
//! Load from TOML (see `config/models.example.toml`) or build in code.
//! Paper policy lives in `docs/decision-rules.md`; this catalog holds concrete
//! model ids, tiers, and placeholder rates.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::RouterError;

/// Placeholder capability / cost tier from decision-rules §3.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelTier {
    #[serde(rename = "T-small")]
    Small,
    #[serde(rename = "T-mid")]
    Mid,
    #[serde(rename = "T-frontier")]
    Frontier,
}

impl ModelTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Small => "T-small",
            Self::Mid => "T-mid",
            Self::Frontier => "T-frontier",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "small / cheap",
            Self::Mid => "mid",
            Self::Frontier => "frontier",
        }
    }
}

/// Cost / cache / capability profile for one LLM id.
///
/// Units are USD per million tokens (mtok) when numeric rates are set.
/// `cost_band_*` strings match decision-rules placeholder language.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCostProfile {
    /// Explicit id (defaults to the catalog map key when omitted in TOML).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Paper tier: T-small | T-mid | T-frontier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<ModelTier>,
    /// Whether this model can run tool / function-calling loops.
    #[serde(default)]
    pub tool_capable: bool,
    /// Input (prompt) price USD / MTok.
    pub input_usd_per_mtok: f64,
    /// Output (completion) price USD / MTok.
    pub output_usd_per_mtok: f64,
    /// Placeholder input band string (e.g. `$0.05–0.20`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_band_in: Option<String>,
    /// Placeholder output band string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_band_out: Option<String>,
    /// Provider supports prompt caching for this model.
    #[serde(default)]
    pub cache_eligible: bool,
    /// Cached-input read price when the provider supports prompt caching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_usd_per_mtok: Option<f64>,
    /// Cache-write price (first time tokens are written into a prompt cache).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_usd_per_mtok: Option<f64>,
    /// Free-form provider cache / capability notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl ModelCostProfile {
    pub fn tier_label(&self) -> Option<String> {
        self.tier
            .map(|t| format!("{} ({})", t.as_str(), t.label()))
    }

    pub fn band_note(&self) -> String {
        match (&self.cost_band_in, &self.cost_band_out, self.tier) {
            (Some(i), Some(o), _) => format!("{i} / 1M in, {o} / 1M out"),
            (_, _, Some(t)) => format!("{} placeholder band", t.as_str()),
            _ => format!(
                "${:.2}/${:.2} per MTok in/out",
                self.input_usd_per_mtok, self.output_usd_per_mtok
            ),
        }
    }
}

/// Catalog of known models keyed by stable model id (e.g. `gpt-4o-mini`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelCatalog {
    pub models: BTreeMap<String, ModelCostProfile>,
}

#[derive(Debug, Deserialize)]
struct CatalogFile {
    models: BTreeMap<String, ModelCostProfile>,
}

impl ModelCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, id: impl Into<String>, profile: ModelCostProfile) {
        self.models.insert(id.into(), profile);
    }

    pub fn get(&self, id: &str) -> Option<&ModelCostProfile> {
        self.models.get(id)
    }

    /// Load from a TOML file with a top-level `[models.<id>]` table map.
    pub fn from_toml_file(path: impl AsRef<Path>) -> Result<Self, RouterError> {
        let raw = fs::read_to_string(path)?;
        Self::from_toml_str(&raw)
    }

    pub fn from_toml_str(raw: &str) -> Result<Self, RouterError> {
        let file: CatalogFile =
            toml::from_str(raw).map_err(|e| RouterError::Config(e.to_string()))?;
        Ok(Self {
            models: file.models,
        })
    }

    /// Built-in illustrative rates for offline demos/tests.
    ///
    /// These are **not** live provider prices — update before production use.
    pub fn demo() -> Self {
        let mut catalog = Self::new();
        catalog.insert(
            "gpt-4o-mini",
            ModelCostProfile {
                id: Some("gpt-4o-mini".into()),
                tier: Some(ModelTier::Small),
                tool_capable: true,
                input_usd_per_mtok: 0.15,
                output_usd_per_mtok: 0.60,
                cost_band_in: Some("$0.05–0.20".into()),
                cost_band_out: Some("$0.20–0.80".into()),
                cache_eligible: true,
                cache_read_usd_per_mtok: Some(0.075),
                cache_write_usd_per_mtok: Some(0.375),
                notes: Some("T-small; prompt-cache friendly.".into()),
            },
        );
        catalog.insert(
            "claude-haiku-3.5",
            ModelCostProfile {
                id: Some("claude-haiku-3.5".into()),
                tier: Some(ModelTier::Small),
                tool_capable: true,
                input_usd_per_mtok: 0.80,
                output_usd_per_mtok: 4.0,
                cost_band_in: Some("$0.05–0.20".into()),
                cost_band_out: Some("$0.20–0.80".into()),
                cache_eligible: true,
                cache_read_usd_per_mtok: Some(0.08),
                cache_write_usd_per_mtok: Some(1.0),
                notes: Some("T-small; low latency.".into()),
            },
        );
        catalog.insert(
            "gpt-4o",
            ModelCostProfile {
                id: Some("gpt-4o".into()),
                tier: Some(ModelTier::Mid),
                tool_capable: true,
                input_usd_per_mtok: 2.50,
                output_usd_per_mtok: 10.0,
                cost_band_in: Some("$0.50–2.00".into()),
                cost_band_out: Some("$1.50–8.00".into()),
                cache_eligible: true,
                cache_read_usd_per_mtok: Some(1.25),
                cache_write_usd_per_mtok: Some(6.25),
                notes: Some("T-mid; routine code / creative.".into()),
            },
        );
        catalog.insert(
            "claude-sonnet-4",
            ModelCostProfile {
                id: Some("claude-sonnet-4".into()),
                tier: Some(ModelTier::Frontier),
                tool_capable: true,
                input_usd_per_mtok: 3.0,
                output_usd_per_mtok: 15.0,
                cost_band_in: Some("$3.00–15.00".into()),
                cost_band_out: Some("$12.00–60.00".into()),
                cache_eligible: true,
                cache_read_usd_per_mtok: Some(0.30),
                cache_write_usd_per_mtok: Some(3.75),
                notes: Some("T-frontier; long-reason / brittle tool loops.".into()),
            },
        );
        catalog
    }

    /// Resolve allowlisted models into enriched candidate rows.
    pub fn enrich_allowlist(
        &self,
        allowlist: &[String],
        current_model: Option<&str>,
    ) -> Result<Vec<EnrichedCandidate>, RouterError> {
        if allowlist.is_empty() {
            return Err(RouterError::EmptyAllowlist);
        }
        if let Some(cur) = current_model {
            if !allowlist.iter().any(|m| m == cur) {
                return Err(RouterError::CurrentNotAllowlisted(cur.to_string()));
            }
        }

        let mut out = Vec::with_capacity(allowlist.len());
        for id in allowlist {
            let profile = self
                .get(id)
                .ok_or_else(|| RouterError::MissingCatalogEntry(id.clone()))?;
            out.push(EnrichedCandidate {
                model_id: id.clone(),
                is_current: current_model == Some(id.as_str()),
                profile: profile.clone(),
            });
        }
        Ok(out)
    }
}

/// Allowlisted model + cost/cache enrichment ready for the prompt packer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnrichedCandidate {
    pub model_id: String,
    pub is_current: bool,
    pub profile: ModelCostProfile,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_catalog_enriches_allowlist() {
        let catalog = ModelCatalog::demo();
        let rows = catalog
            .enrich_allowlist(
                &["gpt-4o-mini".into(), "claude-sonnet-4".into()],
                Some("gpt-4o-mini"),
            )
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].is_current);
        assert_eq!(rows[1].profile.tier, Some(ModelTier::Frontier));
    }

    #[test]
    fn empty_allowlist_errors() {
        let catalog = ModelCatalog::demo();
        let err = catalog.enrich_allowlist(&[], None).unwrap_err();
        assert!(matches!(err, RouterError::EmptyAllowlist));
    }

    #[test]
    fn toml_loads_tier_and_tool_flags() {
        let raw = r#"
[models.demo-small]
tier = "T-small"
tool_capable = true
cache_eligible = true
input_usd_per_mtok = 0.1
output_usd_per_mtok = 0.4
cost_band_in = "$0.05–0.20"
cost_band_out = "$0.20–0.80"
"#;
        let catalog = ModelCatalog::from_toml_str(raw).unwrap();
        let p = catalog.get("demo-small").unwrap();
        assert_eq!(p.tier, Some(ModelTier::Small));
        assert!(p.tool_capable);
        assert!(p.cache_eligible);
    }
}
