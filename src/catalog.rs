//! Per-model cost and cache economics used to enrich routing decisions.
//!
//! Load from TOML (see `config/models.example.toml`) or build in code.
//! The allowlist on [`crate::RouterRequest`] is the caller's constraint;
//! this catalog supplies the enrichment Layer that TypeSafe sees.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::RouterError;

/// Cost / cache profile for one LLM id.
///
/// Units are USD per million tokens (mtok). Fields are illustrative placeholders —
/// replace with your provider's real rates. `None` means "unknown / not applicable".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCostProfile {
    /// Input (prompt) price USD / MTok.
    pub input_usd_per_mtok: f64,
    /// Output (completion) price USD / MTok.
    pub output_usd_per_mtok: f64,
    /// Cached-input read price when the provider supports prompt caching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_usd_per_mtok: Option<f64>,
    /// Cache-write price (first time tokens are written into a prompt cache).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_usd_per_mtok: Option<f64>,
    /// Coarse quality / capability hint for the decision model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_tier: Option<String>,
    /// Free-form notes (context window quirks, tool support, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
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
                input_usd_per_mtok: 0.15,
                output_usd_per_mtok: 0.60,
                cache_read_usd_per_mtok: Some(0.075),
                cache_write_usd_per_mtok: Some(0.375),
                quality_tier: Some("balanced".into()),
                notes: Some("Fast/cheap; prompt-cache friendly for long sessions.".into()),
            },
        );
        catalog.insert(
            "gpt-4o",
            ModelCostProfile {
                input_usd_per_mtok: 2.50,
                output_usd_per_mtok: 10.0,
                cache_read_usd_per_mtok: Some(1.25),
                cache_write_usd_per_mtok: Some(6.25),
                quality_tier: Some("high".into()),
                notes: Some("Higher quality; switching mid-session drops cache locality.".into()),
            },
        );
        catalog.insert(
            "claude-sonnet-4",
            ModelCostProfile {
                input_usd_per_mtok: 3.0,
                output_usd_per_mtok: 15.0,
                cache_read_usd_per_mtok: Some(0.30),
                cache_write_usd_per_mtok: Some(3.75),
                quality_tier: Some("high".into()),
                notes: Some("Strong reasoning; aggressive cache-read discount.".into()),
            },
        );
        catalog.insert(
            "claude-haiku-3.5",
            ModelCostProfile {
                input_usd_per_mtok: 0.80,
                output_usd_per_mtok: 4.0,
                cache_read_usd_per_mtok: Some(0.08),
                cache_write_usd_per_mtok: Some(1.0),
                quality_tier: Some("fast".into()),
                notes: Some("Low latency / cost; good for simple turns.".into()),
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
        assert!(!rows[1].is_current);
    }

    #[test]
    fn empty_allowlist_errors() {
        let catalog = ModelCatalog::demo();
        let err = catalog.enrich_allowlist(&[], None).unwrap_err();
        assert!(matches!(err, RouterError::EmptyAllowlist));
    }
}
