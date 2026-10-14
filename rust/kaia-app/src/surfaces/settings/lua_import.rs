//! The Kaia (Lua app) plan and import counts use the settings page's paged list.

use super::entries::{self, Entry};
use crate::lua_import::{Counts, Plan, Report};
use crate::services::Services;
use crate::surfaces::common::{page, root_window};
use kaia_gfx::Size;
use kaia_i18n::{format as tr_format, tr};
use kaia_ui::components::Expanded;
use kaia_ui::{WidgetPod, Window};

fn kinds(counts: &Counts, other: bool) -> Vec<(String, usize)> {
    crate::lua_import::KINDS
        .iter()
        .filter(|kind| **kind != "Other annotations" || other)
        .map(|kind| (label(kind), counts.get(kind)))
        .collect()
}

/// The label of a count kind. Literal `tr` calls, so the string extractor finds each one.
fn label(kind: &str) -> String {
    match kind {
        "Settings" => tr("Settings"),
        "History entries" => tr("History entries"),
        "Collection entries" => tr("Collection entries"),
        "Book states" => tr("Book states"),
        "Highlights" => tr("Highlights"),
        "Bookmarks" => tr("Bookmarks"),
        "Notes" => tr("Notes"),
        "Stickers" => tr("Stickers"),
        "Assets" => tr("Assets"),
        "Statistics books" => tr("Statistics books"),
        "Page records" => tr("Page records"),
        "Plugin data" => tr("Plugin data"),
        "Words" => tr("Words"),
        "Other annotations" => tr("Other annotations"),
        other => other.to_string(),
    }
}

pub(super) fn build(
    services: &Services,
    screen: Size,
    plan: Option<&Plan>,
    report: Option<&Report>,
) -> Window {
    let mut rows = Vec::new();
    if let Some(plan) = plan {
        if plan.shared_store {
            rows.push(Entry::Text(tr(
                "Stickers, assets and words already use the same store. They will be skipped.",
            )));
        }
        rows.push(Entry::Text(tr(
            "Review the source counts. Existing Rust data takes priority. The source files stay unchanged.",
        )));
        // Keep counts in one block so the confirmation button stays visible.
        let counts: Vec<String> = kinds(&plan.counts, plan.counts.get("Other annotations") > 0)
            .into_iter()
            .map(|(label, count)| tr_format(&tr("%1: %2"), &[&label, &count.to_string()]))
            .collect();
        rows.push(Entry::Text(counts.join("\n")));
        rows.push(Entry::Text(tr(
            "Close the Lua app before importing. Plugin changes apply after a restart.",
        )));
        rows.push(Entry::Button {
            key: "lua-confirm",
            label: tr("Import"),
            action: "settings:data:lua:confirm".into(),
        });
    }
    if let Some(report) = report {
        rows.push(Entry::Text(tr(
            "Import finished. Skipped counts include existing data and missing or unsupported books.",
        )));
        let lines: Vec<String> = kinds(&report.imported, report.skipped.get("Other annotations") > 0)
            .into_iter()
            .zip(kinds(
                &report.skipped,
                report.skipped.get("Other annotations") > 0,
            ))
            .map(|((label, imported), (_, skipped))| {
                tr_format(
                    &tr("%1: %2 imported, %3 skipped"),
                    &[&label, &imported.to_string(), &skipped.to_string()],
                )
            })
            .collect();
        rows.push(Entry::Text(lines.join("\n")));
    }
    rows.push(Entry::Button {
        key: "lua-close",
        // Before the import this button leaves without importing.
        label: if report.is_some() {
            tr("Done")
        } else {
            tr("Cancel")
        },
        action: "settings:data:lua:close".into(),
    });
    let header = WidgetPod::new(
        kaia_ui::composite::Header::new(&services.theme, tr("Import from Kaia (Lua app)"))
            .back("settings:data:lua:close"),
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
                        .with_key("lua-import-entries"),
                    1,
                )),
            ],
        ),
    )
}
