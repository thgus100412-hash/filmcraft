//! Read only fonts advertised by the current OS registry; never inspect a font manager's cache.
use filmcraft_text::fonts::{FaceData, FaceInfo, FontSource};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

const MAX_FONTS: usize = 4096;
const MAX_FONT_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Catalog {
    pub native_registry: bool,
    pub fonts: Vec<Entry>,
    pub aliases: BTreeMap<String, String>,
    pub unavailable: usize,
    pub warning: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub family: String,
    pub styles: Vec<String>,
    pub source: String,
}
struct ActiveSource(Vec<FaceInfo>);
impl FontSource for ActiveSource {
    fn faces(&self) -> Vec<FaceInfo> {
        self.0.clone()
    }
}
struct Snapshot {
    paths: BTreeSet<(PathBuf, u32)>,
    faces: Vec<FaceInfo>,
    aliases: BTreeMap<String, String>,
    unavailable: usize,
    warning: Option<String>,
}
impl Snapshot {
    fn from_paths(paths: impl IntoIterator<Item = (PathBuf, u32)>) -> Self {
        let mut result = Self { paths: BTreeSet::new(), faces: Vec::new(), aliases: BTreeMap::new(), unavailable: 0, warning: None };
        let mut cached = BTreeMap::new();
        for (path, index) in paths.into_iter().take(MAX_FONTS) {
            if !result.paths.insert((path.clone(), index)) {
                continue;
            }
            let names = cached.entry(path.clone()).or_insert_with(|| {
                let Ok(meta) = std::fs::metadata(&path) else { return Vec::new() };
                if !meta.is_file() || meta.len() > MAX_FONT_BYTES {
                    return Vec::new();
                }
                let Ok(mut file) = std::fs::File::open(&path) else { return Vec::new() };
                filmcraft_text::sfnt::read_faces(&mut file)
            });
            let Some(name) = names.iter().find(|n| n.index == index) else {
                result.unavailable += 1;
                continue;
            };
            result.aliases.insert(name.full_name.clone(), name.family.clone());
            result.faces.push(FaceInfo {
                family: name.family.clone(),
                style: name.style.clone(),
                weight: name.weight,
                italic: name.italic,
                index,
                data: FaceData::File(path),
                origin: "macos-active",
            });
        }
        result
    }
    #[cfg(any(target_os = "macos", test))]
    fn family_for_path(&self, path: &std::path::Path, index: u32) -> Option<String> {
        self.faces.iter().find(|f| f.index == index && matches!(&f.data, FaceData::File(p) if p == path)).map(|f| f.family.clone())
    }
}
#[cfg(target_os = "macos")]
fn native_snapshot() -> Result<Snapshot, String> {
    use font_kit::{handle::Handle, source::SystemSource};
    std::panic::catch_unwind(|| {
        let source = SystemSource::new();
        let handles = source.all_fonts().map_err(|_| "macOS 활성 글꼴 목록을 읽지 못했습니다.".to_string())?;
        let total = handles.len();
        let mut fileless = 0usize;
        let paths: Vec<_> = handles
            .into_iter()
            .take(MAX_FONTS)
            .filter_map(|h| match h {
                Handle::Path { path, font_index } => Some((path, font_index)),
                Handle::Memory { .. } => {
                    fileless = fileless.saturating_add(1);
                    None // No copying/extracting fileless or protected font resources.
                }
            })
            .collect();
        let mut snapshot = Snapshot::from_paths(paths);
        snapshot.unavailable = snapshot.unavailable.saturating_add(fileless);
        if total > MAX_FONTS {
            snapshot.warning = Some("글꼴이 너무 많아 처음 4096개만 조회했습니다.".into());
        }
        for name in ["SDCheongRyu", "GangwonEduAll-OTFBold"] {
            if let Ok(Handle::Path { path, font_index }) = source.select_by_postscript_name(name)
                && let Some(family) = snapshot.family_for_path(&path, font_index)
            {
                snapshot.aliases.insert(name.into(), family);
            }
        }
        Ok(snapshot)
    })
    .unwrap_or_else(|_| Err("macOS 글꼴 조회 중 오류가 발생했습니다. 산돌구름 활성 상태를 확인하고 다시 시도하세요.".into()))
}
#[cfg(not(target_os = "macos"))]
fn native_snapshot() -> Result<Snapshot, String> {
    Ok(Snapshot::from_paths(Vec::new()))
}

/// A fresh OS query. The HTTP server runs this in a fresh helper process so deactivated fonts
/// cannot survive in the text engine's intentionally append-only face database.
pub fn catalog() -> Result<Catalog, String> {
    let snapshot = native_snapshot()?;
    let native = cfg!(target_os = "macos");
    if native {
        filmcraft_text::fonts::add_source(&ActiveSource(snapshot.faces.clone()));
    } else {
        filmcraft_text::fonts::scan_system();
    }
    let mut entries = BTreeMap::<String, Entry>::new();
    for face in filmcraft_text::fonts::all_faces() {
        if native
            && face.info.origin != "bundled"
            && !(face.info.origin == "macos-active"
                && snapshot.faces.iter().any(|f| f.family == face.info.family && f.style == face.info.style && f.index == face.info.index))
        {
            continue;
        }
        let e = entries.entry(face.info.family.clone()).or_insert_with(|| Entry {
            family: face.info.family.clone(),
            styles: Vec::new(),
            source: face.info.origin.into(),
        });
        if !e.styles.contains(&face.info.style) {
            e.styles.push(face.info.style.clone());
        }
    }
    Ok(Catalog {
        native_registry: native,
        fonts: entries.into_values().collect(),
        aliases: snapshot.aliases,
        unavailable: snapshot.unavailable,
        warning: snapshot.warning,
    })
}

/// Resolve against the current activation snapshot before loading any font into a new worker.
/// An inactive subscription font is an error, never an unnoticed substitution.
pub fn resolve_requested(name: &str, available: &Catalog) -> Result<String, String> {
    if let Some(entry) = available.fonts.iter().find(|e| e.family.eq_ignore_ascii_case(name)) {
        return Ok(entry.family.clone());
    }
    if let Some((_, family)) = available.aliases.iter().find(|(alias, _)| alias.eq_ignore_ascii_case(name)) {
        return Ok(family.clone());
    }
    Err(format!(
        "글꼴 `{name}`이 현재 활성 목록에 없거나 읽을 수 없습니다. 산돌구름에서 활성화한 뒤 ‘글꼴 목록 새로고침’을 누르고 실제 글꼴 이름을 선택하세요."
    ))
}

/// Verify shaping/rasterization with the exact face; per-character fallback must not mask a
/// missing Korean glyph. Returns metrics, never the font data or a path.
pub fn check(name: &str) -> Result<serde_json::Value, String> {
    if name.trim().is_empty() || name.len() > 256 {
        return Err("글꼴 이름을 확인해 주세요.".into());
    }
    let available = catalog()?;
    let family = resolve_requested(name, &available)?;
    let sample = "가나다라마바사 ABC 123";
    let resolved = filmcraft_text::resolve(&family, "SemiBold");
    let face = filmcraft_text::fonts::face(resolved.face);
    if resolved.missing || !face.covers_text(sample) {
        return Err("선택한 글꼴로 한국어 예문을 표시할 수 없습니다. 다른 활성 글꼴을 선택하세요.".into());
    }
    let style = filmcraft_text::TextStyle { family: family.clone(), style: "SemiBold".into(), size: 48.0, ..Default::default() };
    let layout = filmcraft_text::layout(sample, &style, &Default::default());
    let (mask, _, _) = filmcraft_text::render::rasterize(&layout, &filmcraft_text::render::at(0.0, 64.0));
    if !mask.a.iter().any(|a| *a > 0.0) {
        return Err("글꼴은 목록에 있지만 자막 이미지를 만들 수 없습니다.".into());
    }
    Ok(serde_json::json!({"family":family,"korean":true,"rasterized":true}))
}

/// Run a font probe with bounded time/output. An unnamed temporary file avoids stdout pipe
/// deadlocks with large font lists and never publishes or keeps font paths/bytes.
pub fn probe(worker: &std::path::Path, args: &[&str]) -> Result<serde_json::Value, String> {
    use std::{
        io::{Read, Seek, SeekFrom},
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    const MAX: u64 = 2 * 1024 * 1024;
    let mut output = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut child = Command::new(worker)
        .args(args)
        .stdout(Stdio::from(output.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "글꼴 조회 도구를 시작하지 못했습니다.".to_string())?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < Duration::from_secs(30) && output.metadata().is_ok_and(|m| m.len() <= MAX) => {
                std::thread::sleep(Duration::from_millis(25))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("글꼴 조회 시간 또는 목록 크기를 초과했습니다. 활성 글꼴 수를 줄여 다시 시도하세요.".into());
            }
        }
    };
    if !status.success() {
        return Err("macOS 글꼴 조회를 완료하지 못했습니다. 산돌구름과 프로그램의 실행 상태를 확인하세요.".into());
    }
    output.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    output.take(MAX + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX {
        return Err("글꼴 목록이 너무 큽니다.".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "글꼴 조회 응답을 읽지 못했습니다.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_registered_faces_are_read_and_refresh_removes_deactivated_entries() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts/Inter-Regular.ttf");
        let active = Snapshot::from_paths([(file.clone(), 0), (file, u32::MAX), (PathBuf::from("/not/a/font"), 0)]);
        assert_eq!(active.faces.len(), 1);
        assert_eq!(active.faces[0].family, "Inter");
        assert_eq!(active.unavailable, 2);
        assert!(Snapshot::from_paths(Vec::new()).faces.is_empty());
        assert!(active.family_for_path(std::path::Path::new("/not/a/font"), 0).is_none());
    }
    #[test]
    fn aliases_require_an_active_readable_face_and_korean_check_rejects_latin_only() {
        let mut c = Catalog {
            native_registry: true,
            fonts: vec![Entry { family: "Inter".into(), styles: vec!["Regular".into()], source: "bundled".into() }],
            aliases: BTreeMap::new(),
            unavailable: 0,
            warning: None,
        };
        c.aliases.insert("Inter-Regular".into(), "Inter".into());
        assert_eq!(resolve_requested("Inter-Regular", &c).unwrap(), "Inter");
        assert!(resolve_requested("Inactive Sandoll Font", &c).is_err());
        assert!(check("Inter").is_err(), "Latin-only fonts must not pass a Korean render check via fallback");
    }
}
