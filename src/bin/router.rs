//! Thin CLI around [`typesafe_llm_router::Router`].
//!
//! Prints the full decision-record JSON to stdout (chosen_model,
//! alternatives_considered, cache_hypothesis, rough_cost_note, primary_reason, …).

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use typesafe_llm_router::typesafe::{API_KEY_ENV, BASE_URL_ENV};
use typesafe_llm_router::{
    HttpTypesafeClient, ModelCatalog, Router, RouterRequest, StubTypesafeClient, TypesafeClient,
};

#[derive(Parser, Debug)]
#[command(
    name = "typesafe-llm-router",
    about = "Route an LLM session to the next model using TypeSafe System One",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Decide which allowlisted model should handle the next turn.
    ///
    /// Prints the full decision record JSON to stdout.
    Route {
        /// Path to a JSON file matching [`RouterRequest`].
        #[arg(short, long)]
        session: PathBuf,

        /// Comma-separated allowlist. Overrides `allowlist` in the JSON if set.
        #[arg(short, long, value_delimiter = ',')]
        allowlist: Option<Vec<String>>,

        /// Current model id. Overrides `current_model` in the JSON if set.
        #[arg(short, long)]
        current: Option<String>,

        /// TOML cost/cache catalog. Defaults to the built-in demo catalog.
        #[arg(long)]
        catalog: Option<PathBuf>,

        /// TypeSafe System One model alias (default: jev-latest).
        #[arg(long, default_value = "jev-latest")]
        system_one_model: String,

        /// Use the offline stub (no API key / network). Recommended for A–E smoke.
        #[arg(long)]
        stub: bool,

        /// Force a live HTTP call even if --stub is also passed (errors if no key).
        #[arg(long)]
        live: bool,

        /// Pretty-print JSON decision to stdout (default).
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Json,
}

fn main() -> ExitCode {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Route {
            session,
            allowlist,
            current,
            catalog,
            system_one_model,
            stub,
            live,
            format,
        } => {
            let mut request: RouterRequest = serde_json::from_str(&fs::read_to_string(&session)?)?;
            if let Some(list) = allowlist {
                request.allowlist = list;
            }
            if let Some(cur) = current {
                request.current_model = Some(cur);
            }
            if request.allowlist.is_empty() {
                return Err(
                    "allowlist is empty — pass --allowlist or include it in the JSON".into(),
                );
            }

            let catalog = match catalog {
                Some(path) => ModelCatalog::from_toml_file(path)?,
                None => ModelCatalog::demo(),
            };

            let use_stub = stub || (!live && std::env::var(API_KEY_ENV).is_err());
            if use_stub && !stub && !live {
                eprintln!(
                    "note: {API_KEY_ENV} unset — using offline stub (pass --live with a key, or --stub to silence)"
                );
            }

            let decision = if use_stub && !live {
                let client = StubTypesafeClient::new();
                route_with(&catalog, &client, &request, &system_one_model)?
            } else {
                let client = HttpTypesafeClient::from_env().map_err(|e| {
                    format!(
                        "{e} — set {API_KEY_ENV} (optional {BASE_URL_ENV}), or pass --stub for offline"
                    )
                })?;
                route_with(&catalog, &client, &request, &system_one_model)?
            };

            match format {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&decision)?);
                }
            }
        }
    }
    Ok(())
}

fn route_with<C: TypesafeClient>(
    catalog: &ModelCatalog,
    client: &C,
    request: &RouterRequest,
    system_one_model: &str,
) -> Result<typesafe_llm_router::RouterDecision, typesafe_llm_router::RouterError> {
    Router::new(catalog, client)
        .with_system_one_model(system_one_model)
        .route(request)
}
