//! Local browser client for the existing Shorts worker. Uploads stream directly to disk.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tiny_http::{Header, Request, Response, Server, StatusCode};

type Reply = Response<Box<dyn Read + Send>>;
type ApiResult = Result<Reply, (u16, String)>;
const MAX_UPLOAD: u64 = 8 * 1024 * 1024 * 1024;
const MAX_STORAGE: u64 = 20 * 1024 * 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Serialize, Deserialize)]
struct Upload {
    id: String,
    name: String,
    size: u64,
    path: PathBuf,
}
#[derive(Clone, Serialize, Deserialize)]
struct Record {
    id: String,
    brief: String,
    created: u128,
    status: String,
    error: Option<String>,
}
struct App {
    root: PathBuf,
    worker: PathBuf,
    port: u16,
    records: Mutex<BTreeMap<String, Record>>,
    active: Mutex<Option<(String, Child)>>,
    upload: Mutex<()>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    caption_template: Option<filmcraft_shorts::psd_style::Template>,
    #[serde(default)]
    caption_design: filmcraft_shorts::CaptionDesign,
    title_font: Option<String>,
    dialogue_font: Option<String>,
    crew_font: Option<String>,
    brief: String,
    max_seconds: u32,
    speech_model: String,
    groups: Vec<InputGroup>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputGroup {
    name: String,
    sync: String,
    cameras: Vec<InputSource>,
    recordings: Vec<InputSource>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputSource {
    #[serde(default)]
    caption_role: filmcraft_shorts::CaptionRole,
    file_id: String,
    label: String,
}
fn now() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}
fn new_id() -> String {
    format!("{:x}-{:x}", now(), NEXT.fetch_add(1, Ordering::Relaxed))
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}
fn err(e: impl std::fmt::Display) -> (u16, String) {
    (500, e.to_string())
}
fn header(name: &str, value: &str) -> Option<Header> {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).ok()
}
fn reply(code: u16, content: impl Into<String>, mime: &str) -> Reply {
    let mut r = Response::from_string(content.into()).with_status_code(StatusCode(code)).boxed();
    for (k, v) in [
        ("Content-Type", mime),
        ("Cache-Control", "no-store"),
        ("X-Content-Type-Options", "nosniff"),
        (
            "Content-Security-Policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' blob:; media-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'",
        ),
    ] {
        if let Some(h) = header(k, v) {
            r.add_header(h);
        }
    }
    r
}
fn json_reply(code: u16, v: Value) -> Reply {
    reply(code, v.to_string(), "application/json; charset=utf-8")
}
fn json_body<T: serde::de::DeserializeOwned>(r: &mut Request) -> Result<T, (u16, String)> {
    if !r.headers().iter().any(|h| h.field.equiv("Content-Type") && h.value.as_str().starts_with("application/json")) {
        return Err((415, "JSON 요청이 필요합니다.".into()));
    }
    let mut data = Vec::new();
    r.as_reader().take(1024 * 1024 + 1).read_to_end(&mut data).map_err(err)?;
    if data.len() > 1024 * 1024 {
        return Err((413, "요청이 너무 큽니다.".into()));
    }
    serde_json::from_slice(&data).map_err(|_| (400, "입력 형식을 확인해 주세요.".into()))
}
fn save_record(app: &App, record: &Record) -> Result<(), (u16, String)> {
    let path = app.root.join("jobs").join(&record.id).join("record.json");
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, serde_json::to_vec(record).map_err(err)?).map_err(err)?;
    std::fs::rename(temp, path).map_err(err)
}
fn tail(path: &Path) -> String {
    let Ok(mut f) = File::open(path) else { return String::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(len.saturating_sub(32 * 1024)));
    let mut bytes = Vec::new();
    let _ = f.take(32 * 1024).read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}
fn refresh(app: &App) -> Result<(), (u16, String)> {
    let mut active = app.active.lock().map_err(err)?;
    if let Some((id, child)) = active.as_mut() {
        let status = child.try_wait().map_err(err)?;
        if let Some(status) = status {
            let mut records = app.records.lock().map_err(err)?;
            if let Some(record) = records.get_mut(id) {
                let root = app.root.join("jobs").join(id.as_str());
                let valid = ["short.mp4", "captions.srt", "short.fcproj", "plan.json"]
                    .iter()
                    .all(|n| std::fs::metadata(root.join("output").join(n)).is_ok_and(|m| m.len() > 0));
                record.status = if status.success() && valid { "done" } else { "failed" }.into();
                if record.status == "failed" {
                    record.error =
                        Some(tail(&root.join("worker.log")).lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"));
                }
                save_record(app, record)?;
            }
            *active = None;
        }
    }
    Ok(())
}
fn record_json(app: &App, r: &Record) -> Value {
    let root = app.root.join("jobs").join(&r.id);
    let log = tail(&root.join("worker.log"));
    let stage = log.lines().filter_map(|l| l.strip_prefix("[shorts-stage] ")).next_back().unwrap_or("analysis");
    let plan: Value = std::fs::read(root.join("output/plan.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    let can_retry = ["failed", "cancelled", "interrupted"].contains(&r.status.as_str())
        && ["sources.fcproj", "analysis.json"].iter().all(|n| root.join("output").join(n).is_file());
    let planning_status = log.lines().filter_map(|l| l.strip_prefix("[shorts-planner] ")).next_back();
    json!({"can_retry":can_retry,"planning_status":planning_status,"id":r.id,"brief":r.brief,"created":r.created,"status":r.status,"stage":stage,"error":r.error,"title":plan["title"],"segments":plan["segments"]})
}
fn health() -> Value {
    let agent: ureq::Agent = ureq::Agent::config_builder().proxy(None).timeout_global(Some(Duration::from_secs(2))).build().into();
    let server = agent.get("http://127.0.0.1:8000/health").call().is_ok();
    let models: Value = agent
        .get("http://127.0.0.1:8000/v1/models")
        .call()
        .ok()
        .and_then(|mut r| r.body_mut().with_config().limit(65536).read_json().ok())
        .unwrap_or(Value::Null);
    let model_ready = server && models["data"].as_array().is_some_and(|a| a.iter().any(|m| m["id"] == "interview-local"));
    let status = if model_ready {
        "ready"
    } else if server {
        "model_mismatch"
    } else {
        "not_ready"
    };
    let mut s = filmcraft_engine::Session::default();
    let speech = s.execute("transcript.models", json!({})).unwrap_or(Value::Null);
    let fonts = s.execute("fonts.list", json!({})).unwrap_or(Value::Null);
    json!({"platform":std::env::consts::OS,"model_status":status,"fonts":fonts,"model_ready":model_ready,"speech_available":cfg!(feature="whisper"),"speech_models":speech["models"],"local_only":true})
}
fn upload(r: &mut Request, app: &App) -> ApiResult {
    let _guard = app.upload.try_lock().map_err(|_| (409, "다른 파일을 업로드 중입니다.".into()))?;
    let encoded = r.headers().iter().find(|h| h.field.equiv("X-File-Name")).map(|h| h.value.as_str()).ok_or((400, "파일 이름이 없습니다.".into()))?;
    let name = percent_encoding::percent_decode_str(encoded).decode_utf8().map_err(|_| (400, "파일 이름을 확인해 주세요.".into()))?.to_string();
    if name.len() > 512 || name.contains(['/', '\\', '\0']) {
        return Err((400, "파일 이름을 확인해 주세요.".into()));
    }
    let ext = Path::new(&name).extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if !["mp4", "mov", "mkv", "webm", "avi", "wav", "flac", "mp3", "m4a", "aac", "ogg", "opus", "aiff"].contains(&ext.as_str()) {
        return Err((415, "영상 또는 오디오 파일을 선택해 주세요.".into()));
    }
    let length = r.body_length().ok_or((411, "파일 크기 정보가 필요합니다.".into()))? as u64;
    if length == 0 || length > MAX_UPLOAD {
        return Err((413, "파일당 최대 8GB까지 업로드할 수 있습니다.".into()));
    }
    let used = std::fs::read_dir(app.root.join("uploads")).map_err(err)?.filter_map(Result::ok).filter_map(|f| f.metadata().ok()).map(|m| m.len()).sum::<u64>();
    if used.saturating_add(length) > MAX_STORAGE {
        return Err((413, "업로드 저장 공간(20GB)이 가득 찼습니다.".into()));
    }
    let id = new_id();
    let path = app.root.join("uploads").join(format!("{id}.{ext}"));
    let mut file = OpenOptions::new().write(true).create_new(true).open(&path).map_err(err)?;
    let copied = std::io::copy(&mut r.as_reader().take(MAX_UPLOAD + 1), &mut file);
    if !matches!(copied,Ok(n) if n==length) {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err((400, "업로드가 중단되었습니다. 다시 시도해 주세요.".into()));
    }
    file.sync_all().map_err(err)?;
    let record = Upload { id: id.clone(), name: name.clone(), size: length, path };
    std::fs::write(app.root.join("uploads").join(format!("{id}.json")), serde_json::to_vec(&record).map_err(err)?).map_err(err)?;
    Ok(json_reply(201, json!({"id":id,"name":name,"size":length})))
}
fn remove_upload(app: &App, id: &str) -> ApiResult {
    if !valid_id(id) {
        return Err((400, "잘못된 파일 정보입니다.".into()));
    }
    let _active = app.active.lock().map_err(err)?;
    let _upload = app.upload.lock().map_err(err)?;
    let metadata = app.root.join("uploads").join(format!("{id}.json"));
    let upload: Upload = filmcraft_shorts::read_json(&metadata).map_err(|_| (404, "업로드 파일을 찾을 수 없습니다.".into()))?;
    for record in app.records.lock().map_err(err)?.values() {
        let path = app.root.join("jobs").join(&record.id).join("job.json");
        let Ok(job) = filmcraft_shorts::read_json::<filmcraft_shorts::Job>(&path) else {
            return Ok(json_reply(200, json!({"retained":true})));
        };
        if job.groups.iter().any(|g| g.cameras.iter().chain(&g.recordings).any(|source| source.path == upload.path)) {
            return Ok(json_reply(200, json!({"retained":true})));
        }
    }
    let path = upload.path.canonicalize().map_err(err)?;
    if !path.starts_with(app.root.join("uploads")) {
        return Err((400, "파일 경로가 올바르지 않습니다.".into()));
    }
    std::fs::remove_file(path).map_err(err)?;
    std::fs::remove_file(metadata).map_err(err)?;
    Ok(json_reply(200, json!({"retained":false})))
}

fn start_job(r: &mut Request, app: &App) -> ApiResult {
    let input: Input = json_body(r)?;
    let mut active = app.active.lock().map_err(err)?;
    if active.is_some() {
        return Err((409, "진행 중인 편집이 끝난 후 새 작업을 시작해 주세요.".into()));
    }
    if input.groups.is_empty() || input.groups.len() > 32 {
        return Err((400, "촬영 그룹은 1~32개로 구성해 주세요.".into()));
    }
    let mut groups = Vec::new();
    for group in &input.groups {
        if group.name.len() > 200 {
            return Err((400, "그룹 이름이 너무 깁니다.".into()));
        }
        let mut seen = BTreeSet::new();
        let mut resolve = |sources: &[InputSource]| -> Result<Vec<Value>, (u16, String)> {
            if sources.len() > 32 {
                return Err((400, "그룹당 파일 수가 너무 많습니다.".into()));
            }
            sources
                .iter()
                .map(|source| {
                    if !valid_id(&source.file_id) || !seen.insert(source.file_id.clone()) || source.label.len() > 500 {
                        return Err((400, "중복 파일 또는 잘못된 파일 정보입니다.".into()));
                    }
                    let path = app.root.join("uploads").join(format!("{}.json", source.file_id));
                    let upload: Upload = filmcraft_shorts::read_json(&path).map_err(|_| (400, "업로드된 파일을 찾을 수 없습니다.".into()))?;
                    let path = upload.path.canonicalize().map_err(|_| (400, "업로드 파일을 찾을 수 없습니다.".into()))?;
                    if !path.starts_with(app.root.join("uploads")) || !path.is_file() {
                        return Err((400, "잘못된 업로드 파일 경로입니다.".into()));
                    }
                    Ok(json!({"path":path,"label":source.label,"caption_role":source.caption_role}))
                })
                .collect()
        };
        let cameras = resolve(&group.cameras)?;
        let recordings = resolve(&group.recordings)?;
        groups.push(json!({"name":group.name,"sync":group.sync,"cameras":cameras,"recordings":recordings}));
    }
    let mut manifest = json!({"caption_design":input.caption_design,"brief":input.brief,"max_seconds":input.max_seconds,"speech_model":input.speech_model,"groups":groups,"planner":{"url":"http://127.0.0.1:8000/v1/chat/completions","model":"interview-local"}});
    if let Some(font) = input.title_font {
        manifest["title_font"] = json!(font);
    }
    if let Some(font) = input.dialogue_font {
        manifest["dialogue_font"] = json!(font);
    }
    if let Some(font) = input.crew_font {
        manifest["crew_font"] = json!(font);
    }
    if let Some(template) = input.caption_template {
        manifest["caption_template"] = serde_json::to_value(template).map_err(err)?;
    }
    let typed: filmcraft_shorts::Job = serde_json::from_value(manifest.clone()).map_err(|e| (400, e.to_string()))?;
    filmcraft_shorts::validate_job(&typed).map_err(|e| (400, e))?;
    if !["whisper-tiny", "whisper-base", "whisper-small"].contains(&typed.speech_model.as_str()) {
        return Err((400, "지원하는 음성 모델을 선택해 주세요.".into()));
    }
    let id = new_id();
    let root = app.root.join("jobs").join(&id);
    std::fs::create_dir(&root).map_err(err)?;
    std::fs::write(root.join("job.json"), serde_json::to_vec_pretty(&manifest).map_err(err)?).map_err(err)?;
    let log = File::create(root.join("worker.log")).map_err(err)?;
    let child = Command::new(&app.worker)
        .arg(root.join("job.json"))
        .arg(root.join("output"))
        .arg("--render")
        .stdout(Stdio::from(log.try_clone().map_err(err)?))
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(err)?;
    let record = Record { id: id.clone(), brief: input.brief, created: now(), status: "running".into(), error: None };
    app.records.lock().map_err(err)?.insert(id.clone(), record.clone());
    *active = Some((id.clone(), child));
    save_record(app, &record)?;
    Ok(json_reply(202, record_json(app, &record)))
}
fn retry_job(app: &App, id: &str) -> ApiResult {
    let mut active = app.active.lock().map_err(err)?;
    if active.is_some() {
        return Err((409, "진행 중인 편집이 끝난 뒤 다시 시도하세요.".into()));
    }
    let previous = app.records.lock().map_err(err)?.get(id).cloned().ok_or((404, "작업을 찾을 수 없습니다.".into()))?;
    if !["failed", "cancelled", "interrupted"].contains(&previous.status.as_str()) {
        return Err((409, "중단된 작업만 다시 시도할 수 있습니다.".into()));
    }
    let previous_root = app.root.join("jobs").join(id);
    let cache = previous_root.join("output");
    if !["sources.fcproj", "analysis.json"].iter().all(|n| cache.join(n).is_file()) {
        return Err((409, "완료된 분석이 없습니다. 새 편집을 시작하세요.".into()));
    }
    let manifest: Value = filmcraft_shorts::read_json(&previous_root.join("job.json")).map_err(err)?;
    let typed: filmcraft_shorts::Job = serde_json::from_value(manifest.clone()).map_err(err)?;
    filmcraft_shorts::validate_job(&typed).map_err(err)?;
    let next = new_id();
    let root = app.root.join("jobs").join(&next);
    std::fs::create_dir(&root).map_err(err)?;
    std::fs::write(root.join("job.json"), serde_json::to_vec(&manifest).map_err(err)?).map_err(err)?;
    let log = File::create(root.join("worker.log")).map_err(err)?;
    let child = Command::new(&app.worker)
        .arg(root.join("job.json"))
        .arg(root.join("output"))
        .arg("--render")
        .arg("--resume")
        .arg(cache)
        .stdout(Stdio::from(log.try_clone().map_err(err)?))
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(err)?;
    let record = Record { id: next.clone(), brief: previous.brief, created: now(), status: "running".into(), error: None };
    app.records.lock().map_err(err)?.insert(next.clone(), record.clone());
    *active = Some((next, child));
    save_record(app, &record)?;
    Ok(json_reply(202, record_json(app, &record)))
}
fn artifact(r: &Request, app: &App, id: &str, name: &str) -> ApiResult {
    if !valid_id(id) || !["short.mp4", "captions.srt", "short.fcproj", "plan.json"].contains(&name) {
        return Err((404, "파일을 찾을 수 없습니다.".into()));
    }
    if app.records.lock().map_err(err)?.get(id).is_none_or(|r| r.status != "done") {
        return Err((409, "편집이 완료된 후 결과를 받을 수 있습니다.".into()));
    }
    let mut file = File::open(app.root.join("jobs").join(id).join("output").join(name)).map_err(|_| (404, "파일을 찾을 수 없습니다.".into()))?;
    let len = file.metadata().map_err(err)?.len();
    let mut code = 200;
    let (mut start, mut end) = (0, len.saturating_sub(1));
    if let Some(range) = r.headers().iter().find(|h| h.field.equiv("Range")) {
        let value = range.value.as_str().strip_prefix("bytes=").ok_or((416, "잘못된 영상 범위입니다.".into()))?;
        let (a, b) = value.split_once('-').ok_or((416, "잘못된 영상 범위입니다.".into()))?;
        start = a.parse().map_err(|_| (416, "잘못된 영상 범위입니다.".into()))?;
        if !b.is_empty() {
            end = b.parse().map_err(|_| (416, "잘못된 영상 범위입니다.".into()))?;
        }
        if start > end || end >= len {
            return Err((416, "잘못된 영상 범위입니다.".into()));
        }
        code = 206;
    }
    file.seek(SeekFrom::Start(start)).map_err(err)?;
    let mime = if name.ends_with("mp4") {
        "video/mp4"
    } else if name.ends_with("json") {
        "application/json"
    } else {
        "application/octet-stream"
    };
    let size = end.saturating_sub(start).saturating_add(1);
    let mut response = Response::new(StatusCode(code), Vec::new(), Box::new(file.take(size)) as Box<dyn Read + Send>, usize::try_from(size).ok(), None);
    for (k, v) in [
        ("Content-Type", mime.to_string()),
        ("Accept-Ranges", "bytes".into()),
        ("X-Content-Type-Options", "nosniff".into()),
        ("Cache-Control", "no-store".into()),
    ] {
        if let Some(h) = header(k, &v) {
            response.add_header(h);
        }
    }
    if code == 206
        && let Some(h) = header("Content-Range", &format!("bytes {start}-{end}/{len}"))
    {
        response.add_header(h);
    }
    Ok(response)
}
fn route(r: &mut Request, app: &App) -> ApiResult {
    let host = r.headers().iter().find(|h| h.field.equiv("Host")).map(|h| h.value.as_str()).unwrap_or("");
    if host != format!("127.0.0.1:{}", app.port) && host != format!("localhost:{}", app.port) {
        return Err((403, "로컬 주소로 접속해 주세요.".into()));
    }
    if let Some(origin) = r.headers().iter().find(|h| h.field.equiv("Origin"))
        && origin.value.as_str() != format!("http://{host}")
    {
        return Err((403, "다른 사이트의 요청은 허용하지 않습니다.".into()));
    }
    refresh(app)?;
    let path = r.url().split('?').next().unwrap_or("").to_string();
    match (r.method().as_str(), path.as_str()) {
        ("GET", "/") => Ok(reply(200, include_str!("../../web/index.html"), "text/html; charset=utf-8")),
        ("GET", "/app.css") => Ok(reply(200, include_str!("../../web/app.css"), "text/css; charset=utf-8")),
        ("GET", "/app.js") => Ok(reply(200, include_str!("../../web/app.js"), "text/javascript; charset=utf-8")),
        ("POST", "/api/caption-template") => {
            let length = r.body_length().filter(|n| *n > 0 && *n <= 128 * 1024 * 1024).ok_or((413, "PSD는 128MB 이하로 올려주세요.".into()))?;
            let mut bytes = Vec::new();
            r.as_reader().take(128 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(err)?;
            if bytes.len() != length {
                return Err((400, "PSD 업로드가 중단됐습니다.".into()));
            }
            let template = filmcraft_shorts::psd_style::parse(&bytes).map_err(|e| (422, e))?;
            Ok(json_reply(
                200,
                json!({"template":template,"notice":"텍스트 레이어의 글꼴·색상·크기·세로 위치·단색 테두리를 적용합니다. 글꼴은 Mac에서 활성화해야 합니다. 자간·복합 효과·픽셀 단위 일치는 지원하지 않습니다."}),
            ))
        }
        ("GET", "/api/fonts") => {
            let value = filmcraft_shorts::active_fonts::probe(&app.worker, &["--fonts"]).map_err(|e| (503, e))?;
            Ok(json_reply(200, value))
        }
        ("POST", "/api/fonts/check") => {
            let input: Value = json_body(r)?;
            let name = input["font"].as_str().filter(|s| !s.trim().is_empty() && s.len() <= 256).ok_or((400, "글꼴 이름을 확인해 주세요.".into()))?;
            let value = filmcraft_shorts::active_fonts::probe(&app.worker, &["--check-font", name]).map_err(|e| (503, e))?;
            Ok(json_reply(200, value))
        }
        ("GET", "/api/health") => Ok(json_reply(200, health())),
        ("POST", "/api/files") => upload(r, app),
        ("POST", "/api/jobs") => start_job(r, app),
        ("GET", "/api/jobs") => {
            let records = app.records.lock().map_err(err)?;
            Ok(json_reply(200, json!({"jobs":records.values().rev().take(100).map(|r|record_json(app,r)).collect::<Vec<_>>()})))
        }
        _ => {
            let parts: Vec<_> = path.trim_matches('/').split('/').collect();
            match (r.method().as_str(), parts.as_slice()) {
                ("DELETE", ["api", "files", id]) => remove_upload(app, id),
                ("GET", ["api", "jobs", id]) if valid_id(id) => {
                    let records = app.records.lock().map_err(err)?;
                    let record = records.get(*id).ok_or((404, "작업을 찾을 수 없습니다.".into()))?;
                    Ok(json_reply(200, record_json(app, record)))
                }
                ("POST", ["api", "jobs", id, "retry"]) if valid_id(id) => retry_job(app, id),
                ("POST", ["api", "jobs", id, "cancel"]) if valid_id(id) => {
                    let mut active = app.active.lock().map_err(err)?;
                    if let Some((running, child)) = active.as_mut()
                        && running == id
                    {
                        child.kill().map_err(err)?;
                        let _ = child.wait();
                        *active = None;
                        let mut records = app.records.lock().map_err(err)?;
                        if let Some(record) = records.get_mut(*id) {
                            record.status = "cancelled".into();
                            save_record(app, record)?;
                        }
                        return Ok(json_reply(200, json!({"status":"cancelled"})));
                    }
                    Err((409, "취소할 진행 작업이 없습니다.".into()))
                }
                ("GET", ["api", "jobs", id, "files", name]) => artifact(r, app, id, name),
                _ => Err((404, "요청한 화면을 찾을 수 없습니다.".into())),
            }
        }
    }
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help") {
        writeln!(std::io::stdout(), "filmcraft-shorts-web [--port 8787] [--data-dir DIR] [--worker PATH]\nLocal browser application; binds to 127.0.0.1 only.")
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    let mut port = 8787u16;
    let mut root = filmcraft_engine::autosave::default_data_dir().ok_or("data directory unavailable")?.join("shorts-web");
    let mut worker =
        std::env::current_exe().map_err(|e| e.to_string())?.with_file_name(if cfg!(windows) { "filmcraft-shorts.exe" } else { "filmcraft-shorts" });
    let mut it = args.iter();
    while let Some(k) = it.next() {
        let v = it.next().ok_or("option requires value")?;
        match k.as_str() {
            "--port" => port = v.parse().map_err(|_| "invalid port")?,
            "--data-dir" => root = PathBuf::from(v),
            "--worker" => worker = PathBuf::from(v),
            _ => return Err("unknown option".into()),
        }
    }
    if port == 0 {
        return Err("port must be greater than zero".into());
    }
    if !worker.is_file() {
        return Err("Build filmcraft-shorts before starting the web app.".into());
    }
    std::fs::create_dir_all(root.join("uploads")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(root.join("jobs")).map_err(|e| e.to_string())?;
    root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut records = BTreeMap::new();
    for entry in std::fs::read_dir(root.join("jobs")).map_err(|e| e.to_string())?.take(1000) {
        let entry = entry.map_err(|e| e.to_string())?;
        if let Ok(mut r) = filmcraft_shorts::read_json::<Record>(&entry.path().join("record.json")) {
            if !valid_id(&r.id) || entry.file_name().to_str() != Some(r.id.as_str()) {
                continue;
            }
            if r.status == "running" {
                r.status = "interrupted".into();
                r.error = Some("서버가 다시 시작되어 작업 상태를 확인할 수 없습니다. 이전 작업의 종료 여부를 확인한 후 새 작업을 시작해 주세요.".into());
            }
            records.insert(r.id.clone(), r);
        }
    }
    let app = Arc::new(App { root, worker, port, records: Mutex::new(records), active: Mutex::new(None), upload: Mutex::new(()) });
    let server = Arc::new(Server::http(("127.0.0.1", port)).map_err(|e| e.to_string())?);
    writeln!(std::io::stdout(), "FilmCraft Shorts web server started on local port {port}.").map_err(|e| e.to_string())?;
    let stopping = app.clone();
    ctrlc::set_handler(move || {
        if let Ok(mut active) = stopping.active.lock()
            && let Some((id, child)) = active.as_mut()
        {
            let _ = child.kill();
            let _ = child.wait();
            if let Ok(mut records) = stopping.records.lock()
                && let Some(record) = records.get_mut(id)
            {
                record.status = "interrupted".into();
                record.error = Some("서버가 종료되어 편집이 중단되었습니다. 원본 파일은 보존됩니다.".into());
                let _ = save_record(&stopping, record);
            }
        }
        std::process::exit(0);
    })
    .map_err(|e| e.to_string())?;
    let supervisor = app.clone();
    std::thread::spawn(move || {
        loop {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| refresh(&supervisor)));
            std::thread::sleep(Duration::from_secs(1));
        }
    });
    let mut threads = Vec::new();
    for _ in 0..4 {
        let app = app.clone();
        let server = server.clone();
        threads.push(std::thread::spawn(move || {
            while let Ok(mut request) = server.recv() {
                let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| route(&mut request, &app)))
                    .unwrap_or_else(|_| Err((500, "요청 처리 중 오류가 발생했습니다.".into())));
                let response = match response {
                    Ok(v) => v,
                    Err((code, message)) => json_reply(code, json!({"error":message})),
                };
                let _ = request.respond(response);
            }
        }));
    }
    for thread in threads {
        thread.join().map_err(|_| "HTTP worker stopped")?;
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        let _ = writeln!(std::io::stderr(), "filmcraft-shorts-web: {e}");
        std::process::exit(1);
    }
}
