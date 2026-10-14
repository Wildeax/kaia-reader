//! Keep the library cache in step with saved book state.
use crate::services::Services;
use kaia_book::state::{STATE_FORMAT, STATE_VERSION};
use kaia_events::{Settings, Value};
use kaia_i18n::{format as tr_format, tr};
use kaia_library::{Sort, SortKey, Status};

pub(crate) fn mark_finished(services: &mut Services, id: i64) -> Result<(), String> {
    let row = services
        .library
        .book(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| tr("Book not found"))?;
    let path = services
        .paths
        .data_dir
        .join("kaia/books")
        .join(format!("{}.json", row.partial_md5));
    match Settings::read_exact(&path) {
        Ok(Some(mut settings)) => {
            if settings.read("format").as_str() != Some(STATE_FORMAT)
                || settings.read("version").as_i64() != Some(STATE_VERSION)
            {
                return Err(tr("Cannot read this book's saved state. It was kept unchanged."));
            }
            let mut summary = settings.read("summary").as_table().cloned().unwrap_or_default();
            summary.insert("status".into(), Value::from("complete"));
            settings.save("summary", Value::Table(summary));
            settings.flush_book(&path).map_err(|e| e.to_string())?;
        }
        Ok(None) => {
            // Let first open import the sidecar before applying the status.
            services.settings.save(
                format!("book.status.{}", row.partial_md5),
                Value::from("complete"),
            );
            services
                .settings
                .flush(&services.settings_path())
                .map_err(|e| e.to_string())?;
        }
        Err(error) => {
            let backup = crate::durable::preserve(&path).map_err(|e| e.to_string())?;
            return Err(tr_format(
                &tr("Cannot read book state. Kept a copy at %1. %2"),
                &[backup.display().to_string().as_str(), error.to_string().as_str()],
            ));
        }
    }
    services
        .library
        .set_status(id, Status::Complete)
        .map_err(|e| e.to_string())
}

pub(crate) fn reconcile(services: &mut Services) -> Result<(), String> {
    let books = services
        .library
        .books(Sort::new(SortKey::Title, false), &Default::default(), 0, 100_000)
        .map_err(|e| e.to_string())?;
    for row in books {
        let path = services
            .paths
            .data_dir
            .join("kaia/books")
            .join(format!("{}.json", row.partial_md5));
        let saved = Settings::open(&path);
        if saved.read("format").as_str() != Some(STATE_FORMAT)
            || saved.read("version").as_i64() != Some(STATE_VERSION)
        {
            continue;
        }
        if let Some(percent) = saved.read("percent_finished").as_f64().filter(|p| p.is_finite()) {
            if (percent - row.percent).abs() > f64::EPSILON {
                services
                    .library
                    .record_open(row.id, percent)
                    .map_err(|e| e.to_string())?;
            }
        }
        let pending = services.settings.read(format!("book.status.{}", row.partial_md5));
        let status = pending
            .as_str()
            .or_else(|| saved.read("summary").get("status").as_str());
        let status = match status {
            Some("complete") => Status::Complete,
            Some("abandoned") => Status::Abandoned,
            Some("reading") => Status::Reading,
            _ => continue,
        };
        if row.status != status {
            services
                .library
                .set_status(row.id, status)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
