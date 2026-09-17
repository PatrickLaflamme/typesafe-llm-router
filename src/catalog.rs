//! Per-model cost, tier, and cache economics used to enrich routing decisions.
//!
//! **Preferred (Patrick lock 2026-09-17):** build from [`crate::model_source::ModelInfo`]
//! via [`ModelCatalog::from_model_infos`] so Choice prices match the active
//! ModelSource. Optional TOML (`config/models.example.toml`) remains for
//! route-only / paper demos without a ModelSource.
//!
//! Paper policy: `docs/decision-rules.md`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::RouterError;
use crate::model_source::ModelInfo;

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
        match (&self.cost_band_in, &self.cost_band_out) {
            (Some(i), Some(o)) => format!("{i} / 1M in, {o} / 1M out"),
            _ => format!(
                "${:.2}/${:.2} per MTok in/out",
                self.input_usd_per_mtok, self.output_usd_per_mtok
            ),
        }
    }
}

/// Catalog of known models keyed by stable model id (e.g. `composer-2.5`).
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

    /// Build a catalog directly from [`ModelSource::list_models`](crate::ModelSource::list_models).
    ///
    /// This is the v0 source of truth for Choice cost/cache notes when a
    /// ModelSource is selected — no parallel TOML catalog required.
    pub fn from_model_infos(models: &[ModelInfo]) -> Self {
        let mut catalog = Self::new();
        for m in models {
            catalog.insert(m.id.clone(), m.to_cost_profile());
        }
        catalog
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

    /// Built-in illustrative rates for offline route-only demos/tests.
    ///
    /// Prefer [`Self::from_model_infos`] when a ModelSource is selected.
    /// These mirror Cursor source ids so fixtures stay consistent; rates are
    /// the 2026-09-17 Cursor docs snapshot (same as CursorAgentSdkSource).
    pub fn demo() -> Self {
        // Same cards as CursorAgentSdkSource — kept here for route-without-source.
        Self::from_model_infos(&[
            ModelInfo {
                id: "composer-2.5".into(),
                label: Some("Composer 2.5".into()),
                price_input_per_mtok: 0.50,
                price_output_per_mtok: 2.50,
                price_cache_read_per_mtok: Some(0.20),
                price_cache_write_per_mtok: None,
                tier_hint: Some(ModelTier::Small),
            },
            ModelInfo {
                id: "composer-2.5-fast".into(),
                label: Some("Composer 2.5 Fast".into()),
                price_input_per_mtok: 3.00,
                price_output_per_mtok: 15.00,
                price_cache_read_per_mtok: Some(0.50),
                price_cache_write_per_mtok: None,
                tier_hint: Some(ModelTier::Small),
            },
            ModelInfo {
                id: "grok-4.6".into(),
                label: Some("Grok 4.6".into()),
                price_input_per_mtok: 2.00,
                price_output_per_mtok: 6.00,
                price_cache_read_per_mtok: Some(0.50),
                price_cache_write_per_mtok: None,
                tier_hint: Some(ModelTier::Mid),
            },
            ModelInfo {
                id: "grok-4.6-fast".into(),
                label: Some("Grok 4.6 Fast".into()),
                price_input_per_mtok: 4.00,
                price_output_per_mtok: 12.00,
                price_cache_read_per_mtok: Some(1.00),
                price_cache_write_per_mtok: None,
                tier_hint: Some(ModelTier::Mid),
            },
            ModelInfo {
                id: "grok-4.5".into(),
                label: Some("Grok 4.5".into()),
                price_input_per_mtok: 2.00,
                price_output_per_mtok: 6.00,
                price_cache_read_per_mtok: Some(0.50),
                price_cache_write_per_mtok: None,
                tier_hint: Some(ModelTier::Frontier),
            },
            ModelInfo {
                id: "grok-4.5-fast".into(),
                label: Some("Grok 4.5 Fast".into()),
                price_input_per_mtok: 4.00,
                price_output_per_mtok: 18.00,
                price_cache_read_per_mtok: Some(1.00),
                price_cache_write_per_mtok: None,
                tier_hint: Some(ModelTier::Frontier),
            },
        ])
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
                &["composer-2.5".into(), "grok-4.5".into()],
                Some("composer-2.5"),
            )
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].is_current);
        assert_eq!(rows[0].profile.cache_read_usd_per_mtok, Some(0.20));
        assert_eq!(rows[1].profile.tier, Some(ModelTier::Frontier));
    }

    #[test]
    fn from_model_infos_carries_cache_read() {
        let catalog = ModelCatalog::from_model_infos(&[ModelInfo {
            id: "composer-2.5".into(),
            label: None,
            price_input_per_mtok: 0.5,
            price_output_per_mtok: 2.5,
            price_cache_read_per_mtok: Some(0.2),
            price_cache_write_per_mtok: None,
            tier_hint: Some(ModelTier::Small),
        }]);
        let p = catalog.get("composer-2.5").unwrap();
        assert!(p.cache_eligible);
        assert_eq!(p.cache_read_usd_per_mtok, Some(0.2));
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
