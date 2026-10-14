//! The Kobo plan and import counts use the settings page's paged list.

use super::entries::{self, Entry};
use crate::nickel::{Counts, Plan, Report};
use crate::services::Services;
use crate::surfaces::common::{page, root_window};
use kaia_gfx::Size;
use kaia_i18n::{format as tr_format, tr};
use kaia_ui::components::Expanded;
use kaia_ui::{WidgetPod, Window};

fn kinds(counts: &Counts, matched: bool) -> Vec<(String, usize)> {
    vec![
        (
            if matched { tr("Books matched") } else { tr("Books") },
            counts.books,
        ),
        (tr("Highlights"), counts.highlights),
        (tr("Notes"), counts.notes),
        (tr("Collections"), counts.collections),
        (tr("Words"), counts.words),
        (tr("Ratings"), counts.ratings),
        (tr("Sessions"), counts.sessions),
    ]
}

pub(super) fn build(
    services: &Services,
    screen: Size,
    plan: Option<&Plan>,
    report: Option<&Report>,
) -> Window {
    let mut rows = Vec::new();
    if let Some(plan) = plan {
        rows.push(Entry::Text(tr("These are source counts. Missing books, deleted records and records already imported will be skipped. Highlights need a matching passage in the book.")));
        for (label, count) in kinds(&plan.counts, true) {
            rows.push(Entry::Text(tr_format(
                &tr("%1: %2"),
                &[&label, &count.to_string()],
            )));
        }
        rows.push(Entry::Text(tr_format(
            &tr("Kobo schema version: %1"),
            &[&plan.version.to_string()],
        )));
        for (table, count) in &plan.tables {
            rows.push(Entry::Text(tr_format(
                &tr("%1 rows: %2"),
                &[table, &count.to_string()],
            )));
        }
        rows.push(Entry::Text(tr(
            "Reading positions and typography are not imported.",
        )));
        rows.push(Entry::Button {
            key: "kobo-confirm",
            label: tr("Continue"),
            action: "settings:data:kobo:confirm".into(),
        });
    }
    if let Some(report) = report {
        rows.push(Entry::Text(tr("Kobo import finished. Skipped counts include duplicates, missing books, deleted or invalid records, and highlights whose text was not found.")));
        for ((label, imported), (_, skipped)) in kinds(&report.imported, false)
            .into_iter()
            .zip(kinds(&report.skipped, false))
        {
            rows.push(Entry::Text(tr_format(
                &tr("%1: %2 imported, %3 skipped"),
                &[&label, &imported.to_string(), &skipped.to_string()],
            )));
        }
    }
    rows.push(Entry::Button {
        key: "kobo-close",
        label: tr("Done"),
        action: "settings:data:kobo:close".into(),
    });
    let header = WidgetPod::new(
        kaia_ui::composite::Header::new(&services.theme, tr("Import from Kobo"))
            .back("settings:data:kobo:close"),
    );
    root_window(
        services,
        screen,
        page(
            services,
            vec![
                header,
                WidgetPod::new(Expanded::new(
                    WidgetPod::new(kaia_ui::composite::PagedList::new(rows, entries::widget))
                        .with_key("kobo-import-entries"),
                    1,
                )),
            ],
        ),
    )
}
