//! End-to-end CLI harness: stub ModelProvider across A–E fixtures.
//!
//! Builds/runs the `typesafe-llm-router` binary. No live API keys.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_typesafe-llm-router"))
}

fn route_execute(session: &str) -> (i32, String, String) {
    let out = Command::new(bin())
        .args([
            "route",
            "--session",
            session,
            "--stub",
            "--execute",
            "--model-provider",
            "stub",
            "--outcomes-dir",
            &format!("target/e2e-outcomes-{}", std::process::id()),
            "--score-queue-dir",
            &format!("target/e2e-queue-{}", std::process::id()),
        ])
        .output()
        .expect("spawn typesafe-llm-router");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn e2e_short_classify_demo_prints_billing() {
    let out = Command::new(bin())
        .args([
            "demo",
            "--session",
            "examples/a_e/b_short_classify.json",
            "--model-provider",
            "stub",
            "--outcomes-dir",
            &format!("target/e2e-demo-outcomes-{}", std::process::id()),
            "--score-queue-dir",
            &format!("target/e2e-demo-queue-{}", std::process::id()),
        ])
        .output()
        .expect("spawn demo");
    assert!(
        out.status.success(),
        "demo failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("=== INPUT ==="), "{stdout}");
    assert!(stdout.contains("=== OUTPUT ==="), "{stdout}");
    assert!(stdout.contains("model_output:"), "{stdout}");
    assert!(stdout.contains("billing"), "{stdout}");
}

#[test]
fn e2e_route_a_through_e_fixtures() {
    let fixtures = [
        "examples/a_e/a_routine_code_fix.json",
        "examples/a_e/b_short_classify.json",
        "examples/a_e/c_long_reasoning.json",
        "examples/a_e/d_creative_brief.json",
        "examples/a_e/e_tool_use_agent.json",
    ];
    for fixture in fixtures {
        let (code, stdout, stderr) = route_execute(fixture);
        assert_eq!(code, 0, "fixture {fixture} failed: {stderr}\n{stdout}");
        assert!(
            stdout.contains("chosen_model") || stdout.contains("model_output"),
            "unexpected stdout for {fixture}: {stdout}"
        );
        // Outcome JSON from --execute without --verbose prints RouteOutcome.
        assert!(
            stdout.contains("scores_status") || stdout.contains("selected_model"),
            "missing scores/output markers for {fixture}: {stdout}"
        );
    }
}

#[test]
fn e2e_model_source_alias_still_works() {
    let out = Command::new(bin())
        .args([
            "route",
            "--session",
            "examples/a_e/b_short_classify.json",
            "--stub",
            "--execute",
            "--model-source",
            "stub",
            "--outcomes-dir",
            &format!("target/e2e-alias-outcomes-{}", std::process::id()),
            "--score-queue-dir",
            &format!("target/e2e-alias-queue-{}", std::process::id()),
        ])
        .output()
        .expect("spawn with alias");
    assert!(
        out.status.success(),
        "alias failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn e2e_help_lists_databricks_provider() {
    let out = Command::new(bin())
        .args(["route", "--help"])
        .output()
        .expect("help");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("model-provider") || stdout.contains("model-source"),
        "{stdout}"
    );
    assert!(stdout.to_lowercase().contains("databricks"), "{stdout}");
}
