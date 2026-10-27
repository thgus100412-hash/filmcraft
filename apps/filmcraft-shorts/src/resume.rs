//! Reuse verified source/project snapshots after a failed planner call. Never rerun Whisper or
//! waveform alignment on a retry, and refuse changed input jobs or changed source files.
use crate::{Job, Media, Prepared, Result, execute, read_json};
use filmcraft_engine::Session;
use filmcraft_project::MediaRef;
use serde_json::{Value, json};
use std::path::Path;
pub fn restore(job: &Job, job_path: &Path, cache: &Path) -> Result<Prepared> {
    let expected =
        if cache.join("input-job.json").is_file() { cache.join("input-job.json") } else { cache.parent().ok_or("missing cached job")?.join("job.json") };
    if read_json::<Value>(job_path)? != read_json::<Value>(&expected)? {
        return Err("작업 설정이 달라 분석을 재사용할 수 없습니다. 새 편집을 시작하세요.".into());
    }
    let analysis: Value = read_json(&cache.join("analysis.json"))?;
    if analysis["brief"].as_str() != Some(job.brief.as_str()) || analysis["max_seconds"].as_u64() != Some(u64::from(job.max_seconds)) {
        return Err("cached job analysis mismatch".into());
    }
    let mut session = Session::default();
    execute(&mut session, "file.open", json!({"path":cache.join("sources.fcproj")}))?;
    let data = analysis["groups"].as_array().ok_or("missing cached groups")?;
    if data.len() != job.groups.len() {
        return Err("cached group mismatch".into());
    }
    let mut groups = Vec::new();
    for (index, group) in job.groups.iter().enumerate() {
        if data.get(index).and_then(|g| g["id"].as_u64()) != Some(index as u64) {
            return Err("cached group id mismatch".into());
        }
        let rows = data.get(index).and_then(|g| g["sources"].as_array()).ok_or("missing cached sources")?;
        if rows.len() != group.cameras.len() + group.recordings.len() {
            return Err("cached source mismatch".into());
        }
        let mut media = Vec::new();
        for (source_index, (source, row)) in group.cameras.iter().chain(&group.recordings).zip(rows).enumerate() {
            if row["id"].as_u64() != Some(source_index as u64) {
                return Err("cached source id mismatch".into());
            }
            let path =
                job_path.parent().unwrap_or(Path::new(".")).join(&source.path).canonicalize().map_err(|_| "원본 파일이 없어 분석을 재사용할 수 없습니다.")?;
            let (id, item) = session
                .project
                .items
                .iter()
                .filter_map(|(id, item)| item.as_media().map(|m| (*id, m)))
                .find(|(_, m)| matches!(&m.media,MediaRef::File{path:p} if Path::new(p).canonicalize().ok().as_ref()==Some(&path)))
                .ok_or("cached media missing")?;
            let identity = item.identity.ok_or("원본 파일 지문이 없는 이전 분석입니다. 새 편집을 시작하세요.")?;
            let actual =
                filmcraft_engine::relink::identity_of(session.services.as_ref(), &path.to_string_lossy()).map_err(|_| "원본 파일을 확인하지 못했습니다.")?;
            if actual != identity {
                return Err("원본 파일이 변경돼 분석을 재사용할 수 없습니다. 새 편집을 시작하세요.".into());
            }
            let transcript = session.project.transcripts.get(&id).map(|t| t.as_ref().clone()).unwrap_or_default();
            transcript.check()?;
            let words = row["words"].as_array().ok_or("missing cached words")?;
            if words.len() != transcript.words.len()
                || words.iter().zip(&transcript.words).enumerate().any(|(word_index, (a, b))| {
                    a["id"].as_u64() != Some(word_index as u64)
                        || a["speaker"].as_str() != transcript.speaker_name(b).as_deref()
                        || a["text"].as_str() != Some(b.text.as_str())
                        || a["start"].as_i64() != Some(b.start.0)
                        || a["end"].as_i64() != Some(b.end.0)
                })
            {
                return Err("cached transcript mismatch".into());
            }
            media.push(Media {
                item: id.0,
                start: row["start"].as_i64().ok_or("missing cached sync")?,
                duration: item.duration().0,
                video: item.info.has_video(),
                audio: item.info.has_audio(),
                transcript,
            });
        }
        groups.push(media);
    }
    Ok(Prepared { session, groups, analysis })
}
