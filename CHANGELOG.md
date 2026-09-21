# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Crate version lives in `Cargo.toml` (`[package].version`). Sidecar Node deps are
pinned in root `package.json` (not a published npm package).

## [Unreleased]

### Added

- Project Cursor skill at `.cursor/skills/typesafe-ai` (upstream TypeSafe skill)
  so Cloud Agents load live docs guidance for decisions/builds.
- M4 live Databricks smoke docs + CLI checklist (`docs/databricks-live.md`,
  expanded `docs/demos.md` §3) and `scripts/smoke_databricks.sh` (refuses missing
  auth; never prints access tokens). Lab login uses placeholder host only —
  no real workspace URL in repo.
- `examples/a_e/b_short_classify_databricks.json` — short-classify fixture with a
  Databricks catalog `current_model` for the live smoke path.

### Changed

- Renamed `CursorAgentSdkSource` → `CursorAgentSdkProvider` (ModelProvider-aligned).
  Deprecated type aliases (`CursorAgentSdkSource`, `CursorAgentModelSource`,
  `ModelSourceError`, …) remain for older call sites.
- Documented stub / Cursor / Databricks-offline demos in `docs/demos.md`.
- Added root `package.json` pinning `@cursor/sdk` for reproducible sidecar install.

## [0.1.0] — 2026-09-18

Initial lab release (M1 trunk via PR #6 and prior design locks).

### Added

- **ModelProvider** trait (`list_models` + `complete`) with Stub, Cursor Agent SDK
  (Node ESM sidecar), and Databricks Unity AI Gateway providers.
- Async **Score** queue (`score_queue`) — hot path never awaits Score.
- Choice allowlist lock: allowlist + token prices from `ModelProvider.list_models()`;
  `RouterDecision.chosen_model` is the execute id (no catalog remap).
- GitHub Actions CI: `fmt`, `clippy -D warnings`, unit, integration, e2e (stub).
- `.env.example` for Typesafe / Cursor / Databricks profile vars (no secrets).
- Morning `demo` CLI and A–E session fixtures.

### Changed

- Public naming: **ModelProvider** preferred over historical **ModelSource**
  (`--model-source` remains a CLI alias of `--model-provider` only).

[Unreleased]: https://github.com/PatrickLaflamme/typesafe-llm-router/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/PatrickLaflamme/typesafe-llm-router/releases/tag/v0.1.0
