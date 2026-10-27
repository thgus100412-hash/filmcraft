//! Lossless quote/word-id compaction. Only the planning copy uses milliseconds; the original
//! transcripts and all editing/validation stay in exact ticks. No sources or words are dropped.
use crate::{LIMIT, Result};
use serde_json::{Value, json};
pub fn compact(analysis: &Value) -> Result<Value> {
    let Some(groups) = analysis["groups"].as_array() else { return Err("missing analysis groups".into()) };
    if groups.len() > 100 {
        return Err("too many analysis groups".into());
    }
    let ms = |v: &Value| v.as_i64().map(|n| n / (filmcraft_time::TICKS_PER_SECOND / 1000)).ok_or_else(|| "invalid analysis time".to_string());
    let mut result = Vec::new();
    for group in groups {
        let sources = group["sources"].as_array().ok_or("missing analysis sources")?;
        if sources.len() > 32 {
            return Err("too many analysis sources".into());
        }
        let mut compact_sources = Vec::new();
        for source in sources {
            let words = source["words"].as_array().ok_or("missing source words")?;
            let mut tuples = Vec::new();
            for word in words {
                tuples.push(json!([word["id"], word["text"], ms(&word["start"])?, ms(&word["end"])?, word["speaker"]]));
            }
            compact_sources.push(json!({"id":source["id"],"label":source["label"],"caption_role":source["caption_role"],"video":source["video"],"audio":source["audio"],"start_ms":ms(&source["start"])?,"duration_ms":ms(&source["duration"])?,"words":tuples}));
        }
        result.push(json!({"id":group["id"],"name":group["name"],"sources":compact_sources}));
    }
    let result = json!({"brief":analysis["brief"],"max_seconds":analysis["max_seconds"],"time_unit":"milliseconds","word_columns":["id","text","start_ms","end_ms","source_local_speaker"],"groups":result});
    if serde_json::to_vec(&result).map_err(|e| e.to_string())?.len() > LIMIT {
        return Err("대본이 너무 큽니다. 촬영 그룹을 나누어 편집하세요.".into());
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_every_source_quote_and_word_id_without_sync_diagnostics() {
        let tick = filmcraft_time::TICKS_PER_SECOND;
        let words: Vec<_> = (0..100)
            .map(|i| json!({"id":i,"text":"다시 시작했어요.","start":i*tick,"end":(i+1)*tick,"speaker":"Speaker 1","confidence":0.987654321}))
            .collect();
        let source = json!({"id":3,"label":"게스트 마이크","caption_role":"guest","video":false,"audio":true,"start":2*tick,"duration":110*tick,"words":words});
        let full = json!({"brief":"용기","max_seconds":45,"groups":[{"id":2,"name":"take","sources":[source.clone(),source],"sync":{"private_diagnostic":"not needed"}}]});
        let compact = compact(&full).unwrap();
        assert_eq!(compact["groups"][0]["sources"].as_array().unwrap().len(), 2);
        assert_eq!(compact["groups"][0]["sources"][0]["words"][99], json!([99, "다시 시작했어요.", 99000, 100000, "Speaker 1"]));
        assert_eq!(compact["groups"][0]["sources"][0]["start_ms"], 2000);
        assert!(!compact.to_string().contains("confidence"));
        assert!(!compact.to_string().contains("private_diagnostic"));
        assert!(compact.to_string().len() < full.to_string().len() * 3 / 4);
    }
}
