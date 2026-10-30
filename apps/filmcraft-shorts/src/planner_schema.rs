//! Constrain decoding to actual source/word ids, rather than trusting prose instructions.
use crate::Result;
use serde_json::{Value, json};

fn segment() -> Value {
    json!({"type":"object","properties":{
        "group":{"type":"integer","minimum":0},"audio":{"type":"integer","minimum":0},
        "camera":{"type":"integer","minimum":0},"first_word":{"type":"integer","minimum":0},
        "last_word":{"type":"integer","minimum":0},"reason":{"type":"string","maxLength":200}},
        "required":["group","audio","camera","first_word","last_word","reason"],"additionalProperties":false})
}
fn id(value: &Value) -> Result<Value> {
    value.as_u64().map(|n| json!(n)).ok_or_else(|| "invalid planner schema id".into())
}
fn fixed(schema: &mut Value, field: &str, values: Vec<Value>) {
    schema["properties"][field] = json!({"type":"integer","enum":values});
}
pub fn response_format(input: &Value) -> Result<Value> {
    let mut choices = Vec::new();
    if let Some(candidates) = input["candidates"].as_array() {
        for candidate in candidates {
            let mut item = segment();
            for field in ["group", "audio", "camera", "first_word", "last_word"] {
                fixed(&mut item, field, vec![id(&candidate["segment"][field])?]);
            }
            choices.push(item);
        }
    } else {
        for group in input["groups"].as_array().ok_or("missing planner groups")? {
            let sources = group["sources"].as_array().ok_or("missing planner sources")?;
            let cameras: Vec<_> = sources.iter().filter(|s| s["video"] == true).map(|s| id(&s["id"])).collect::<Result<_>>()?;
            if cameras.is_empty() {
                continue;
            }
            for source in sources {
                if source["audio"] != true {
                    continue;
                }
                let words: Vec<_> = source["words"].as_array().ok_or("missing schema words")?.iter().map(|w| id(&w[0])).collect::<Result<_>>()?;
                if words.is_empty() {
                    continue;
                }
                let mut item = segment();
                fixed(&mut item, "group", vec![id(&group["id"])?]);
                fixed(&mut item, "audio", vec![id(&source["id"])?]);
                fixed(&mut item, "camera", cameras.clone());
                fixed(&mut item, "first_word", words.clone());
                fixed(&mut item, "last_word", words);
                choices.push(item);
            }
        }
    }
    let empty = choices.is_empty();
    let items = if empty { segment() } else { json!({"anyOf":choices}) };
    let limit = if empty { 0 } else { input["selection_limit"].as_u64().unwrap_or(12).min(12) };
    let schema = json!({"type":"object","properties":{
        "title":{"type":"string","minLength":1,"maxLength":60},
        "segments":{"type":"array","items":items,"maxItems":limit}},
        "required":["title","segments"],"additionalProperties":false});
    if serde_json::to_vec(&schema).map_err(|e| e.to_string())?.len() > 512 * 1024 {
        return Err("AI 응답 선택지가 너무 큽니다. 촬영 그룹을 줄여 주세요.".into());
    }
    // This exact extension is documented by the pinned llama.cpp b11669 server. Schema is a
    // decoding grammar, not extra prompt text; it does not consume the transcript context budget.
    Ok(json!({"type":"json_object","schema":schema}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nonzero_source_ids_are_bound_to_the_source_with_words() {
        let input = json!({"selection_limit":3,"groups":[{"id":8,"sources":[
            {"id":4,"video":true,"audio":true,"words":[]},
            {"id":7,"video":false,"audio":true,"words":[[42,"아니요",0,500,"guest"],[43,"근거",500,1000,"guest"]]}
        ]}]});
        let format = response_format(&input).unwrap();
        let array = &format["schema"]["properties"]["segments"];
        assert_eq!(array["maxItems"], 3);
        let choices = array["items"]["anyOf"].as_array().unwrap();
        assert_eq!(choices.len(), 1);
        let props = &choices[0]["properties"];
        assert_eq!(props["group"]["enum"], json!([8]));
        assert_eq!(props["audio"]["enum"], json!([7]));
        assert_eq!(props["camera"]["enum"], json!([4]));
        assert_eq!(props["first_word"]["enum"], json!([42, 43]));
        assert_eq!(props["last_word"]["enum"], json!([42, 43]));
        assert!(!format.to_string().contains("아니요"));
    }
    #[test]
    fn ranking_constrains_each_complete_original_candidate_and_rejects_bad_ids() {
        let mut input = json!({"selection_limit":1,"candidates":[{"segment":{"group":2,"audio":7,"camera":4,"first_word":99,"last_word":110}}]});
        let format = response_format(&input).unwrap();
        let array = &format["schema"]["properties"]["segments"];
        assert_eq!(array["maxItems"], 1);
        assert_eq!(array["items"]["anyOf"][0]["properties"]["last_word"]["enum"], json!([110]));
        input["candidates"][0]["segment"]["audio"] = json!(-1);
        assert!(response_format(&input).is_err());
        assert_eq!(response_format(&json!({"groups":[]})).unwrap()["schema"]["properties"]["segments"]["maxItems"], 0);
    }
}
