//! FFmpeg generates synthetic fixtures only; FilmCraft performs all product operations.
//! Style rendering uses bundled Inter and English text; user PSD font files are not distributed.
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
            "color=c=0x408060:size=96x64:rate=30:duration=5",
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
        std::fs::write(dir.join(format!("{name}.json")),serde_json::to_vec(&json!({"language":"en","words":[{"text":"I failed.","start":2*sec-offset,"end":5*sec/2-offset},{"text":"I tried again.","start":5*sec/2-offset,"end":3*sec-offset}]})).unwrap()).unwrap();
    }
    let job_value = json!({"caption_design":"sotam","title_font":"Inter","dialogue_font":"Inter","brief":"실패를 극복한 경험", "groups":[{"name":"interview","cameras":[{"path":"a.mov","transcript":"a.json"},{"path":"b.mov","transcript":"b.json"}],"recordings":[{"path":"guest.wav","label":"guest mic","transcript":"guest.json"},{"path":"host.wav","caption_role":"crew","label":"host mic","transcript":"host.json"}]}],"planner":{"url":"http://127.0.0.1:1/v1/chat/completions","model":"test"}});
    std::fs::write(dir.join("job.json"), serde_json::to_vec_pretty(&job_value).unwrap()).unwrap();
    let job: Job = serde_json::from_value(job_value.clone()).unwrap();
    let mut prepared = prepare(&job, &dir).unwrap();
    let starts: Vec<_> = prepared.groups[0].iter().map(|m| m.start).collect();
    assert!((starts[1] - starts[0] - sec / 2).abs() < sec / 30, "{starts:?}");
    assert!((starts[2] - starts[0] - sec).abs() < sec / 30, "{starts:?}");
    let plan: Plan = serde_json::from_value(
        json!({"title":"Starting again","segments":[{"group":0,"audio":2,"camera":1,"first_word":0,"last_word":0,"reason":"게스트의 발언"},{"group":0,"audio":3,"camera":0,"first_word":1,"last_word":1,"reason":"다른 마이크로 전환"}]}),
    )
    .unwrap();
    for design in ["standard", "sotam"] {
        let mut bad_job = job_value.clone();
        bad_job["caption_design"] = json!(design);
        let bad_job: Job = serde_json::from_value(bad_job).unwrap();
        let mut bad_plan = plan.clone();
        bad_plan.title = "\u{10ffff}".into();
        assert!(assemble(&mut prepared, &bad_job, &bad_plan).is_err(), "unsupported glyph must fail for {design}");
    }
    let mut standard_value = job_value.clone();
    standard_value["caption_design"] = json!("standard");
    let standard_job: Job = serde_json::from_value(standard_value).unwrap();
    assemble(&mut prepared, &standard_job, &plan).unwrap();
    assert!(
        prepared.session.active_sequence().unwrap().caption_tracks.iter().all(|t| t.style.font == "Inter"),
        "standard captions must use the checked selected font"
    );
    assemble(&mut prepared, &job, &plan).unwrap();
    let seq = prepared.session.active_sequence().unwrap();
    assert_eq!(seq.caption_tracks.iter().filter(|t| t.enabled).count(), 3);
    let guest = seq.caption_tracks.iter().find(|t| t.name == "말자막 게스트").unwrap();
    let crew = seq.caption_tracks.iter().find(|t| t.name == "말자막 제작진").unwrap();
    assert_eq!(guest.style.color, [255, 252, 0, 255]);
    assert_eq!(crew.style.color, [62, 249, 255, 255]);
    assert_eq!(guest.captions.len(), 1);
    assert_eq!(crew.captions.len(), 1);
    assert_eq!(guest.captions[0].text, "I failed.");
    assert_eq!(crew.captions[0].text, "I tried again.");
    assert!(guest.captions[0].end() <= crew.captions[0].start);
    assert!(!seq.caption_tracks[0].enabled, "combined SRT track must not double-burn captions");
    let title = seq.caption_tracks.iter().find(|t| t.name == "Title").unwrap();
    assert_eq!(title.captions[0].duration.0, sec);
    let heard: Vec<_> = prepared.session.active_sequence().unwrap().audio_tracks.iter().flat_map(|t| t.items.iter().map(|c| c.item.0)).collect();
    assert_eq!(heard, vec![prepared.groups[0][2].item, prepared.groups[0][3].item], "only the selected microphone for each turn is audible");
    execute(&mut prepared.session, "file.saveAs", json!({"path":dir.join("short.fcproj")})).unwrap();
    execute(&mut prepared.session, "captions.export", json!({"path":dir.join("captions.srt"),"track":"C1"})).unwrap();
    let srt = std::fs::read_to_string(dir.join("captions.srt")).unwrap();
    assert!(srt.contains("I failed.") && srt.contains("I tried again."), "{srt}");
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
    execute(&mut prepared.session, "file.exportFrame", json!({"path":dir.join("with-captions.png"),"time":sec/4})).unwrap();
    execute(&mut prepared.session, "file.exportFrame", json!({"path":dir.join("crew-captions.png"),"time":3*sec/4})).unwrap();
    execute(&mut prepared.session, "captions.hideAll", json!({})).unwrap();
    execute(&mut prepared.session, "file.exportFrame", json!({"path":dir.join("without-captions.png"),"time":sec/4})).unwrap();
    assert_ne!(
        std::fs::read(dir.join("with-captions.png")).unwrap(),
        std::fs::read(dir.join("without-captions.png")).unwrap(),
        "captions must actually affect rendered pixels"
    );
    // Solid source must reach all four corners: fit/letterbox would leave them black.
    let pixels = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(dir.join("without-captions.png"))
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .unwrap();
    assert!(pixels.status.success());
    for (x, y) in [(0, 0), (1079, 0), (0, 1919), (1079, 1919)] {
        let offset = (y * 1080 + x) * 3;
        assert!(pixels.stdout[offset..offset + 3].iter().any(|v| *v > 30), "9:16 output corner {x},{y} must be filled");
    }
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
    assert!(std::fs::read_to_string(dir.join("cli-output/captions.srt")).unwrap().contains("I failed."));
    if let Ok(path) = std::env::var("FILMCRAFT_TEST_PSD") {
        let mut template = filmcraft_shorts::psd_style::parse(&std::fs::read(path).unwrap()).unwrap();
        let korean = "Noto Sans CJK KR";
        let resolved = filmcraft_text::resolve(korean, "SemiBold");
        if !resolved.missing && filmcraft_text::fonts::face(resolved.face).covers_text("다시 시작한 이유 실패했지만") {
            // PSD metadata is real; portable Korean rendering uses an open system font.
            // The user's proprietary Sandoll fonts still require validation on their Mac.
            template.title.font = korean.into();
            template.guest.font = korean.into();
            template.crew.font = korean.into();
            let mut korean_value = job_value.clone();
            korean_value["caption_template"] = serde_json::to_value(template).unwrap();
            let korean_job: Job = serde_json::from_value(korean_value).unwrap();
            let mut korean_plan = plan.clone();
            korean_plan.title = "다시 시작한 이유".into();
            for media in prepared.groups.iter_mut().flatten() {
                media.transcript.words[0].text = "실패했지만".into();
                media.transcript.words[1].text = "다시 시작했어요.".into();
            }
            assemble(&mut prepared, &korean_job, &korean_plan).unwrap();
            execute(&mut prepared.session, "file.exportFrame", json!({"path":dir.join("korean-psd.png"),"time":sec/4})).unwrap();
            let seq = prepared.session.active_sequence().unwrap();
            let guest = seq.caption_tracks.iter().find(|t| t.name == "말자막 게스트").unwrap();
            assert_eq!(guest.style.color, [255, 252, 0, 255]);
            assert_eq!(guest.captions[0].text, "실패했지만");
            for media in prepared.groups.iter_mut().flatten() {
                media.transcript.words[0].text = "가나다라마바사아자차카타파하가나다라마바사아자차카타파하".into();
            }
            assemble(&mut prepared, &korean_job, &korean_plan).unwrap();
            let seq = prepared.session.active_sequence().unwrap();
            let guest = seq.caption_tracks.iter().find(|t| t.name == "말자막 게스트").unwrap();
            let font = filmcraft_text::TextStyle {
                family: guest.style.font.clone(),
                style: "SemiBold".into(),
                size: guest.style.size * 1920.0 / 1080.0,
                ..Default::default()
            };
            assert!(guest.captions.len() > 1, "long Korean ASR units need short cues");
            assert!(guest.captions.iter().all(|c| filmcraft_text::measure(&c.text, &font) <= 972.0), "Korean captions must fit the portrait safe width");
        }
    }
    // Keep the current-run artifacts for visual inspection / debugging.
    eprintln!("verified fixture outputs: {}", dir.display());
}
