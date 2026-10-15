//! Write-only WebDAV requests. Response bodies are never read.
use base64::Engine;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use url::Url;

use super::{Config, Failure};

pub(super) struct Client {
    folder: Url,
    collections: Vec<Url>,
    authorization: String,
    agent: ureq::Agent,
    allowed: Arc<AtomicBool>,
}
impl Client {
    pub fn new(config: &Config, allowed: Arc<AtomicBool>, test: bool) -> Result<Self, Failure> {
        let folder = config.folder_url()?;
        let count = config.folder.split('/').filter(|s| !s.is_empty()).count().max(1);
        let mut collections = Vec::new();
        for parents in (0..count).rev() {
            let mut collection = folder.clone();
            {
                let mut segments = collection.path_segments_mut().map_err(|_| Failure::Address)?;
                segments.pop_if_empty();
                for _ in 0..parents {
                    segments.pop();
                }
                segments.push("");
            }
            collections.push(collection);
        }
        let timeout = if test {
            kaia_sync::Timeouts::default().auth
        } else {
            kaia_sync::Timeouts::default().progress
        };
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .allow_non_standard_methods(true)
                .http_status_as_error(false)
                .max_redirects(0)
                .max_redirects_will_error(false)
                .timeout_global(Some(timeout.total))
                .timeout_connect(Some(timeout.block))
                .timeout_send_request(Some(timeout.block))
                .timeout_send_body(Some(timeout.block))
                .timeout_recv_response(Some(timeout.block))
                .timeout_recv_body(Some(timeout.block))
                .build(),
        );
        if config.username.is_empty() || config.username.contains(':') || config.password.is_empty() {
            return Err(Failure::Credentials);
        }
        let authorization = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", config.username, config.password))
        );
        Ok(Self {
            folder,
            collections,
            authorization,
            agent,
            allowed,
        })
    }
    fn request(&self, method: &str, url: &Url, content: &str, mime: &str) -> Result<u16, Failure> {
        if !self.allowed.load(Ordering::SeqCst) {
            return Err(Failure::Held);
        }
        let request = ureq::http::Request::builder()
            .method(method)
            .uri(url.as_str())
            .header("Authorization", &self.authorization)
            .header("Depth", "0")
            .header("Content-Type", mime)
            .body(content)
            .map_err(|_| Failure::Address)?;
        let reply = self.agent.run(request).map_err(|_| Failure::Network)?;
        let status = reply.status().as_u16();
        if status == 401 {
            return Err(Failure::Unauthorized);
        }
        Ok(status)
    }
    pub fn test(&self) -> Result<(), Failure> {
        let status = self.request("PROPFIND", &self.folder, "", "application/xml")?;
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(Failure::Status(status))
        }
    }
    pub fn upload(&self, name: &str, markdown: &str, json: &str) -> Result<(), Failure> {
        // MKCOL returns 405 for an existing collection. Never fetch its contents.
        for collection in &self.collections {
            let status = self.request("MKCOL", collection, "", "application/xml")?;
            if status != 405 && !(200..300).contains(&status) {
                return Err(Failure::Status(status));
            }
        }
        for (extension, text, mime) in [
            ("md", markdown, "text/markdown; charset=utf-8"),
            ("json", json, "application/json"),
        ] {
            let mut url = self.folder.clone();
            url.path_segments_mut()
                .map_err(|_| Failure::Address)?
                .pop_if_empty()
                .push(&format!("{name}.{extension}"));
            let status = self.request("PUT", &url, text, mime)?;
            if !(200..300).contains(&status) {
                return Err(Failure::Status(status));
            }
        }
        Ok(())
    }
}
