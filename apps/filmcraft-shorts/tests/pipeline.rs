//! FFmpeg generates synthetic fixtures only; FilmCraft performs all product operations.
use filmcraft_shorts::{Job, Plan, assemble, execute, prepare};
use serde_json::json;
use std::process::Command;

fn fixture(args: &[&str], cwd: &std::path::Path) {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("ffmpeg is required for this integration test");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}
#[test]
fn actual_multicam_sync_external_mic_and_burned_caption_export() {
    let dir = std::env::temp_dir().join(format!("filmcraft-shorts-pipeline-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    fixture(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=96x64:rate=30:duration=5",
            "-f",
            "lavfi",
            "-i",
            "anoisesrc=sample_rate=48000:duration=5:seed=41:amplitude=0.2",
            "-c:v",
            "libx264",
            "-profile:v",
            "baseline",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "pcm_s16le",
            "a.mov",
        ],
        &dir,
    );
    fixture(&["-ss", "0.5", "-i", "a.mov", "-t", "4", "-c:v", "libx264", "-profile:v", "baseline", "-c:a", "pcm_s16le", "b.mov"], &dir);
    fixture(&["-ss", "1", "-i", "a.mov", "-t", "3", "-vn", "-c:a", "pcm_s16le", "guest.wav"], &dir);
    fixture(&["-ss", "0.5", "-i", "a.mov", "-t", "4", "-vn", "-c:a", "pcm_s16le", "host.wav"], &dir);
    let sec = filmcraft_time::TICKS_PER_SECOND;
    for (name, offset) in [("a", 0), ("b", sec / 2), ("guest", sec), ("host", sec / 2)] {
        std::fs::write(dir.join(format!("{name}.json")),serde_json::to_vec(&json!({"language":"ko","words":[{"text":"실패했지만","start":2*sec-offset,"end":5*sec/2-offset},{"text":"다시 시작했어요.","start":5*sec/2-offset,"end":3*sec-offset}]})).unwrap()).unwrap();
    }
    let job_value = json!({"brief":"실패를 극복한 경험", "groups":[{"name":"interview","cameras":[{"path":"a.mov","transcript":"a.json"},{"path":"b.mov","transcript":"b.json"}],"recordings":[{"path":"guest.wav","label":"guest mic","transcript":"guest.json"},{"path":"host.wav","label":"host mic","transcript":"host.json"}]}],"planner":{"url":"http://127.0.0.1:1/v1/chat/completions","model":"test"}});
    std::fs::write(dir.join("job.json"), serde_json::to_vec_pretty(&job_value).unwrap()).unwrap();
    let job: Job = serde_json::from_value(job_value).unwrap();
    let mut prepared = prepare(&job, &dir).unwrap();
    let starts: Vec<_> = prepared.groups[0].iter().map(|m| m.start).collect();
    assert!((starts[1] - starts[0] - sec / 2).abs() < sec / 30, "{starts:?}");
    assert!((starts[2] - starts[0] - sec).abs() < sec / 30, "{starts:?}");
    let plan: Plan = serde_json::from_value(
        json!({"title":"다시 시작한 이유","segments":[{"group":0,"audio":2,"camera":1,"first_word":0,"last_word":0,"reason":"게스트의 발언"},{"group":0,"audio":3,"camera":0,"first_word":1,"last_word":1,"reason":"다른 마이크로 전환"}]}),
    )
    .unwrap();
    assemble(&mut prepared, &job, &plan).unwrap();
    let heard: Vec<_> = prepared.session.active_sequence().unwrap().audio_tracks.iter().flat_map(|t| t.items.iter().map(|c| c.item.0)).collect();
    assert_eq!(heard, vec![prepared.groups[0][2].item, prepared.groups[0][3].item], "only the selected microphone for each turn is audible");
    execute(&mut prepared.session, "file.saveAs", json!({"path":dir.join("short.fcproj")})).unwrap();
    execute(&mut prepared.session, "captions.export", json!({"path":dir.join("captions.srt"),"track":"C1"})).unwrap();
    let srt = std::fs::read_to_string(dir.join("captions.srt")).unwrap();
    assert!(srt.contains("실패했지만"), "{srt}");
    let result = execute(
        &mut prepared.session,
        "file.exportMedia",
        json!({"path":dir.join("short.mp4"),"format":"h264","width":108,"height":192,"burnCaptions":true,"audio":true,"wait":true,"hardwareEncoding":"off"}),
    )
    .unwrap();
    assert_eq!(result["offlineMedia"], json!([]), "{result}");
    assert!(std::fs::metadata(dir.join("short.mp4")).unwrap().len() > 1000);
    let probe = Command::new("ffprobe").args(["-v", "error", "-show_streams", "-of", "json"]).arg(dir.join("short.mp4")).output().unwrap();
    assert!(probe.status.success());
    let info: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
    let streams = info["streams"].as_array().unwrap();
    assert!(streams.iter().any(|v| v["codec_type"] == "video" && v["width"] == 108 && v["height"] == 192));
    assert!(streams.iter().any(|v| v["codec_type"] == "audio"));
    execute(&mut prepared.session, "file.exportFrame", json!({"path":dir.join("with-captions.png"),"seconds":0.5})).unwrap();
    execute(&mut prepared.session, "captions.hideAll", json!({})).unwrap();
    execute(&mut prepared.session, "file.exportFrame", json!({"path":dir.join("without-captions.png"),"seconds":0.5})).unwrap();
    assert_ne!(
        std::fs::read(dir.join("with-captions.png")).unwrap(),
        std::fs::read(dir.join("without-captions.png")).unwrap(),
        "captions must actually affect rendered pixels"
    );
    std::fs::write(dir.join("plan.json"), serde_json::to_vec(&plan).unwrap()).unwrap();
    let cli = Command::new(env!("CARGO_BIN_EXE_filmcraft-shorts"))
        .arg(dir.join("job.json"))
        .arg(dir.join("cli-output"))
        .arg("--plan")
        .arg(dir.join("plan.json"))
        .arg("--render")
        .output()
        .unwrap();
    assert!(cli.status.success(), "CLI: {}", String::from_utf8_lossy(&cli.stderr));
    assert!(dir.join("cli-output/short.mp4").is_file());
    assert!(std::fs::read_to_string(dir.join("cli-output/captions.srt")).unwrap().contains("실패했지만"));
    // Keep the current-run artifacts for visual inspection / debugging.
    eprintln!("verified fixture outputs: {}", dir.display());
}
