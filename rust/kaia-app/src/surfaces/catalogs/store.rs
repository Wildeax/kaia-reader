use kaia_events::{Settings, Value};
use kaia_opds::Catalog;
use std::path::PathBuf;

use crate::services::Services;

pub const CATALOGS: &str = "opds.catalogs";
pub const DOWNLOADS: &str = "network.downloads";

pub fn catalogs(settings: &Settings) -> Vec<Catalog> {
    let list = settings.read(CATALOGS);
    (1..=list.array_len() as i64)
        .filter_map(|i| {
            let item = list.get(i);
            Some(Catalog {
                title: item.get("title").as_str().unwrap_or("Catalog").into(),
                url: item.get("url").as_str()?.into(),
                username: item.get("username").as_str().map(str::to_owned),
                password: item.get("password").as_str().map(str::to_owned),
            })
        })
        .collect()
}

pub fn save(settings: &mut Settings, catalogs: &[Catalog]) {
    settings.save(
        CATALOGS,
        Value::array(catalogs.iter().map(|c| {
            Value::Table(
                [
                    ("title".into(), Value::from(c.title.clone())),
                    ("url".into(), Value::from(c.url.clone())),
                    (
                        "username".into(),
                        c.username.clone().map(Value::from).unwrap_or_default(),
                    ),
                    (
                        "password".into(),
                        c.password.clone().map(Value::from).unwrap_or_default(),
                    ),
                ]
                .into(),
            )
        })),
    );
}

pub fn downloads(services: &Services) -> PathBuf {
    services
        .settings
        .read(DOWNLOADS)
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| services.paths.books_dir.clone())
}
