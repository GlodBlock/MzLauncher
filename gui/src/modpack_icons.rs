use std::collections::HashMap;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use eframe::egui;

struct IconEntry {
    source_url: String,
    loading: bool,
    failed: bool,
    image: Option<egui::ColorImage>,
    texture: Option<egui::TextureHandle>,
}

#[derive(Default)]
pub struct ModpackIconCache {
    entries: HashMap<String, IconEntry>,
}

impl ModpackIconCache {
    pub fn needs_request(&self, key: &str, source_url: &str) -> bool {
        let Some(entry) = self.entries.get(key) else {
            return true;
        };
        if entry.loading || entry.failed {
            return false;
        }
        entry.source_url != source_url || (entry.image.is_none() && entry.texture.is_none())
    }

    pub fn reset_pending(&mut self) {
        for entry in self.entries.values_mut() {
            if entry.loading {
                entry.loading = false;
            }
        }
    }

    pub fn begin_request(&mut self, key: &str, source_url: &str) {
        self.entries.insert(
            key.to_owned(),
            IconEntry {
                source_url: source_url.to_owned(),
                loading: true,
                failed: false,
                image: None,
                texture: None,
            },
        );
    }

    pub fn complete(&mut self, key: &str, data_base64: &str) -> Result<(), String> {
        let bytes = STANDARD
            .decode(data_base64)
            .map_err(|error| error.to_string())?;
        let image = image::load_from_memory(&bytes)
            .map_err(|error| error.to_string())?
            .to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        let entry = self
            .entries
            .get_mut(key)
            .ok_or_else(|| "icon request expired".to_owned())?;
        entry.loading = false;
        entry.failed = false;
        entry.image = Some(image);
        entry.texture = None;
        Ok(())
    }

    pub fn fail(&mut self, key: &str) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.loading = false;
            entry.failed = true;
        }
    }

    pub fn texture(&mut self, ctx: &egui::Context, key: &str) -> Option<egui::TextureHandle> {
        let entry = self.entries.get_mut(key)?;
        if entry.texture.is_none() {
            let image = entry.image.take()?;
            entry.texture = Some(ctx.load_texture(
                format!("modpack-icon-{key}"),
                image,
                egui::TextureOptions::LINEAR,
            ));
        }
        entry.texture.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_gif_icons() {
        let encoded = "R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";
        let mut cache = ModpackIconCache::default();
        cache.begin_request("gif", "https://example.com/icon.gif");
        cache.complete("gif", encoded).unwrap();
        assert_eq!(cache.entries["gif"].image.as_ref().unwrap().size, [1, 1]);
    }

    #[test]
    fn decodes_icons_and_tracks_source_urls() {
        let encoded = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
        let url = "https://example.com/icon.png";
        let mut cache = ModpackIconCache::default();
        assert!(cache.needs_request("project", url));
        cache.begin_request("project", url);
        assert!(!cache.needs_request("project", url));
        cache.complete("project", encoded).unwrap();
        assert!(!cache.needs_request("project", url));
        assert!(cache.needs_request("project", "https://example.com/other.png"));
        assert_eq!(
            cache.entries["project"].image.as_ref().unwrap().size,
            [1, 1]
        );
    }
}
