//! Metadata-only PSD v1 text-style import. Raster pixels and font files are never imported.
//! Format: Adobe's public Photoshop File Formats Specification (PSD layer records / TySh).
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
type Result<T> = std::result::Result<T, String>;
const ERROR: &str = "PSD 텍스트 스타일을 읽을 수 없습니다. 편집 가능한 제목·제작진·게스트 텍스트 레이어가 필요합니다.";
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub title: Style,
    pub guest: Style,
    pub crew: Style,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Style {
    pub font: String,
    pub size: f64,
    pub color: String,
    pub margin: f64,
    pub outline: f64,
    pub outline_color: String,
    pub line_spacing: f64,
}
impl Template {
    pub fn validate(&self) -> Result<()> {
        for s in [&self.title, &self.guest, &self.crew] {
            if s.font.is_empty()
                || s.font.len() > 256
                || !s.size.is_finite()
                || !(4.0..=200.0).contains(&s.size)
                || !s.margin.is_finite()
                || !(0.0..=0.45).contains(&s.margin)
                || !s.outline.is_finite()
                || !(0.0..=20.0).contains(&s.outline)
                || !s.line_spacing.is_finite()
                || !(0.8..=2.0).contains(&s.line_spacing)
                || [&s.color, &s.outline_color].iter().any(|c| c.len() != 7 || !c.starts_with('#') || !c.bytes().skip(1).all(|b| b.is_ascii_hexdigit()))
            {
                return Err(ERROR.into());
            }
        }
        Ok(())
    }
}
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or(ERROR)?;
        let out = self.data.get(self.at..end).ok_or(ERROR)?;
        self.at = end;
        Ok(out)
    }
    fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn f64(&mut self) -> Result<f64> {
        let b = self.take(8)?;
        let n = f64::from_be_bytes(b.try_into().map_err(|_| ERROR)?);
        if n.is_finite() { Ok(n) } else { Err(ERROR.into()) }
    }
    fn block(&mut self) -> Result<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }
}
// EngineData is a small PostScript-like dictionary, not executable PostScript.
struct Engine<'a> {
    r: Reader<'a>,
    tokens: usize,
}
impl Engine<'_> {
    fn space(&mut self) {
        while self.r.data.get(self.r.at).is_some_and(u8::is_ascii_whitespace) {
            self.r.at += 1;
        }
    }
    fn value(&mut self, depth: usize) -> Result<Value> {
        self.tokens += 1;
        if depth > 32 || self.tokens > 100_000 {
            return Err(ERROR.into());
        }
        self.space();
        if self.r.data.get(self.r.at..).is_some_and(|b| b.starts_with(b"<<")) {
            self.r.take(2)?;
            let mut o = serde_json::Map::new();
            loop {
                self.space();
                if self.r.data.get(self.r.at..).is_some_and(|b| b.starts_with(b">>")) {
                    self.r.take(2)?;
                    break;
                }
                if self.r.take(1)? != b"/" {
                    return Err(ERROR.into());
                }
                let key = self.word()?;
                let v = self.value(depth + 1)?;
                o.insert(key, v);
            }
            return Ok(Value::Object(o));
        }
        match self.r.data.get(self.r.at).copied().ok_or(ERROR)? {
            b'[' => {
                self.r.take(1)?;
                let mut a = Vec::new();
                loop {
                    self.space();
                    if self.r.data.get(self.r.at) == Some(&b']') {
                        self.r.take(1)?;
                        break;
                    }
                    a.push(self.value(depth + 1)?);
                }
                Ok(Value::Array(a))
            }
            b'(' => {
                self.r.take(1)?;
                let mut b = Vec::new();
                loop {
                    let c = *self.r.take(1)?.first().ok_or(ERROR)?;
                    if c == b')' {
                        break;
                    }
                    if c == b'\\' {
                        let escaped = *self.r.take(1)?.first().ok_or(ERROR)?;
                        b.push(escaped);
                    } else {
                        b.push(c);
                    }
                }
                let text = if b.starts_with(&[254, 255]) {
                    if b.len() % 2 != 0 {
                        return Err(ERROR.into());
                    }
                    String::from_utf16(&b[2..].chunks_exact(2).map(|x| u16::from_be_bytes([x[0], x[1]])).collect::<Vec<_>>()).map_err(|_| ERROR)?
                } else {
                    String::from_utf8(b).map_err(|_| ERROR)?
                };
                Ok(json!(text))
            }
            _ => {
                let s = self.word()?;
                match s.as_str() {
                    "true" => Ok(json!(true)),
                    "false" => Ok(json!(false)),
                    _ => {
                        let n = s.parse::<f64>().map_err(|_| ERROR)?;
                        if !n.is_finite() {
                            return Err(ERROR.into());
                        }
                        Ok(json!(n))
                    }
                }
            }
        }
    }
    fn word(&mut self) -> Result<String> {
        let start = self.r.at;
        while self.r.data.get(self.r.at).is_some_and(|c| !c.is_ascii_whitespace() && !b"<>[]()/".contains(c)) {
            self.r.at += 1;
        }
        if self.r.at == start {
            return Err(ERROR.into());
        }
        String::from_utf8(self.r.data.get(start..self.r.at).ok_or(ERROR)?.to_vec()).map_err(|_| ERROR.into())
    }
}
fn embedded<'a>(data: &'a [u8], marker: &[u8]) -> Result<&'a [u8]> {
    let start = data.windows(marker.len()).position(|w| w == marker).ok_or(ERROR)? + marker.len();
    Reader::new(data.get(start..).ok_or(ERROR)?).block()
}
fn number(v: &Value) -> Result<f64> {
    v.as_f64().filter(|n| n.is_finite()).ok_or(ERROR.into())
}
fn rgb(values: &Value) -> Result<String> {
    let a = values.as_array().ok_or(ERROR)?;
    let c: Vec<_> = a.iter().skip(1).take(3).map(number).collect::<Result<_>>()?;
    if c.len() != 3 || c.iter().any(|n| !(0.0..=1.0).contains(n)) {
        return Err(ERROR.into());
    }
    Ok(format!("#{:02x}{:02x}{:02x}", (c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8))
}
fn effect_number(data: &[u8], marker: &[u8]) -> Option<f64> {
    let pos = data.windows(marker.len()).position(|w| w == marker)? + marker.len();
    Reader::new(data.get(pos..)?).f64().ok()
}
fn style(ty: &[u8], effects: &[u8], top: f64, bottom: f64, height: f64, title: bool) -> Result<Style> {
    let mut r = Reader::new(ty);
    if r.u16()? != 1 {
        return Err(ERROR.into());
    }
    let xx = r.f64()?;
    let xy = r.f64()?;
    let yx = r.f64()?;
    let yy = r.f64()?;
    if xy.abs() > 0.001 || yx.abs() > 0.001 || xx <= 0.0 || yy <= 0.0 || (xx / yy - 1.0).abs() > 0.01 {
        return Err("회전·왜곡된 PSD 텍스트는 지원하지 않습니다.".into());
    }
    let bytes = embedded(ty, b"EngineDatatdta")?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(ERROR.into());
    }
    let root = Engine { r: Reader::new(bytes), tokens: 0 }.value(0)?;
    let runs = root["EngineDict"]["StyleRun"]["RunArray"].as_array().ok_or(ERROR)?;
    if runs.len() != 1 {
        return Err("한 레이어에 여러 글꼴·색상이 섞인 PSD는 아직 지원하지 않습니다.".into());
    }
    let data = &runs[0]["StyleSheet"]["StyleSheetData"];
    let index = number(&data["Font"])?;
    if !(0.0..=4096.0).contains(&index) || index.fract() != 0.0 {
        return Err(ERROR.into());
    }
    let font = root["ResourceDict"]["FontSet"].get(index as usize).and_then(|f| f["Name"].as_str()).ok_or(ERROR)?.to_string();
    let px = number(&data["FontSize"])? * yy;
    let leading = data["Leading"].as_f64().unwrap_or(number(&data["FontSize"])? * 1.2) * yy;
    let effects = effects.windows(8).position(|w| w == b"FrFXObjc").and_then(|p| effects.get(p..)).unwrap_or(&[]);
    let enabled = effects.windows(9).any(|w| w == b"enabbool\x01");
    let effects = if enabled { effects } else { &[] };
    let outline = effect_number(effects, b"Sz  UntF#Pxl").unwrap_or(0.0) * 1080.0 / height;
    let colors: Vec<_> =
        [b"Rd  doub", b"Grn doub", b"Bl  doub"].iter().map(|m| effect_number(effects, *m).unwrap_or(0.0).clamp(0.0, 255.0).round() as u8).collect();
    Ok(Style {
        font,
        size: px * 1080.0 / height,
        color: rgb(&data["FillColor"]["Values"])?,
        margin: if title { top / height } else { (height - bottom) / height },
        outline,
        outline_color: format!("#{:02x}{:02x}{:02x}", colors[0], colors[1], colors[2]),
        line_spacing: (leading / px).clamp(0.8, 2.0),
    })
}
/// Read the three editable type layers. The uppermost type layer is the title; dialogue layer
/// names must contain 게스트/제작진 (or guest/crew). Ambiguous files fail rather than guessing.
pub fn parse(bytes: &[u8]) -> Result<Template> {
    if bytes.len() > 128 * 1024 * 1024 {
        return Err("PSD는 128MB 이하로 올려주세요.".into());
    }
    let mut r = Reader::new(bytes);
    if r.take(4)? != b"8BPS" || r.u16()? != 1 {
        return Err(ERROR.into());
    }
    r.take(8)?;
    let h = r.u32()? as f64;
    let w = r.u32()? as f64;
    r.take(4)?;
    if h < 1.0 || w < 1.0 || (w / h - 9.0 / 16.0).abs() > 0.01 {
        return Err("9:16 캔버스의 PSD를 올려주세요.".into());
    }
    r.block()?;
    r.block()?;
    let mut mask = Reader::new(r.block()?);
    let mut layers = Reader::new(mask.block()?);
    let count = (layers.u16()? as i16).unsigned_abs() as usize;
    if count > 512 {
        return Err(ERROR.into());
    }
    let mut title = None;
    let mut guest = None;
    let mut crew = None;
    for _ in 0..count {
        let top = layers.u32()? as i32 as f64;
        layers.take(4)?;
        let bottom = layers.u32()? as i32 as f64;
        layers.take(4)?;
        let channels = layers.u16()? as usize;
        if channels > 64 {
            return Err(ERROR.into());
        }
        layers.take(channels * 6)?;
        layers.take(12)?;
        let mut extra = Reader::new(layers.block()?);
        extra.block()?;
        extra.block()?;
        let len = *extra.take(1)?.first().ok_or(ERROR)? as usize;
        let fallback = String::from_utf8_lossy(extra.take(len)?).to_string();
        extra.take((4 - (len + 1) % 4) % 4)?;
        let mut name = fallback;
        let mut ty = None;
        let mut effects = &[][..];
        while extra.data.len().saturating_sub(extra.at) >= 12 {
            if extra.take(4)? != b"8BIM" {
                return Err(ERROR.into());
            }
            let key = extra.take(4)?;
            let data = extra.block()?;
            if data.len() % 2 != 0 {
                extra.take(1)?;
            }
            match key {
                b"luni" => {
                    let mut u = Reader::new(data);
                    let n = u.u32()? as usize;
                    if n > 1024 {
                        return Err(ERROR.into());
                    }
                    name =
                        String::from_utf16(&u.take(n * 2)?.chunks_exact(2).map(|b| u16::from_be_bytes([b[0], b[1]])).collect::<Vec<_>>()).map_err(|_| ERROR)?;
                }
                b"TySh" => ty = Some(data),
                b"lfx2" => effects = data,
                _ => {}
            }
        }
        if let Some(ty) = ty {
            let lower = name.to_lowercase();
            let is_guest = lower.contains("게스트") || lower.contains("guest");
            let is_crew = lower.contains("제작진") || lower.contains("crew");
            if top < 0.0 || bottom <= top || bottom > h {
                return Err(ERROR.into());
            }
            let slot = if is_guest {
                &mut guest
            } else if is_crew {
                &mut crew
            } else if top < h * 0.3 {
                &mut title
            } else {
                continue;
            };
            if slot.is_some() {
                return Err("제목·제작진·게스트 텍스트 레이어를 각각 하나씩 남겨주세요.".into());
            }
            *slot = Some(style(ty, effects, top, bottom, h, !is_guest && !is_crew)?);
        }
    }
    let result = Template { title: title.ok_or(ERROR)?, guest: guest.ok_or(ERROR)?, crew: crew.ok_or(ERROR)? };
    result.validate()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn block(bytes: &[u8]) -> Vec<u8> {
        let mut o = (bytes.len() as u32).to_be_bytes().to_vec();
        o.extend(bytes);
        o
    }
    fn fixture() -> Vec<u8> {
        let engine=b"<< /EngineDict << /StyleRun << /RunArray [ << /StyleSheet << /StyleSheetData << /Font 0 /FontSize 96 /FillColor << /Values [ 1 1 0.98822 0 ] >> >> >> >> ] >> >> /ResourceDict << /FontSet [ << /Name (Inter) >> ] >> >>";
        let mut records = 3i16.to_be_bytes().to_vec();
        for (name, top, bottom) in [("Title", 100i32, 300i32), ("guest", 1200, 1300), ("crew", 1200, 1300)] {
            for n in [top, 100, bottom, 980] {
                records.extend(n.to_be_bytes());
            }
            records.extend(0u16.to_be_bytes());
            records.extend(b"8BIMnorm\xff\0\0\0");
            let mut extra = vec![0; 8];
            extra.push(name.len() as u8);
            extra.extend(name.as_bytes());
            while !extra.len().is_multiple_of(4) {
                extra.push(0);
            }
            let mut ty = 1u16.to_be_bytes().to_vec();
            for n in [1f64, 0., 0., 1., 0., 0.] {
                ty.extend(n.to_be_bytes());
            }
            ty.extend(b"EngineDatatdta");
            ty.extend(block(engine));
            extra.extend(b"8BIMTySh");
            extra.extend(block(&ty));
            if !ty.len().is_multiple_of(2) {
                extra.push(0);
            }
            records.extend(block(&extra));
        }
        let mut file = b"8BPS\0\x01\0\0\0\0\0\0\0\x03".to_vec();
        file.extend(1920u32.to_be_bytes());
        file.extend(1080u32.to_be_bytes());
        file.extend(b"\0\x08\0\x03");
        file.extend([0; 8]);
        file.extend(block(&block(&records)));
        file
    }
    #[test]
    fn text_metadata_roundtrip_and_hostile_truncations() {
        let data = fixture();
        let t = parse(&data).unwrap();
        assert_eq!(t.guest.font, "Inter");
        assert_eq!(t.guest.color, "#fffc00");
        assert_eq!(t.title.size, 54.0);
        for n in 0..data.len() {
            assert!(parse(&data[..n]).is_err());
        }
        let mut invalid = t;
        invalid.title.size = f64::NAN;
        assert!(invalid.validate().is_err());
        for i in (0..data.len()).step_by(13) {
            let mut damaged = data.clone();
            damaged[i] = 255;
            let _ = parse(&damaged);
        }
    }
    #[test]
    fn actual_user_psd_when_supplied_locally() {
        let Ok(path) = std::env::var("FILMCRAFT_TEST_PSD") else { return };
        let t = parse(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(t.title.font, "SDCheongRyu");
        assert_eq!(t.guest.font, "GangwonEduAll-OTFBold");
        assert_eq!(t.title.color, "#3ef9ff");
        assert_eq!(t.crew.color, "#3ef9ff");
        assert_eq!(t.guest.color, "#fffc00");
        assert!((t.title.size - 67.5).abs() < 0.1);
        assert!((t.guest.size - 51.815).abs() < 0.1);
        assert_eq!(t.title.outline, 2.8125);
        assert_eq!(t.title.outline_color, "#22232b");
        eprintln!("Private PSD styles verified: {t:?}");
    }
}
