//! CLI: route, demo (morning recording), drain-score-queue.
//!
//! When `--model-provider` is set (or implied by `--execute` / `demo`), Choice
//! allowlist + prices come from `ModelProvider.list_models()` — no separate
//! gpt-4o-mini catalog remap.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use typesafe_llm_router::typesafe::{API_KEY_ENV, BASE_URL_ENV};
use typesafe_llm_router::{
    allowlist_from_provider, complete_request_from_session, complete_turn_hot_path,
    complete_turn_with_model_provider, drain_score_queue, score_inline_lab_only, ClientMode,
    CursorAgentSdkSource, DatabricksAiGatewayProvider, FileOutcomeStore, FileScoreQueue,
    HttpTypesafeClient, ModelCatalog, ModelProvider, OutcomeStore, Router, RouterRequest,
    ScoreQueue, ScoresStatus, StubModelProvider, StubTypesafeClient,
};

#[derive(Parser, Debug)]
#[command(
    name = "typesafe-llm-router",
    about = "LLM session router (Choice) + ModelProvider + async Score",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Morning-demo friendly: INPUT → process logs → OUTPUT (model + model_output).
    Demo {
        /// Session JSON (RouterRequest), e.g. examples/a_e/b_short_classify.json
        #[arg(short, long)]
        session: PathBuf,

        /// ModelProvider backend (default: stub). Alias: `--model-source`.
        #[arg(
            long = "model-provider",
            visible_alias = "model-source",
            value_enum,
            default_value_t = ModelProviderKind::Stub
        )]
        model_provider: ModelProviderKind,

        /// Offline Typesafe stub (default for recording).
        #[arg(long, default_value_t = true)]
        stub: bool,

        #[arg(long)]
        catalog: Option<PathBuf>,

        #[arg(long, default_value = "jev-latest")]
        system_one_model: String,

        #[arg(long, default_value = ".router-data/outcomes")]
        outcomes_dir: PathBuf,

        #[arg(long, default_value = ".router-data/score-queue")]
        score_queue_dir: PathBuf,
    },

    /// Route (+ optional execute / model_output / score enqueue).
    Route {
        #[arg(short, long)]
        session: PathBuf,

        #[arg(short, long, value_delimiter = ',')]
        allowlist: Option<Vec<String>>,

        #[arg(short, long)]
        current: Option<String>,

        /// ModelProvider for `--execute` (and to fill allowlist from list_models).
        /// Alias: `--model-source`.
        #[arg(long = "model-provider", visible_alias = "model-source", value_enum)]
        model_provider: Option<ModelProviderKind>,

        #[arg(long)]
        catalog: Option<PathBuf>,

        #[arg(long, default_value = "jev-latest")]
        system_one_model: String,

        #[arg(long)]
        stub: bool,

        #[arg(long)]
        live: bool,

        /// After Choice, run ModelProvider.complete (visible model_output).
        #[arg(long)]
        execute: bool,

        /// Verbose process logs on stderr.
        #[arg(long)]
        verbose: bool,

        /// Inject model_output from file instead of ModelProvider (or `-` for stdin).
        #[arg(long)]
        model_output: Option<PathBuf>,

        #[arg(long)]
        output_tokens_est: Option<u32>,

        #[arg(long, default_value = ".router-data/outcomes")]
        outcomes_dir: PathBuf,

        #[arg(long, default_value = ".router-data/score-queue")]
        score_queue_dir: PathBuf,

        /// LAB ONLY: inline Score (not product hot path).
        #[arg(long)]
        score_inline: bool,

        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
    },

    DrainScoreQueue {
        #[arg(long, default_value = ".router-data/score-queue")]
        score_queue_dir: PathBuf,

        #[arg(long, default_value = ".router-data/outcomes")]
        outcomes_dir: PathBuf,

        #[arg(long, default_value_t = 32)]
        max: usize,

        #[arg(long)]
        stub: bool,

        #[arg(long)]
        live: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ModelProviderKind {
    Stub,
    Cursor,
    /// Databricks Unity AI Gateway (`databricks auth login` → CLI host/token).
    Databricks,
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
        Commands::Demo {
            session,
            model_provider,
            stub: _,
            catalog,
            system_one_model,
            outcomes_dir,
            score_queue_dir,
        } => run_demo(
            session,
            model_provider,
            catalog,
            system_one_model,
            outcomes_dir,
            score_queue_dir,
        ),

        Commands::Route {
            session,
            allowlist,
            current,
            model_provider,
            catalog,
            system_one_model,
            stub,
            live,
            execute,
            verbose,
            model_output,
            output_tokens_est,
            outcomes_dir,
            score_queue_dir,
            score_inline,
            format,
        } => {
            let mut request = load_request(&session, allowlist, current)?;
            let (use_stub, client_mode) = resolve_client_mode(stub, live)?;

            // --execute implies a ModelProvider; default stub when omitted.
            let source_kind = if execute || model_output.is_some() {
                Some(model_provider.unwrap_or(ModelProviderKind::Stub))
            } else {
                model_provider
            };

            let (catalog, source_label) = if let Some(kind) = source_kind {
                let (cat, label) = prepare_request_from_provider(kind, &mut request)?;
                // Optional TOML override only if caller forced --catalog without source prices.
                let cat = if let Some(path) = catalog {
                    eprintln!(
                        "note: --catalog overrides ModelProvider prices; prefer list_models prices"
                    );
                    ModelCatalog::from_toml_file(path)?
                } else {
                    cat
                };
                (cat, Some(label))
            } else {
                let cat = load_catalog(catalog)?;
                if request.allowlist.is_empty() {
                    request.allowlist = cat.models.keys().cloned().collect();
                }
                (cat, None)
            };

            if request.allowlist.is_empty() {
                return Err(
                    "allowlist is empty — pass --allowlist, --model-provider, or use demo catalog"
                        .into(),
                );
            }

            if execute || model_output.is_some() {
                if verbose || execute {
                    print_input_section(&request);
                }
                let queue = FileScoreQueue::open(&score_queue_dir)?;
                let store = FileOutcomeStore::open(&outcomes_dir)?;
                let kind = source_kind.unwrap_or(ModelProviderKind::Stub);

                if use_stub {
                    let client = StubTypesafeClient::new();
                    let router =
                        Router::new(&catalog, &client).with_system_one_model(&system_one_model);
                    let hot = if let Some(path) = model_output {
                        log_step("enrich + Choice route (stub Typesafe)");
                        let output = read_output(&path)?;
                        let decision = router.route(&request)?;
                        log_step(&format!("chosen_model = {}", decision.chosen_model));
                        log_step("ModelProvider skipped (using --model-output file)");
                        complete_turn_hot_path(
                            &router,
                            &request,
                            output,
                            output_tokens_est,
                            client_mode,
                            &queue,
                            &store,
                        )?
                    } else {
                        log_step("ModelProvider.list_models → Choice allowlist + prices");
                        log_step("Choice route via StubTypesafeClient (jev-latest questions)");
                        let hot = run_execute(
                            &router,
                            &request,
                            kind,
                            client_mode,
                            &queue,
                            &store,
                            verbose || execute,
                        )?;
                        log_step(&format!(
                            "chosen_model = {} (= ModelProvider.complete model_id)",
                            hot.decision.chosen_model
                        ));
                        hot
                    };
                    log_step(&format!(
                        "score: pending/enqueued (job {}) — async, not on hot path",
                        hot.score_job_id
                    ));
                    if score_inline {
                        eprintln!("warning: --score-inline is LAB ONLY");
                        let scores = score_inline_lab_only(
                            &client,
                            &request,
                            &hot.decision,
                            &hot.outcome.model_output,
                            &system_one_model,
                            false,
                        )?;
                        let _ = queue.dequeue()?;
                        let mut outcome = hot.outcome;
                        outcome.mark_scored_ok(scores);
                        store.save(&outcome)?;
                        print_json(&outcome, format)?;
                    } else if verbose || execute {
                        print_output_section(
                            &hot.decision.chosen_model,
                            hot.decision.chosen_tier.as_deref(),
                            &hot.outcome.model_output,
                            hot.decision.primary_reason.as_str(),
                        );
                        if format == OutputFormat::Json {
                            println!("\n=== DECISION JSON ===");
                            println!("{}", serde_json::to_string_pretty(&hot.decision)?);
                        }
                    } else {
                        print_json(&hot.outcome, format)?;
                    }
                } else {
                    let http = HttpTypesafeClient::from_env()?;
                    let router =
                        Router::new(&catalog, &http).with_system_one_model(&system_one_model);
                    let hot = run_execute(
                        &router,
                        &request,
                        kind,
                        client_mode,
                        &queue,
                        &store,
                        verbose,
                    )?;
                    print_json(&hot.outcome, format)?;
                }
            } else {
                let _ = source_label;
                let decision = if use_stub {
                    let client = StubTypesafeClient::new();
                    Router::new(&catalog, &client)
                        .with_system_one_model(&system_one_model)
                        .route(&request)?
                } else {
                    let client = HttpTypesafeClient::from_env()?;
                    Router::new(&catalog, &client)
                        .with_system_one_model(&system_one_model)
                        .route(&request)?
                };
                print_json(&decision, format)?;
            }
            Ok(())
        }

        Commands::DrainScoreQueue {
            score_queue_dir,
            outcomes_dir,
            max,
            stub,
            live,
        } => {
            let (use_stub, _) = resolve_client_mode(stub, live)?;
            let queue = FileScoreQueue::open(score_queue_dir)?;
            let store = FileOutcomeStore::open(outcomes_dir)?;
            let outcomes = if use_stub {
                drain_score_queue(&StubTypesafeClient::new(), &queue, &store, max)?
            } else {
                drain_score_queue(&HttpTypesafeClient::from_env()?, &queue, &store, max)?
            };
            println!("{}", serde_json::to_string_pretty(&outcomes)?);
            eprintln!("drained {} score job(s)", outcomes.len());
            Ok(())
        }
    }
}

fn run_demo(
    session: PathBuf,
    model_provider: ModelProviderKind,
    catalog: Option<PathBuf>,
    system_one_model: String,
    outcomes_dir: PathBuf,
    score_queue_dir: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut request = load_request(&session, None, None)?;
    let (catalog_from_src, src_name) = prepare_request_from_provider(model_provider, &mut request)?;
    let catalog = if let Some(path) = catalog {
        eprintln!("note: --catalog overrides ModelProvider prices; prefer list_models prices");
        ModelCatalog::from_toml_file(path)?
    } else {
        catalog_from_src
    };

    println!("========================================");
    println!("  typesafe-llm-router — morning demo");
    println!("========================================\n");

    print_input_section(&request);

    println!("=== PROCESS ===");
    log_step(&format!(
        "ModelProvider.list_models ({src_name}) → Choice allowlist + $/MTok"
    ));
    log_step("System One Choice route (StubTypesafeClient / jev-latest shape)");

    let client = StubTypesafeClient::new();
    let router = Router::new(&catalog, &client).with_system_one_model(&system_one_model);
    let queue = FileScoreQueue::open(&score_queue_dir)?;
    let store = FileOutcomeStore::open(&outcomes_dir)?;

    let _preview = complete_request_from_session("pending", &request.session, None);
    log_step(&format!(
        "ModelProvider = {src_name} (chosen_model id == complete model_id)"
    ));

    let hot = run_execute(
        &router,
        &request,
        model_provider,
        ClientMode::Stub,
        &queue,
        &store,
        true,
    )?;

    log_step(&format!(
        "chosen_model = {}{}",
        hot.decision.chosen_model,
        hot.decision
            .chosen_tier
            .as_ref()
            .map(|t| format!(" ({t})"))
            .unwrap_or_default()
    ));
    log_step("ModelProvider.complete → model_output ready");
    log_step(&format!(
        "score: pending/enqueued (job {}) — async worker later; NOT on hot path",
        hot.score_job_id
    ));
    log_step(&format!(
        "primary_reason = {}",
        hot.decision.primary_reason.as_str()
    ));
    println!();

    print_output_section(
        &hot.decision.chosen_model,
        hot.decision.chosen_tier.as_deref(),
        &hot.outcome.model_output,
        hot.decision.primary_reason.as_str(),
    );

    println!("=== OUTCOME META ===");
    println!("scores_status: {:?}", hot.outcome.scores_status);
    if hot.outcome.scores_status == ScoresStatus::Pending {
        println!("(drain later: cargo run -- drain-score-queue --stub)");
    }
    println!();

    Ok(())
}

fn prepare_request_from_provider(
    kind: ModelProviderKind,
    request: &mut RouterRequest,
) -> Result<(ModelCatalog, &'static str), Box<dyn std::error::Error>> {
    match kind {
        ModelProviderKind::Stub => {
            let src = StubModelProvider::with_task_class(request.task_class);
            let models = src.list_models()?;
            let (allowlist, catalog) = allowlist_from_provider(
                &models,
                &request.allowlist,
                request.current_model.as_deref(),
            )?;
            request.allowlist = allowlist;
            Ok((catalog, src.name()))
        }
        ModelProviderKind::Cursor => {
            let src = CursorAgentSdkSource::from_env();
            let models = src.list_models()?;
            let (allowlist, catalog) = allowlist_from_provider(
                &models,
                &request.allowlist,
                request.current_model.as_deref(),
            )?;
            request.allowlist = allowlist;
            Ok((catalog, src.name()))
        }
        ModelProviderKind::Databricks => {
            let src = DatabricksAiGatewayProvider::from_cli()?;
            let models = src.list_models()?;
            let (allowlist, catalog) = allowlist_from_provider(
                &models,
                &request.allowlist,
                request.current_model.as_deref(),
            )?;
            request.allowlist = allowlist;
            Ok((catalog, src.name()))
        }
    }
}

fn run_execute<C: typesafe_llm_router::TypesafeClient, Q: ScoreQueue, S: OutcomeStore>(
    router: &Router<'_, C>,
    request: &RouterRequest,
    kind: ModelProviderKind,
    client_mode: ClientMode,
    queue: &Q,
    store: &S,
    log: bool,
) -> Result<typesafe_llm_router::HotPathResult, Box<dyn std::error::Error>> {
    match kind {
        ModelProviderKind::Stub => {
            let src = StubModelProvider::with_task_class(request.task_class);
            if log {
                log_step(&format!("ModelProvider.complete ({})", src.name()));
            }
            Ok(complete_turn_with_model_provider(
                router,
                request,
                &src,
                client_mode,
                queue,
                store,
                None,
            )?)
        }
        ModelProviderKind::Cursor => {
            let src = CursorAgentSdkSource::from_env();
            if log {
                log_step(&format!("ModelProvider.complete ({})", src.name()));
            }
            Ok(complete_turn_with_model_provider(
                router,
                request,
                &src,
                client_mode,
                queue,
                store,
                None,
            )?)
        }
        ModelProviderKind::Databricks => {
            let src = DatabricksAiGatewayProvider::from_cli()?;
            if log {
                log_step(&format!("ModelProvider.complete ({})", src.name()));
            }
            Ok(complete_turn_with_model_provider(
                router,
                request,
                &src,
                client_mode,
                queue,
                store,
                None,
            )?)
        }
    }
}

fn print_input_section(request: &RouterRequest) {
    println!("=== INPUT ===");
    if let Some(tc) = &request.task_class {
        println!("task_class: {tc:?}");
    }
    if let Some(cur) = &request.current_model {
        println!("current_model: {cur}");
    }
    println!("allowlist: {:?}", request.allowlist);
    println!("session:");
    for (i, m) in request.session.iter().enumerate() {
        let role = format!("{:?}", m.role).to_lowercase();
        let body = if m.content.len() > 280 {
            format!("{}…", &m.content[..280])
        } else {
            m.content.clone()
        };
        println!("  [{i}] {role}: {body}");
    }
    println!();
}

fn print_output_section(
    chosen_model: &str,
    tier: Option<&str>,
    model_output: &str,
    primary_reason: &str,
) {
    println!("=== OUTPUT ===");
    println!(
        "selected_model: {chosen_model}{}",
        tier.map(|t| format!(" [{t}]")).unwrap_or_default()
    );
    println!("primary_reason: {primary_reason}");
    println!("model_output:");
    println!("{model_output}");
    println!();
}

fn log_step(msg: &str) {
    eprintln!("  → {msg}");
}

fn load_request(
    session: &PathBuf,
    allowlist: Option<Vec<String>>,
    current: Option<String>,
) -> Result<RouterRequest, Box<dyn std::error::Error>> {
    let mut request: RouterRequest = serde_json::from_str(&fs::read_to_string(session)?)?;
    if let Some(list) = allowlist {
        request.allowlist = list;
    }
    if let Some(cur) = current {
        request.current_model = Some(cur);
    }
    Ok(request)
}

fn load_catalog(catalog: Option<PathBuf>) -> Result<ModelCatalog, Box<dyn std::error::Error>> {
    Ok(match catalog {
        Some(path) => ModelCatalog::from_toml_file(path)?,
        None => ModelCatalog::demo(),
    })
}

fn resolve_client_mode(
    stub: bool,
    live: bool,
) -> Result<(bool, ClientMode), Box<dyn std::error::Error>> {
    let use_stub = stub || (!live && std::env::var(API_KEY_ENV).is_err());
    if use_stub && !stub && !live {
        eprintln!(
            "note: {API_KEY_ENV} unset — using offline stub (pass --live with a key, or --stub)"
        );
    }
    if live && std::env::var(API_KEY_ENV).is_err() {
        return Err(format!("--live requires {API_KEY_ENV} (optional {BASE_URL_ENV})").into());
    }
    let mode = if use_stub && !live {
        ClientMode::Stub
    } else {
        ClientMode::Live
    };
    Ok((use_stub && !live, mode))
}

fn read_output(path: &PathBuf) -> Result<String, Box<dyn std::error::Error>> {
    if path.as_os_str() == "-" {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        Ok(buf)
    } else {
        Ok(fs::read_to_string(path)?)
    }
}

fn print_json<T: serde::Serialize>(
    value: &T,
    format: OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(value)?),
    }
    Ok(())
}
