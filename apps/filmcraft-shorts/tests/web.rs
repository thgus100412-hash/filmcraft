//! HTTP contract and worker lifecycle test. The fake worker tests orchestration only;
//! pipeline.rs separately exercises actual FilmCraft synchronization and MP4 encoding.
#![cfg(unix)]
use serde_json::{Value, json};
use std::{
    io::Write,
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
struct Running {
    child: Child,
    root: PathBuf,
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().proxy(None).timeout_global(Some(Duration::from_secs(5))).http_status_as_error(false).build().into()
}
fn get(url: &str) -> (u16, Value) {
    let mut r = agent().get(url).call().unwrap();
    (r.status().as_u16(), r.body_mut().read_json().unwrap())
}
fn post(url: &str, body: Value) -> (u16, Value) {
    let mut r = agent().post(url).send_json(body).unwrap();
    (r.status().as_u16(), r.body_mut().read_json().unwrap())
}
#[test]
fn uploads_jobs_cancellation_results_ranges_and_origin_boundaries() {
    let root = std::env::temp_dir().join(format!("shorts-web-test-{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap();
    let worker = root.join("worker.py");
    std::fs::write(
        &worker,
        r#"#!/usr/bin/env python3
import pathlib,sys,time,json
if sys.argv[1]=='--fonts':
 print(json.dumps({'native_registry':True,'fonts':[{'family':'Test Active','styles':['Regular'],'source':'macos-active'}],'aliases':{},'unavailable':0,'warning':None}));sys.exit(0)
if sys.argv[1]=='--check-font':
 print(json.dumps({'error':'unavailable'}));sys.exit(0)
out=pathlib.Path(sys.argv[2]);out.mkdir()
print('[shorts-stage] rendering',file=sys.stderr,flush=True)
time.sleep(0.6)
for name in ('short.mp4','captions.srt','short.fcproj'):(out/name).write_bytes(b'0123456789')
(out/'plan.json').write_text(json.dumps({'title':'테스트 제목','segments':[]}))
"#,
    )
    .unwrap();
    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let log = std::fs::File::create(root.join("server.log")).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_filmcraft-shorts-web"))
        .args(["--port", &port.to_string(), "--data-dir"])
        .arg(root.join("data"))
        .arg("--worker")
        .arg(&worker)
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log))
        .spawn()
        .unwrap();
    let running = Running { child, root };
    let base = format!("http://127.0.0.1:{port}");
    for _ in 0..100 {
        if TcpListener::bind(("127.0.0.1", port)).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    let invalid_psd = agent().post(format!("{base}/api/caption-template")).send(&b"not a psd"[..]).unwrap();
    assert_eq!(invalid_psd.status().as_u16(), 422);
    if let Ok(path) = std::env::var("FILMCRAFT_TEST_PSD") {
        let data = std::fs::read(path).unwrap();
        let mut response = agent().post(format!("{base}/api/caption-template")).send(&data[..]).unwrap();
        assert_eq!(response.status().as_u16(), 200);
        let parsed: Value = response.body_mut().read_json().unwrap();
        assert_eq!(parsed["template"]["title"]["color"], "#3ef9ff");
    }
    let (_, fonts) = get(&format!("{base}/api/fonts"));
    assert_eq!(fonts["fonts"][0]["family"], "Test Active");
    assert_eq!(post(&format!("{base}/api/fonts/check"), json!({"font":""})).0, 400);
    assert_eq!(post(&format!("{base}/api/fonts/check"), json!({"font":"x".repeat(257)})).0, 400);
    assert_eq!(post(&format!("{base}/api/fonts/check"), json!({"font":"Test Active"})).1["error"], "unavailable");
    let forbidden_font =
        agent().post(format!("{base}/api/fonts/check")).header("Origin", "https://other.example").send_json(json!({"font":"Test Active"})).unwrap();
    assert_eq!(forbidden_font.status().as_u16(), 403);
    let response = agent().get(&base).call().unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let forbidden = agent().post(format!("{base}/api/jobs")).header("Origin", "https://other.example").send_json(json!({})).unwrap();
    assert_eq!(forbidden.status().as_u16(), 403);
    let forbidden = agent().get(&base).header("Host", "evil.example").call().unwrap();
    assert_eq!(forbidden.status().as_u16(), 403);
    let bad = agent().post(format!("{base}/api/files")).header("X-File-Name", "..%2Fsecret.mp4").send(&b"test"[..]).unwrap();
    assert_eq!(bad.status().as_u16(), 400);
    let mut uploaded = agent().post(format!("{base}/api/files")).header("X-File-Name", "interview.mp4").send(&b"fixture-media"[..]).unwrap();
    assert_eq!(uploaded.status().as_u16(), 201);
    let file: Value = uploaded.body_mut().read_json().unwrap();
    let mut input = json!({"caption_design":"sotam","title_font":"Inter","dialogue_font":"Inter","brief":"다시 시작한 경험","max_seconds":45,"speech_model":"whisper-small","groups":[{"name":"테이크 1","sync":"audio","cameras":[{"file_id":file["id"],"label":"게스트","caption_role":"crew"}],"recordings":[]}]});
    let style = json!({"font":"Inter","size":54.0,"color":"#3ef9ff","margin":0.1,"outline":2.0,"outline_color":"#000000","line_spacing":1.2});
    input["caption_template"] = json!({"title":style,"guest":style,"crew":style});
    input["crew_font"] = json!("Inter");
    let mut invalid_style = input.clone();
    invalid_style["caption_template"]["title"]["size"] = json!(99999);
    assert_eq!(post(&format!("{base}/api/jobs"), invalid_style).0, 400);
    let (_, health) = get(&format!("{base}/api/health"));
    assert!(health["platform"].is_string());
    assert!(health["fonts"].as_array().unwrap().iter().any(|f| f["family"] == "Inter"));
    let mut invalid = input.clone();
    invalid["groups"][0]["cameras"][0]["caption_role"] = json!("invented");
    assert_eq!(post(&format!("{base}/api/jobs"), invalid).0, 400);
    let (code, job) = post(&format!("{base}/api/jobs"), input.clone());
    assert_eq!(code, 202, "{job}");
    let id = job["id"].as_str().unwrap();
    let manifest: Value = serde_json::from_slice(&std::fs::read(running.root.join("data/jobs").join(id).join("job.json")).unwrap()).unwrap();
    assert_eq!(manifest["caption_design"], "sotam");
    assert_eq!(manifest["caption_template"], input["caption_template"]);
    assert_eq!(manifest["crew_font"], "Inter");
    assert_eq!(manifest["groups"][0]["cameras"][0]["caption_role"], "crew");
    let (code, _) = post(&format!("{base}/api/jobs"), input.clone());
    assert_eq!(code, 409);
    let mut done = Value::Null;
    for _ in 0..100 {
        let (_, job) = get(&format!("{base}/api/jobs/{id}"));
        if job["status"] == "done" {
            done = job;
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    assert_eq!(done["title"], "테스트 제목");
    let mut result = agent().get(format!("{base}/api/jobs/{id}/files/short.mp4")).header("Range", "bytes=2-5").call().unwrap();
    assert_eq!(result.status().as_u16(), 206);
    assert_eq!(result.headers().get("Content-Range").unwrap(), "bytes 2-5/10");
    assert_eq!(result.body_mut().read_to_vec().unwrap(), b"2345");
    let response = agent().get(format!("{base}/api/jobs/{id}/files/worker.log")).call().unwrap();
    assert_eq!(response.status().as_u16(), 404);
    let (code, job) = post(&format!("{base}/api/jobs"), input);
    assert_eq!(code, 202);
    let id = job["id"].as_str().unwrap();
    let (code, _) = post(&format!("{base}/api/jobs/{id}/cancel"), json!({}));
    assert_eq!(code, 200);
    assert_eq!(get(&format!("{base}/api/jobs/{id}")).1["status"], "cancelled");
    let (_, history) = get(&format!("{base}/api/jobs"));
    assert_eq!(history["jobs"].as_array().unwrap().len(), 2);
    let mut retained = agent().delete(format!("{base}/api/files/{}", file["id"].as_str().unwrap())).call().unwrap();
    assert_eq!(retained.status().as_u16(), 200);
    assert_eq!(retained.body_mut().read_json::<Value>().unwrap()["retained"], true);
    let mut unused = agent().post(format!("{base}/api/files")).header("X-File-Name", "unused.wav").send(&b"unused"[..]).unwrap();
    let unused: Value = unused.body_mut().read_json().unwrap();
    let mut deleted = agent().delete(format!("{base}/api/files/{}", unused["id"].as_str().unwrap())).call().unwrap();
    assert_eq!(deleted.body_mut().read_json::<Value>().unwrap()["retained"], false);

    assert!(running.root.join("data/uploads").is_dir());
    let _ = std::io::stderr().write_all(format!("web test artifacts: {}\n", running.root.display()).as_bytes());
}

#[test]
fn font_helper_reports_catalog_and_rejects_missing_font_without_paths() {
    let worker = std::path::Path::new(env!("CARGO_BIN_EXE_filmcraft-shorts"));
    let catalog = filmcraft_shorts::active_fonts::probe(worker, &["--fonts"]).unwrap();
    assert!(catalog["fonts"].is_array());
    assert!(!catalog.to_string().contains(".ttf"));
    let check = filmcraft_shorts::active_fonts::probe(worker, &["--check-font", "Definitely Missing Font 123"]).unwrap();
    assert!(check["error"].is_string());
}
