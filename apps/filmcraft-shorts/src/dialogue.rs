//! Short, source-timed cues. Long ASR units are split proportionally within their own interval;
//! that fallback is an estimate, not acoustic forced alignment.
use filmcraft_project::Word;
use filmcraft_time::TICKS_PER_SECOND as SEC;
pub fn cues(words: &[Word], offset: i64, max_chars: usize) -> Result<Vec<(i64, i64, String)>, String> {
    let max_chars = max_chars.clamp(1, 16);
    let mut out = Vec::new();
    let mut text = String::new();
    let mut begin = 0;
    let mut end = 0;
    let mut speaker = None;
    for word in words {
        let chars: Vec<_> = word.text.trim().chars().collect();
        if chars.is_empty() {
            continue;
        }
        if chars.len() > 16000 {
            return Err("한 발언의 자막 텍스트가 너무 깁니다.".into());
        }
        let duration = word.end.0.checked_sub(word.start.0).filter(|d| *d > 0).ok_or("잘못된 발언 시간입니다.")?;
        let pieces = chars
            .len()
            .div_ceil(max_chars)
            .max(((duration as i128 + 2 * SEC as i128 - 1) / (2 * SEC as i128)) as usize)
            .min(chars.len())
            .min(usize::try_from(duration).unwrap_or(usize::MAX));
        for i in 0..pieces {
            let from = i * chars.len() / pieces;
            let to = (i + 1) * chars.len() / pieces;
            let part: String = chars.get(from..to).ok_or("잘못된 자막 범위입니다.")?.iter().collect();
            let start = word
                .start
                .0
                .checked_add((duration as i128 * from as i128 / chars.len() as i128) as i64)
                .and_then(|t| t.checked_add(offset))
                .ok_or("자막 시간 초과")?;
            let stop = word
                .start
                .0
                .checked_add((duration as i128 * to as i128 / chars.len() as i128) as i64)
                .and_then(|t| t.checked_add(offset))
                .ok_or("자막 시간 초과")?;
            if !text.is_empty()
                && (text.chars().count() + part.chars().count() + 1 > max_chars
                    || stop - begin > 2 * SEC
                    || start - end > SEC / 3
                    || speaker != word.speaker
                    || i > 0)
            {
                out.push((begin, end, std::mem::take(&mut text)));
            }
            if text.is_empty() {
                begin = start;
            } else {
                text.push(' ');
            }
            text.push_str(part.trim());
            end = stop;
            speaker = word.speaker;
            if part.ends_with(['.', '?', '!', '。', '？', '！']) {
                out.push((begin, end, std::mem::take(&mut text)));
            }
        }
    }
    if !text.is_empty() {
        out.push((begin, end, text));
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    use filmcraft_time::Tick;
    #[test]
    fn pauses_speakers_and_long_korean_units_keep_cut_relative_times() {
        let mut a = Word::new("다시 시작했어요", Tick(SEC), Tick(2 * SEC));
        a.speaker = Some(0);
        let mut b = Word::new("끝까지 해보려고요", Tick(3 * SEC), Tick(4 * SEC));
        b.speaker = Some(1);
        let c = Word::new("가나다라마바사아자차카타파하가나다라마바사아자차카타파하", Tick(4 * SEC), Tick(8 * SEC));
        let result = cues(&[a, b, c], -SEC, 10).unwrap();
        assert_eq!(result[0], (0, SEC, "다시 시작했어요".into()));
        assert_eq!(result[1].0, 2 * SEC);
        assert!(result.iter().all(|(a, b, t)| b > a && t.chars().count() <= 10));
        assert!(result.windows(2).all(|w| w[0].1 <= w[1].0));
        assert_eq!(result.last().unwrap().1, 7 * SEC);
    }
}
