//! Shared font database. Loading system fonts is slow, so it happens once and every
//! [`FontSystem`] (layout measurer, renderer, possibly on different threads) clones it.
//!
//! [`FontOptions::new`] keeps every face from that scan. [`FontOptions::with_families`] keeps
//! cappy's UI families plus the caller's names, and [`FontOptions::with_files`] adds font files
//! after that filter.

use std::path::PathBuf;
use std::sync::Arc;

use cosmic_text::{FontSystem, fontdb};

/// Which faces to keep from the system font directories, plus any extra font files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontOptions {
    families: Option<Vec<String>>,
    files: Vec<PathBuf>,
}

impl FontOptions {
    /// Keeps every installed face and loads no extra files.
    pub fn new() -> Self {
        Self::default()
    }

    /// After the system scan, keeps cappy's UI families plus `families`.
    #[must_use]
    pub fn with_families(mut self, families: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.families = Some(families.into_iter().map(Into::into).collect());
        self
    }

    /// Loads these font files after the system scan and any family filter.
    ///
    /// A file is kept even when its family is not on the allowlist. A missing or malformed
    /// file is logged and skipped.
    #[must_use]
    pub fn with_files(mut self, files: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        self.files.extend(files.into_iter().map(Into::into));
        self
    }
}

/// Preferred UI families, tried in order; the first one installed becomes `sans-serif`.
const PREFERRED_SANS_SERIF: &[&str] = &[
    "Segoe UI",
    "SF Pro Text",
    "Helvetica Neue",
    "Inter",
    "Noto Sans",
    "Cantarell",
    "Ubuntu",
    "DejaVu Sans",
    "Liberation Sans",
    "Arial",
];

/// Emoji and symbol families the platform fallback asks for after the UI face.
const SYMBOL_FAMILIES: &[&str] = &[
    "Segoe UI Emoji",
    "Segoe UI Symbol",
    "Segoe UI Historic",
    "Noto Color Emoji",
    "Noto Sans Symbols",
    "Noto Sans Symbols 2",
    "Apple Color Emoji",
];

/// Preferred monospace families, tried in order; the first one installed becomes `monospace`.
const PREFERRED_MONOSPACE: &[&str] = &[
    "Cascadia Mono",
    "Cascadia Code",
    "Consolas",
    "SF Mono",
    "Menlo",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Noto Sans Mono",
    "Courier New",
];

/// Cheap-to-clone handle to a loaded font database.
#[derive(Clone)]
pub struct FontContext {
    locale: String,
    database: Arc<fontdb::Database>,
}

impl FontContext {
    /// Loads every system font and picks a platform UI font as the default family.
    pub fn from_system() -> Self {
        Self::load(&FontOptions::new())
    }

    /// Loads fonts as `options` describes and picks a platform UI font as the default family.
    ///
    /// When a family filter matches nothing and no font file loads, the full scan is kept so
    /// text still has a face to shape with.
    pub fn load(options: &FontOptions) -> Self {
        let (locale, mut database) = FontSystem::new().into_locale_and_db();
        if let Some(families) = &options.families {
            restrict_to_families(&mut database, families, &options.files);
        } else {
            load_font_files(&mut database, &options.files);
        }
        if let Some(family) = installed_family(&database, PREFERRED_SANS_SERIF) {
            database.set_sans_serif_family(family);
        }
        if let Some(family) = installed_family(&database, PREFERRED_MONOSPACE) {
            database.set_monospace_family(family);
        }
        Self::from_database(locale, database)
    }

    /// Uses an already prepared database, e.g. bundled fonts for reproducible tests.
    pub fn from_database(locale: String, database: fontdb::Database) -> Self {
        Self {
            locale,
            database: Arc::new(database),
        }
    }

    /// A fresh font system sharing this context's fonts.
    pub fn font_system(&self) -> FontSystem {
        FontSystem::new_with_locale_and_db(self.locale.clone(), (*self.database).clone())
    }
}

impl std::fmt::Debug for FontContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontContext")
            .field("locale", &self.locale)
            .field("faces", &self.database.len())
            .finish()
    }
}

fn installed_family<'a>(database: &fontdb::Database, candidates: &[&'a str]) -> Option<&'a str> {
    candidates.iter().copied().find(|candidate| {
        database
            .faces()
            .any(|face| face_has_family(face, candidate))
    })
}

fn restrict_to_families(database: &mut fontdb::Database, families: &[String], files: &[PathBuf]) {
    let scanned = database.len();
    let mut extras = fontdb::Database::new();
    let loaded = load_font_files(&mut extras, files);
    if !allowlist_matches(database, families) && loaded == 0 {
        log::warn!(
            "none of the requested UI fonts are installed; keeping all {scanned} font faces"
        );
        return;
    }
    drop_unlisted_faces(database, families);
    append_faces(database, extras);
    log::info!("font database kept {} of {scanned} faces", database.len());
}

/// Loads each file into `database`. Returns how many faces were added.
///
/// A missing or unreadable file is logged and skipped.
fn load_font_files(database: &mut fontdb::Database, files: &[PathBuf]) -> usize {
    let before = database.len();
    for path in files {
        if let Err(error) = database.load_font_file(path) {
            log::warn!("failed to load font file '{}': {error}", path.display());
        }
    }
    database.len() - before
}

fn append_faces(database: &mut fontdb::Database, extras: fontdb::Database) {
    for face in extras.faces() {
        let mut info = face.clone();
        info.id = fontdb::ID::dummy();
        database.push_face_info(info);
    }
}

fn allowlist_matches(database: &fontdb::Database, families: &[String]) -> bool {
    database.faces().any(|face| face_is_allowed(face, families))
}

fn drop_unlisted_faces(database: &mut fontdb::Database, families: &[String]) {
    let remove: Vec<fontdb::ID> = database
        .faces()
        .filter(|face| !face_is_allowed(face, families))
        .map(|face| face.id)
        .collect();
    for id in remove {
        database.remove_face(id);
    }
}

fn face_is_allowed(face: &fontdb::FaceInfo, families: &[String]) -> bool {
    PREFERRED_SANS_SERIF
        .iter()
        .chain(SYMBOL_FAMILIES)
        .chain(PREFERRED_MONOSPACE)
        .any(|family| face_has_family(face, family))
        || families.iter().any(|family| face_has_family(face, family))
}

fn face_has_family(face: &fontdb::FaceInfo, family: &str) -> bool {
    face.families
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case(family))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn face(family: &str) -> fontdb::FaceInfo {
        let data: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(Vec::<u8>::new());
        fontdb::FaceInfo {
            id: fontdb::ID::dummy(),
            source: fontdb::Source::Binary(data),
            index: 0,
            families: vec![(family.to_owned(), fontdb::Language::English_UnitedStates)],
            post_script_name: family.to_owned(),
            style: fontdb::Style::Normal,
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            monospaced: false,
        }
    }

    #[test]
    fn unlisted_faces_are_dropped() {
        let mut database = fontdb::Database::new();
        database.push_face_info(face("Segoe UI"));
        database.push_face_info(face("Yu Gothic"));
        database.push_face_info(face("Segoe UI Emoji"));
        drop_unlisted_faces(&mut database, &[]);
        let names: Vec<&str> = database
            .faces()
            .filter_map(|face| face.families.first().map(|(name, _)| name.as_str()))
            .collect();
        assert_eq!(names, ["Segoe UI", "Segoe UI Emoji"]);
    }

    #[test]
    fn caller_family_is_kept() {
        let mut database = fontdb::Database::new();
        database.push_face_info(face("Segoe UI"));
        database.push_face_info(face("Yu Gothic"));
        drop_unlisted_faces(&mut database, &["Yu Gothic".to_owned()]);
        let names: Vec<&str> = database
            .faces()
            .filter_map(|face| face.families.first().map(|(name, _)| name.as_str()))
            .collect();
        assert_eq!(names, ["Segoe UI", "Yu Gothic"]);
    }

    #[test]
    fn missing_font_file_is_skipped() {
        let mut database = fontdb::Database::new();
        let loaded = load_font_files(&mut database, &[PathBuf::from("missing-font.ttf")]);
        assert_eq!(loaded, 0);
        assert!(database.is_empty());
    }
}
