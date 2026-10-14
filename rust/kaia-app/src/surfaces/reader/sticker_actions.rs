//! Picker, note editor and sticker actions.
use super::{
    panels, show_panel,
    state::{read_field, Core},
    stickers,
};
use crate::surface::{Nav, Outcome, SurfaceEnv};
use kaia_i18n::{format as tr_format, tr};
use kaia_storage::{Sticker, StickerSize, StickerState};
use kaia_ui::widget::PaintHint;
use std::cell::RefCell;
use std::rc::Rc;

fn get(c: &Core, id: &str) -> Result<Sticker, String> {
    let s = c.session.as_ref().ok_or_else(|| tr("No book is open."))?;
    let sticker = stickers::store(s)?.get_sticker(id).map_err(|e| e.to_string())?;
    if sticker.doc.partial_md5 != s.identity().partial_md5 {
        return Err(tr("This sticker belongs to another book."));
    }
    Ok(sticker)
}

pub(super) fn open(rc: &Rc<RefCell<Core>>, c: &mut Core, id: &str, env: &mut SurfaceEnv<'_>) -> Outcome {
    match get(c, id) {
        Ok(sticker) => {
            let window = panels::sticker_sheet(env.services, env.screen, &sticker);
            show_panel(rc, c, env.ctx, window, c.panel.is_some())
        }
        Err(e) => {
            env.toast(e, 4.0);
            Outcome::Handled
        }
    }
}

pub(super) fn command(rc: &Rc<RefCell<Core>>, cmd: &str, env: &mut SurfaceEnv<'_>) -> Option<Outcome> {
    if cmd != "reader:sel:sticker" && cmd != "reader:sel:share" && !cmd.starts_with("reader:sticker:") {
        return None;
    }
    let mut c = rc.borrow_mut();
    let result = run(rc, &mut c, cmd, env);
    Some(match result {
        Ok(outcome) => outcome,
        Err(e) => {
            env.notice(tr_format(&tr("Cannot save sticker or note. %1"), &[e.as_str()]));
            Outcome::Handled
        }
    })
}

fn run(rc: &Rc<RefCell<Core>>, c: &mut Core, cmd: &str, env: &mut SurfaceEnv<'_>) -> Result<Outcome, String> {
    if cmd == "reader:sel:share" {
        let selection = c.selection.clone().ok_or_else(|| tr("Select some text first."))?;
        c.session
            .as_mut()
            .ok_or_else(|| tr("No book is open."))?
            .add_highlight(
                &selection,
                kaia_book::Drawer::Lighten,
                "yellow",
                Some("Shared to Notes"),
            )
            .map_err(|e| e.to_string())?;
        c.clear_selection(env.ctx);
        env.toast(tr("Shared to Notes"), 2.0);
        return Ok(Outcome::Navigate(Nav::Pop));
    }
    if cmd == "reader:sel:sticker" {
        if let Some(id) = c.moving_sticker.clone() {
            let mut sticker = get(c, &id)?;
            let mut anchor = c
                .sticker_anchor
                .clone()
                .ok_or_else(|| tr("Select some text first."))?;
            if let (
                kaia_storage::Anchor::Reflowable { placement: old, .. },
                kaia_storage::Anchor::Reflowable {
                    placement, offset, ..
                },
            ) = (&sticker.anchor, &mut anchor)
            {
                *placement = *old;
                offset.dy = if *old == kaia_storage::Placement::Inline {
                    -1.1
                } else {
                    0.0
                };
            }
            sticker.anchor = anchor;
            sticker.state = StickerState::Attached;
            save(c, &sticker)?;
            c.moving_sticker = None;
            c.clear_selection(env.ctx);
            env.toast(tr("Sticker placed"), 2.0);
            return Ok(Outcome::Navigate(Nav::Pop));
        }
        c.editing_sticker = None;
        let window = panels::sticker_picker(env.services, env.screen);
        return Ok(show_panel(rc, c, env.ctx, window, true));
    }
    let rest = cmd.strip_prefix("reader:sticker:").unwrap_or("");
    if rest == "cancel-move" {
        c.moving_sticker = None;
        env.toast(tr("Move cancelled"), 2.0);
        return Ok(Outcome::Navigate(Nav::Pop));
    }
    if let Some(id) = rest.strip_prefix("open:") {
        return Ok(open(rc, c, id, env));
    }
    if let Some(id) = rest.strip_prefix("move:") {
        get(c, id)?;
        c.moving_sticker = Some(id.into());
        c.clear_selection(env.ctx);
        env.toast(tr("Long-press the new place, then choose Sticker."), 6.0);
        return Ok(Outcome::Navigate(Nav::Pop));
    }
    if let Some(id) = rest.strip_prefix("delete:") {
        get(c, id)?;
        stickers::store(c.session.as_ref().unwrap())?
            .delete_sticker(id)
            .map_err(|e| e.to_string())?;
        c.repaint_page(env.ctx, PaintHint::Ui);
        return Ok(Outcome::Navigate(Nav::Pop));
    }
    if let Some(id) = rest.strip_prefix("view:") {
        let sticker = get(c, id)?;
        let path = sticker
            .asset
            .as_ref()
            .map(|sha| {
                stickers::store(c.session.as_ref().unwrap())?
                    .asset_path(sha)
                    .map_err(|e| e.to_string())
            })
            .transpose()?;
        let window = panels::sticker_view(env.services, env.screen, &sticker, path.as_deref());
        return Ok(show_panel(rc, c, env.ctx, window, true));
    }
    if rest == "text" || rest.starts_with("edit:") {
        let text = if let Some(id) = rest.strip_prefix("edit:") {
            let s = get(c, id)?;
            if s.asset.is_some() {
                return Err(tr("Only text notes can be edited."));
            }
            c.editing_sticker = Some(id.into());
            s.text.unwrap_or_default()
        } else {
            c.editing_sticker = None;
            String::new()
        };
        let window = panels::sticker_note(env.services, env.screen, &text);
        return Ok(show_panel(rc, c, env.ctx, window, true));
    }
    let image = rest.strip_prefix("add:");
    if rest == "save" || image.is_some() {
        let text = if rest == "save" {
            let text = c
                .panel
                .as_ref()
                .and_then(|layer| read_field(&layer.window, env.ctx, "sticker-note-field"))
                .unwrap_or_default();
            if text.trim().is_empty() {
                return Err(tr("Write a note first."));
            }
            Some(text.trim().to_string())
        } else {
            None
        };
        if let Some(id) = c.editing_sticker.clone() {
            let mut sticker = get(c, &id)?;
            sticker.text = text;
            save(c, &sticker)?;
            c.editing_sticker = None;
        } else {
            let anchor = c
                .sticker_anchor
                .clone()
                .ok_or_else(|| tr("Select some text first."))?;
            let session = c.session.as_ref().ok_or_else(|| tr("No book is open."))?;
            let mut store = stickers::store(session)?;
            let asset = image
                .map(|name| {
                    let path =
                        stickers::bundled_path(env.services, name).ok_or_else(|| tr("Unknown sticker."))?;
                    store.add_asset_file(&path, "").map_err(|e| e.to_string())
                })
                .transpose()?;
            let size = if let Some(asset) = &asset {
                let ratio = if session.is_reflowable() {
                    c.viewport.w as f32 / c.viewport.h as f32
                } else {
                    let t = c
                        .session
                        .as_mut()
                        .unwrap()
                        .page_transform(c.viewport)
                        .map_err(|e| e.to_string())?;
                    t.page.width / t.page.height
                };
                StickerSize {
                    w: 0.07,
                    h: (0.07 * asset.height as f32 / asset.width as f32 * ratio).clamp(0.02, 1.0),
                }
            } else {
                StickerSize { w: 0.15, h: 0.1 }
            };
            let sticker = Sticker {
                id: None,
                doc: stickers::identity(c.session.as_ref().unwrap()),
                anchor,
                asset: asset.map(|a| a.sha256),
                text,
                size,
                rotation: 0,
                state: StickerState::Attached,
                created_at: 0,
                updated_at: 0,
            };
            store.put_sticker(&sticker).map_err(|e| e.to_string())?;
        }
        c.clear_selection(env.ctx);
        return Ok(Outcome::Navigate(Nav::Pop));
    }
    Ok(Outcome::Handled)
}

fn save(c: &Core, sticker: &Sticker) -> Result<(), String> {
    stickers::store(c.session.as_ref().ok_or_else(|| tr("No book is open."))?)?
        .put_sticker(sticker)
        .map_err(|e| e.to_string())?;
    Ok(())
}
