//! Shared font database. Loading system fonts is slow, so it happens once and every
//! [`FontSystem`] (layout measurer, renderer, possibly on different threads) clones it.

use std::sync::Arc;

use glyphon::{FontSystem, fontdb};

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

/// Cheap-to-clone handle to a loaded font database.
#[derive(Clone)]
pub struct FontContext {
    locale: String,
    database: Arc<fontdb::Database>,
}

impl FontContext {
    /// Loads the system fonts and picks a platform UI font as the default family.
    pub fn from_system() -> Self {
        let (locale, mut database) = FontSystem::new().into_locale_and_db();
        if let Some(family) = installed_family(&database, PREFERRED_SANS_SERIF) {
            database.set_sans_serif_family(family);
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
        database.faces().any(|face| {
            face.families
                .iter()
                .any(|(family, _)| family.eq_ignore_ascii_case(candidate))
        })
    })
}
