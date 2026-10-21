#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use filmcraft_shorts::{Job, Plan, assemble, execute, prepare, read_json, request_plan};
use serde_json::json;
use std::io::Write;
use std::path::Path;

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--fonts" || a == "--check-font") {
        let report = if args.first().is_some_and(|a| a == "--fonts") {
            filmcraft_shorts::active_fonts::catalog().and_then(|c| serde_json::to_value(c).map_err(|e| e.to_string()))
        } else {
            filmcraft_shorts::active_fonts::check(args.get(1).ok_or("missing font name")?)
        };
        let value = report.unwrap_or_else(|error| json!({"error":error}));
        writeln!(std::io::stdout(), "{value}").map_err(|e| e.to_string())?;
        return Ok(());
    }
    if args.is_empty() || args.first().is_some_and(|a| a == "--help") {
        writeln!(std::io::stdout(), "filmcraft-shorts <job.json> <new-output-directory> [--render]\nfilmcraft-shorts <job.json> <new-output-directory> --plan <plan.json> [--render]\nPaths in job.json are relative to that file. Without --plan, call the configured LLM.\nPlanner must be local (localhost or 127.0.0.1); remote transcript transmission is disabled.\nBuild with --features whisper for automatic Korean transcription.\nOutput directory must not exist; original media are never modified.").map_err(|e| e.to_string())?;
        return Ok(());
    }
    let job_path = Path::new(args.first().ok_or("missing job.json")?);
    let out = Path::new(args.get(1).ok_or("missing output directory")?);
    let mut plan_path = None;
    let mut render = false;
    let mut opts = args.iter().skip(2);
    while let Some(opt) = opts.next() {
        match opt.as_str() {
            "--render" => render = true,
            "--plan" => plan_path = Some(opts.next().ok_or("--plan requires a JSON file")?),
            _ => return Err(format!("unknown option: {opt}")),
        }
    }
    let job: Job = read_json(job_path)?;
    filmcraft_shorts::validate_job(&job)?;
    std::fs::create_dir(out).map_err(|e| format!("create new output directory: {e}"))?;
    // Preserve diagnostics / imported project on later model or render failures.
    let base = job_path.parent().unwrap_or(Path::new("."));
    stage("analysis");
    let mut prepared = prepare(&job, base)?;
    execute(&mut prepared.session, "file.saveAs", json!({"path":out.join("sources.fcproj")}))?;
    std::fs::write(out.join("analysis.json"), serde_json::to_vec_pretty(&prepared.analysis).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    stage("planning");
    let plan: Plan = if let Some(path) = plan_path { read_json(Path::new(path))? } else { request_plan(&job.planner, &prepared.analysis)? };
    stage("assembling");
    assemble(&mut prepared, &job, &plan)?;
    std::fs::write(out.join("plan.json"), serde_json::to_vec_pretty(&plan).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    execute(&mut prepared.session, "file.saveAs", json!({"path":out.join("short.fcproj")}))?;
    execute(&mut prepared.session, "captions.export", json!({"path":out.join("captions.srt"),"track":"C1"}))?;
    if render {
        stage("rendering");
        let result = execute(
            &mut prepared.session,
            "file.exportMedia",
            json!({"path":out.join("short.mp4"),"format":"h264","width":1080,"height":1920,"burnCaptions":true,"audio":true,"wait":true,"hardwareEncoding":"off"}),
        )?;
        if result["offlineMedia"].as_array().is_some_and(|a| !a.is_empty()) {
            return Err("export used offline placeholders; output is not ready".into());
        }
    }
    stage("complete");
    writeln!(std::io::stdout(), "Saved {} (MP4 rendered: {render})", out.display()).map_err(|e| e.to_string())?;
    Ok(())
}
fn stage(name: &str) {
    let _ = writeln!(std::io::stderr(), "[shorts-stage] {name}");
}
fn main() {
    if let Err(e) = run() {
        let _ = writeln!(std::io::stderr(), "filmcraft-shorts: {e}");
        std::process::exit(1);
    }
}
