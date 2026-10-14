//! Storage totals and confirmed deletion by source.

use std::path::PathBuf;

use kaia_events::{Value, WindowId};
use kaia_i18n::{format as tr_format, tr, tr_n};
use kaia_library::storage::{DeleteReport, Source, Sources, StorageBook, StorageGroup};
use kaia_ui::composite::{Dialog, DialogButton};
use kaia_ui::Window;

use super::entries::Entry;
use crate::services::Services;
use crate::surface::{Outcome, Surface, SurfaceEnv};
use crate::surfaces::Route;

/// Both download clients use `network.downloads`. Calibre also writes the
/// received paths to its metadata cache. Invalid cache paths confer no source.
pub fn sources(services: &Services) -> std::io::Result<Sources> {
    let downloads = download_folder(services);
    let downloads = std::fs::canonicalize(&downloads).or_else(|_| std::path::absolute(&downloads))?;
    // Strict: a damaged cache must stop deletes, not hide which books came from Calibre.
    let calibre_books = kaia_calibre::read_metadata_cache_strict(&downloads)?
        .iter()
        .filter_map(|book| book.get("lpath").and_then(|value| value.as_str()))
        .filter(|path| kaia_calibre::check_lpath(path).is_ok())
        .map(|path| downloads.join(path))
        .collect();
    Ok(Sources {
        calibre_books,
        downloads,
    })
}

fn download_folder(services: &Services) -> PathBuf {
    services
        .settings
        .read("network.downloads")
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| services.paths.books_dir.clone())
}

/// Explicit scan roots, settings folders and the configured download folder
/// are the app's books folders. None of these grants access to hidden paths.
pub fn roots(services: &Services) -> Vec<PathBuf> {
    let mut roots = services.library_roots.clone();
    roots.extend(
        super::facts::library_folders(&services.settings, &services.paths.books_dir)
            .into_iter()
            .map(PathBuf::from),
    );
    roots.push(download_folder(services));
    roots
        .into_iter()
        .filter(|root| {
            !root.components().any(|part| match part {
                std::path::Component::Normal(name) => name.to_string_lossy().starts_with('.'),
                std::path::Component::ParentDir => true,
                _ => false,
            })
        })
        .filter(|root| !std::fs::symlink_metadata(root).is_ok_and(|metadata| metadata.is_symlink()))
        .map(|root| std::fs::canonicalize(&root).unwrap_or(root))
        .collect()
}

fn name(source: Source) -> String {
    match source {
        Source::Calibre => tr("Calibre"),
        Source::Opds => tr("OPDS"),
        Source::Other => tr("Other"),
    }
}

fn exact_bytes(bytes: u64) -> String {
    tr_format(
        &tr_n("%1 byte", "%1 bytes", bytes.min(i64::MAX as u64) as i64),
        &[&bytes.to_string()],
    )
}

pub fn size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        tr_format(&tr("%1 GB"), &[&format!("{:.2}", bytes as f64 / 1_000_000_000.0)])
    } else if bytes >= 1_000_000 {
        tr_format(&tr("%1 MB"), &[&format!("{:.2}", bytes as f64 / 1_000_000.0)])
    } else if bytes >= 1_000 {
        tr_format(&tr("%1 KB"), &[&format!("{:.2}", bytes as f64 / 1_000.0)])
    } else {
        exact_bytes(bytes)
    }
}

fn row(label: String, value: String) -> Entry {
    Entry::Row {
        primary: label,
        secondary: None,
        value: Some(value),
        action: None,
    }
}

pub fn entries(services: &Services) -> Vec<Entry> {
    let mut entries = Vec::new();
    match sources(services)
        .map_err(kaia_library::LibraryError::from)
        .and_then(|sources| services.library.storage_groups(&sources))
    {
        Ok(groups) => {
            entries.push(row(
                tr("Indexed books"),
                size(groups.iter().map(|g| g.bytes).sum()),
            ));
            for group in groups {
                entries.push(Entry::Row {
                    primary: name(group.source),
                    secondary: Some(tr_format(
                        &tr_n("%1 book", "%1 books", group.books.len() as i64),
                        &[&group.books.len().to_string()],
                    )),
                    value: Some(size(group.bytes)),
                    action: None,
                });
                if !group.books.is_empty() {
                    entries.push(Entry::Button {
                        key: match group.source {
                            Source::Calibre => "storage-delete-calibre",
                            Source::Opds => "storage-delete-opds",
                            Source::Other => "storage-delete-other",
                        },
                        label: tr_format(&tr("Delete all from %1"), &[&name(group.source)]),
                        action: format!("storage:delete:{}", group.source.key()),
                    });
                }
            }
        }
        Err(_) => entries.push(Entry::Text(tr(
            "Cannot read storage groups. No books can be deleted.",
        ))),
    }
    let mut volumes = roots(services);
    if volumes.is_empty() {
        volumes.push(services.paths.books_dir.clone());
    }
    volumes.sort();
    volumes.dedup();
    for root in volumes {
        entries.push(Entry::Heading(root.display().to_string()));
        match kaia_platform::storage::volume_space(&root) {
            Ok(space) => {
                entries.push(row(tr("Free space"), size(space.available)));
                entries.push(row(tr("Total space"), size(space.total)));
            }
            Err(_) => entries.push(Entry::Text(tr("Cannot read volume space."))),
        }
    }
    entries.push(row(tr("Kaia data"), directory_size(&services.paths.data_dir)));
    let cache = services.paths.data_dir.join("cache");
    if cache.exists() {
        entries.push(row(tr("Cache, included in Kaia data"), directory_size(&cache)));
    }
    entries.push(Entry::Text(tr("Calibre uses its saved book list. OPDS includes other books in the download folder. Other includes books outside that folder. Sizes come from the library index.")));
    entries
}

fn directory_size(path: &std::path::Path) -> String {
    kaia_platform::storage::directory_bytes(path)
        .map(size)
        .unwrap_or_else(|_| tr("Cannot read size"))
}

fn reserve_book(
    data: &std::path::Path,
    book: &StorageBook,
) -> kaia_library::Result<Option<kaia_book::session::ClosedBookGuard>> {
    kaia_book::session::hold_closed_book(&book.path, data, &book.partial_md5)
        .map_err(|error| kaia_library::LibraryError::InvalidArgument(error.to_string()))
}

pub fn delete_one(services: &mut Services, id: i64) -> kaia_library::Result<bool> {
    let book = services
        .library
        .book(id)?
        .ok_or(kaia_library::LibraryError::NotFound(id))?;
    let book = StorageBook {
        id,
        path: book.path,
        bytes: book.size,
        partial_md5: book.partial_md5,
    };
    let Some(_book_guard) = reserve_book(&services.paths.data_dir, &book)? else {
        return Ok(false);
    };
    let roots = roots(services);
    let _scan_lock = crate::durable::lock(&services.paths.data_dir.join("nickel-import.lock"), false)?;
    services.library.delete_indexed_file(&book, &roots)?;
    Ok(true)
}

pub fn delete_group(services: &mut Services, group: &StorageGroup) -> kaia_library::Result<DeleteReport> {
    let sources = sources(services)?;
    let roots = roots(services);
    let data = services.paths.data_dir.clone();
    let _scan_lock = crate::durable::lock(&data.join("nickel-import.lock"), false)?;
    let mut book_guard = None;
    Ok(services
        .library
        .delete_storage_group(group, &sources, &roots, |book| {
            // Release the previous file before reserving the next. Duplicate content
            // can share a state file without either copy being open in the reader.
            drop(book_guard.take());
            book_guard = reserve_book(&data, book)?;
            Ok(book_guard.is_none())
        }))
}

#[derive(Default)]
pub struct StorageSurface {
    pending: Option<(WindowId, StorageGroup)>,
}

impl Surface for StorageSurface {
    fn route(&self) -> Route {
        Route::SettingsSection {
            name: "storage".into(),
        }
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        Some(super::section::build(
            env.services,
            env.screen,
            "storage",
            false,
            0,
        ))
    }

    fn on_command(&mut self, cmd: &str, _args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        if cmd == "storage:cancel" || (cmd == "back" && self.pending.is_some()) {
            if let Some((window, _)) = self.pending.take() {
                env.ctx.close(window, None);
            }
            return Outcome::Handled;
        }
        if cmd == "storage:confirm" {
            let Some((window, group)) = self.pending.take() else {
                return Outcome::Handled;
            };
            env.ctx.close(window, None);
            match delete_group(env.services, &group) {
                Ok(report) => {
                    let message = tr_format(
                        &tr("%1 deleted, %2 failed, %3 skipped because open."),
                        &[
                            &report.deleted.to_string(),
                            &report.failed.to_string(),
                            &report.skipped_open.to_string(),
                        ],
                    );
                    env.notice(message.clone());
                    env.toast(message, 6.0);
                }
                Err(_) => {
                    env.toast(
                        tr("Cannot delete books. Wait for the library scan to finish and try again."),
                        6.0,
                    );
                }
            }
            return Outcome::Rebuild;
        }
        let Some(key) = cmd.strip_prefix("storage:delete:") else {
            return Outcome::NotMine;
        };
        if self.pending.is_some() {
            return Outcome::Handled;
        }
        let groups = sources(env.services)
            .map_err(kaia_library::LibraryError::from)
            .and_then(|sources| env.services.library.storage_groups(&sources));
        let Ok(groups) = groups else {
            env.toast(tr("Cannot read storage groups. No books can be deleted."), 5.0);
            return Outcome::Handled;
        };
        if let Some(group) = groups
            .into_iter()
            .find(|g| g.source.key() == key && !g.books.is_empty())
        {
            let window = env.dialog(delete_dialog(env.theme(), &group));
            self.pending = Some((window, group));
        }
        Outcome::Handled
    }

    fn on_hide(&mut self, env: &mut SurfaceEnv<'_>) {
        if let Some((window, _)) = self.pending.take() {
            env.ctx.close(window, None);
        }
    }
}

/// Uses the frozen count and byte size that the confirm action will delete.
pub fn delete_dialog(theme: &kaia_ui::Theme, group: &StorageGroup) -> Dialog {
    let message = tr_format(&tr_n("Delete %1 book from %2, %3? This removes the file from the device. Highlights and notes stay in Kaia's data. Open books are skipped.", "Delete %1 books from %2, %3? This removes the files from the device. Highlights and notes stay in Kaia's data. Open books are skipped.", group.books.len() as i64), &[
        &group.books.len().to_string(), &name(group.source), &exact_bytes(group.bytes),
    ]);
    Dialog::new(
        theme,
        tr("Delete books?"),
        message,
        vec![
            DialogButton::new(tr("Cancel"), "storage:cancel"),
            DialogButton::new(tr("Delete books"), "storage:confirm"),
        ],
    )
}
