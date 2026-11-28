// Register macOS system CJK font collections (such as PingFang.ttc stored under
// /System/Library/AssetsV2 on macOS 15+/26) into Slint's shared fontique collection.
// fontique 0.7 only scans {~,/,/System}/Library/Fonts, so CoreText fallback names
// pointing to MobileAsset fonts fail to resolve and render CJK glyphs as tofu.

use std::collections::HashSet;
use std::ffi::{CStr, c_void};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use i_slint_common::sharedfontique::{self, fontique, ttf_parser};

#[repr(C)]
#[derive(Clone, Copy)]
struct CFRange {
    location: isize,
    length: isize,
}

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_CT_FONT_UI_FONT_SYSTEM: u32 = 2;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
    fn CFStringCreateWithBytes(
        alloc: *const c_void,
        bytes: *const u8,
        num_bytes: isize,
        encoding: u32,
        is_external_representation: u8,
    ) -> *const c_void;
    fn CFStringGetLength(the_string: *const c_void) -> isize;
    fn CFURLGetFileSystemRepresentation(
        url: *const c_void,
        resolve_against_base: u8,
        buffer: *mut u8,
        max_buf_len: isize,
    ) -> u8;
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    static kCTFontURLAttribute: *const c_void;
    fn CTFontCreateWithName(
        name: *const c_void,
        size: f64,
        matrix: *const c_void,
    ) -> *const c_void;
    fn CTFontCreateUIFontForLanguage(
        ui_type: u32,
        size: f64,
        language: *const c_void,
    ) -> *const c_void;
    fn CTFontCreateForStringWithLanguage(
        current_font: *const c_void,
        string: *const c_void,
        range: CFRange,
        language: *const c_void,
    ) -> *const c_void;
    fn CTFontCopyAttribute(font: *const c_void, attribute: *const c_void) -> *const c_void;
}

struct CfGuard(*const c_void);

impl CfGuard {
    fn new(ptr: *const c_void) -> Option<Self> {
        if ptr.is_null() { None } else { Some(Self(ptr)) }
    }
    fn as_ptr(&self) -> *const c_void {
        self.0
    }
}

impl Drop for CfGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

fn cf_string(text: &str) -> Option<CfGuard> {
    let ptr = unsafe {
        CFStringCreateWithBytes(
            std::ptr::null(),
            text.as_ptr(),
            text.len() as isize,
            K_CF_STRING_ENCODING_UTF8,
            0,
        )
    };
    CfGuard::new(ptr)
}

fn font_file_path(font: *const c_void) -> Option<PathBuf> {
    if font.is_null() {
        return None;
    }
    let url = CfGuard::new(unsafe { CTFontCopyAttribute(font, kCTFontURLAttribute) })?;
    let mut buf = vec![0u8; 4096];
    let ok = unsafe {
        CFURLGetFileSystemRepresentation(url.as_ptr(), 1, buf.as_mut_ptr(), buf.len() as isize)
    };
    if ok == 0 {
        return None;
    }
    let cstr = CStr::from_bytes_until_nul(&buf).ok()?;
    let path_str = cstr.to_str().ok()?;
    if path_str.is_empty() {
        None
    } else {
        Some(PathBuf::from(path_str))
    }
}

fn discover_cjk_font_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |p: PathBuf| {
        if p.is_file() && seen.insert(p.clone()) {
            paths.push(p);
        }
    };

    // 1. Ask CoreText directly for PingFang SC and system zh-Hans CJK fallback.
    for name in ["PingFang SC", "PingFangSC-Regular", "Hiragino Sans GB"] {
        if let Some(cf_name) = cf_string(name) {
            if let Some(font) =
                CfGuard::new(unsafe { CTFontCreateWithName(cf_name.as_ptr(), 14.0, std::ptr::null()) })
            {
                if let Some(path) = font_file_path(font.as_ptr()) {
                    push(path);
                }
            }
        }
    }

    if let (Some(lang), Some(sample)) = (cf_string("zh-Hans"), cf_string("设备连接与管理热点局域网"))
    {
        if let Some(base) = CfGuard::new(unsafe {
            CTFontCreateUIFontForLanguage(K_CT_FONT_UI_FONT_SYSTEM, 14.0, lang.as_ptr())
        }) {
            let len = unsafe { CFStringGetLength(sample.as_ptr()) };
            if let Some(resolved) = CfGuard::new(unsafe {
                CTFontCreateForStringWithLanguage(
                    base.as_ptr(),
                    sample.as_ptr(),
                    CFRange { location: 0, length: len },
                    lang.as_ptr(),
                )
            }) {
                if let Some(path) = font_file_path(resolved.as_ptr()) {
                    push(path);
                }
            }
        }
    }

    // 2. Known macOS system CJK font locations as a final safety net.
    for fallback in [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
    ] {
        push(PathBuf::from(fallback));
    }

    paths
}

fn family_covers_cjk(
    collection: &mut sharedfontique::Collection,
    family_id: fontique::FamilyId,
    infos: &[fontique::FontInfo],
) -> bool {
    let Some(info) = infos.first() else {
        return false;
    };
    let Some(qf) = collection.get_font_for_info(family_id, info) else {
        return false;
    };
    let Ok(face) = ttf_parser::Face::parse(qf.blob.data(), qf.index) else {
        return false;
    };
    if !['设', '备', '热', '点'].iter().all(|&ch| face.glyph_index(ch).is_some_and(|g| g.0 != 0)) {
        return false;
    }
    femtovg::TextContext::default()
        .add_shared_font_with_index(qf.blob.clone(), qf.index)
        .is_ok()
}

fn family_priority(name: &str) -> u8 {
    let lower = name.to_ascii_lowercase();
    if lower == "pingfang sc" {
        0
    } else if lower == ".pingfang sc" {
        1
    } else if lower.contains("pingfang") && lower.contains("sc") {
        2
    } else if lower == "hiragino sans gb" {
        3
    } else if lower.contains("heiti") && lower.contains("sc") {
        4
    } else if lower.contains("pingfang") {
        5
    } else {
        10
    }
}

fn register_path(
    collection: &mut sharedfontique::Collection,
    path: &Path,
) -> Option<(fontique::FamilyId, String, u8)> {
    let bytes = std::fs::read(path).ok()?;
    let registered = collection.register_fonts(bytes.into(), None);
    let mut best: Option<(fontique::FamilyId, String, u8)> = None;
    for (family_id, infos) in registered {
        let Some(name) = collection.family_name(family_id).map(str::to_owned) else {
            continue;
        };
        if !family_covers_cjk(collection, family_id, &infos) {
            continue;
        }
        let prio = family_priority(&name);
        if best.as_ref().is_none_or(|(_, _, bp)| prio < *bp) {
            best = Some((family_id, name, prio));
        }
    }
    best
}

static REGISTERED_FAMILY: OnceLock<Option<String>> = OnceLock::new();

/// Discover and register the native macOS CJK font collection (e.g. PingFang SC from
/// MobileAssets on macOS 15+/26) into Slint's shared `fontique` collection and attach
/// it to CJK script fallbacks. Returns the exact registered family name.
pub fn register_macos_system_cjk_fonts() -> Option<String> {
    REGISTERED_FAMILY
        .get_or_init(|| {
            let paths = discover_cjk_font_paths();
            let mut collection = sharedfontique::get_collection();
            let mut best: Option<(fontique::FamilyId, String, u8)> = None;
            for path in paths {
                if let Some(cand) = register_path(&mut collection, &path) {
                    let is_ideal = cand.2 <= 1;
                    if best.as_ref().is_none_or(|(_, _, bp)| cand.2 < *bp) {
                        best = Some(cand);
                    }
                    if is_ideal {
                        break;
                    }
                }
            }
            let (family_id, family_name, _) = best?;

            // Attach as fallback for Han/CJK scripts and locales so both UI text and
            // monospace terminal cells never render Chinese glyphs as tofu.
            for script in ["Hani", "Bopo", "Hira", "Kana"] {
                collection.append_fallbacks(
                    fontique::FallbackKey::new(fontique::Script::from(script), None),
                    std::iter::once(family_id),
                );
            }
            for locale in ["zh-Hans", "zh-CN", "zh-Hant", "ja"] {
                collection.append_fallbacks(
                    fontique::FallbackKey::from(("Hani", locale)),
                    std::iter::once(family_id),
                );
            }
            for generic in [
                fontique::GenericFamily::SansSerif,
                fontique::GenericFamily::SystemUi,
                fontique::GenericFamily::UiSansSerif,
            ] {
                collection.append_generic_families(generic, std::iter::once(family_id));
            }
            Some(family_name)
        })
        .clone()
}

/// Verify that a registered macOS font family resolves for Regular (400) and Semibold (600),
/// loads cleanly into FemtoVG, and maps every non-whitespace character in `sample` to a
/// non-zero glyph ID.
pub fn verify_macos_cjk_glyphs(family: &str, sample: &str) -> Result<(), String> {
    let mut collection = sharedfontique::get_collection();
    let family_id = collection
        .family_id(family)
        .ok_or_else(|| format!("family '{family}' not found in shared fontique collection"))?;

    let text_context = femtovg::TextContext::default();
    for weight in [400.0_f32, 600.0_f32] {
        let mut query = collection.query();
        query.set_families(std::iter::once(fontique::QueryFamily::from(family_id)));
        query.set_attributes(fontique::Attributes {
            weight: fontique::FontWeight::new(weight),
            style: fontique::FontStyle::Normal,
            width: fontique::FontWidth::NORMAL,
        });
        let mut matched: Option<fontique::QueryFont> = None;
        query.matches_with(|qf| {
            matched = Some(qf.clone());
            fontique::QueryStatus::Stop
        });
        let qf = matched.ok_or_else(|| format!("no match for '{family}' weight {weight}"))?;
        let face = ttf_parser::Face::parse(qf.blob.data(), qf.index)
            .map_err(|e| format!("ttf_parser failed for '{family}' index {}: {e}", qf.index))?;
        for ch in sample.chars().filter(|c| !c.is_whitespace()) {
            let gid = face.glyph_index(ch).map(|g| g.0).unwrap_or(0);
            if gid == 0 {
                return Err(format!(
                    "missing glyph (tofu) for '{ch}' (U+{:04X}) in '{family}' weight {weight}",
                    ch as u32
                ));
            }
        }
        text_context
            .add_shared_font_with_index(qf.blob.clone(), qf.index)
            .map_err(|e| format!("femtovg failed to load '{family}' index {}: {e}", qf.index))?;
    }

    let has_han_fallback = collection
        .fallback_families(fontique::FallbackKey::new(fontique::Script::from("Hani"), None))
        .next()
        .is_some();
    if !has_han_fallback {
        return Err("fontique Hani fallback list is empty".into());
    }
    Ok(())
}
