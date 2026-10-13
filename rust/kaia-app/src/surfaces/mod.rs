//! The surfaces. Each folder holds one surface: its state, its widget tree and
//! the commands it parses. This file holds the routes and the table that builds a
//! surface from a route. Adding a screen means one variant in [`Route`], one line
//! in [`create`], and the surface's own folder. See the crate README.

pub mod common;
pub mod home;
pub mod library;
pub mod notes;
pub mod reader;
pub mod settings;

use crate::surface::Surface;

/// A screen the app can show. Home, Library, Notes and Settings are roots. The
/// others are pushed above a root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Home,
    Library {
        page: usize,
    },
    Notes,
    Settings,
    Reader {
        book: i64,
    },
    /// The reader's overlay: a transparent layer with a Back button.
    Overlay,
    ReaderPanel,
    BookDetails {
        book: i64,
    },
    SettingsSection {
        name: String,
    },
}

impl Route {
    /// Home, Library, Notes and Settings.
    pub fn is_top_level(&self) -> bool {
        matches!(
            self,
            Route::Home | Route::Library { .. } | Route::Notes | Route::Settings
        )
    }
}

/// The surface for a route. None for [`Route::Overlay`]: the reader builds its
/// overlay with the title and page it has, and pushes it.
pub fn create(route: &Route) -> Option<Box<dyn Surface>> {
    let surface: Box<dyn Surface> = match route {
        Route::Home => Box::new(home::HomeSurface::default()),
        Route::Library { page } => Box::new(library::LibrarySurface::new(*page)),
        Route::Notes => Box::new(notes::NotesSurface),
        Route::Settings => Box::new(settings::SettingsSurface),
        Route::Reader { book } => Box::new(reader::ReaderSurface::new(*book)),
        Route::BookDetails { book } => Box::new(library::details::BookDetailsSurface::new(*book)),
        Route::SettingsSection { name } => {
            Box::new(settings::section::SettingsSectionSurface::new(name.clone()))
        }
        Route::Overlay | Route::ReaderPanel => return None,
    };
    Some(surface)
}
