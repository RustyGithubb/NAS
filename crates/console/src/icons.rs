//! Émojis en couleur (Twemoji, licence CC-BY 4.0 : assets/emoji/LICENSE-Twemoji.txt) pour les
//! fichiers et les emplacements. Chaque image n'est décodée qu'une fois, puis gardée en mémoire.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, Rect};

macro_rules! emojis {
    ($($name:ident = $file:literal),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Emoji { $($name),* }

        fn bytes(e: Emoji) -> &'static [u8] {
            match e { $(Emoji::$name => include_bytes!(concat!("../assets/emoji/", $file, ".png"))),* }
        }
    };
}

emojis! {
    Folder = "1f4c1",
    Image = "1f5bc",
    Video = "1f3ac",
    Audio = "1f3b5",
    Document = "1f4c4",
    Note = "1f4dd",
    Pdf = "1f4d5",
    Sheet = "1f4ca",
    Slides = "1f4fd",
    Archive = "1f4e6",
    Code = "1f4bb",
    Model3d = "1f9ca",
    Program = "2699",
    Font = "1f524",
    Database = "1f5c4",
    Web = "1f310",
    Disc = "1f4bf",
    Star = "2b50",
    Backup = "1f4be",
    Share = "1f5c2",
    Computer = "1f5a5",
    Plugin = "1f9e9",
    Game = "1f3ae",
}

type Cache = Arc<Mutex<HashMap<Emoji, egui::TextureHandle>>>;

fn texture(ctx: &egui::Context, e: Emoji) -> Option<egui::TextureHandle> {
    let cache: Cache = ctx.data_mut(|d| d.get_temp_mut_or_default::<Cache>(egui::Id::unique("nas_emoji_cache")).clone());
    let mut map = cache.lock().ok()?;
    if let Some(t) = map.get(&e) {
        return Some(t.clone());
    }
    let img = image::load_from_memory(bytes(e)).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    let tex = ctx.load_texture(format!("emoji-{e:?}"), color, egui::TextureOptions::LINEAR);
    map.insert(e, tex.clone());
    Some(tex)
}

/// Dessine l'émoji dans `r`.
pub fn paint(p: &egui::Painter, r: Rect, e: Emoji) {
    if let Some(t) = texture(p.ctx(), e) {
        p.image(t.id(), r, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
}

/// Émoji et type affiché d'un fichier, d'après son extension.
pub fn kind(name: &str, is_dir: bool) -> (Emoji, &'static str) {
    if is_dir {
        return (Emoji::Folder, t!("Dossier", "Folder"));
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "svg" | "heic" | "psd" | "kra" | "exr" | "tga" => (Emoji::Image, t!("Image", "Image")),
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "m4v" => (Emoji::Video, t!("Vidéo", "Video")),
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "opus" => (Emoji::Audio, t!("Audio", "Audio")),
        "pdf" => (Emoji::Pdf, t!("Document PDF", "PDF document")),
        "txt" | "md" | "rtf" | "log" => (Emoji::Note, t!("Texte", "Text")),
        "doc" | "docx" | "odt" | "pages" => (Emoji::Document, t!("Document", "Document")),
        "xls" | "xlsx" | "ods" | "csv" | "numbers" => (Emoji::Sheet, t!("Tableur", "Spreadsheet")),
        "ppt" | "pptx" | "odp" | "key" => (Emoji::Slides, t!("Présentation", "Presentation")),
        "zip" | "7z" | "rar" | "tar" | "gz" | "xz" | "bz2" | "zst" => (Emoji::Archive, t!("Archive", "Archive")),
        "rs" | "py" | "js" | "ts" | "c" | "cpp" | "h" | "hpp" | "cs" | "java" | "go" | "sh" | "ps1" | "bat" | "json" | "toml" | "yaml" | "yml" | "xml" | "ini" | "lua" => {
            (Emoji::Code, t!("Code", "Code"))
        }
        "html" | "htm" | "css" | "php" => (Emoji::Web, t!("Page web", "Web page")),
        "fbx" | "obj" | "blend" | "gltf" | "glb" | "stl" | "dae" | "max" | "ma" | "mb" | "usd" | "usdz" | "abc" => (Emoji::Model3d, t!("Modèle 3D", "3D model")),
        "uasset" | "umap" | "uproject" | "unity" | "prefab" | "godot" | "tscn" => (Emoji::Game, t!("Fichier de jeu", "Game file")),
        "exe" | "msi" | "dll" | "appimage" | "deb" | "app" => (Emoji::Program, t!("Programme", "Program")),
        "ttf" | "otf" | "woff" | "woff2" => (Emoji::Font, t!("Police", "Font")),
        "sql" | "db" | "sqlite" => (Emoji::Database, t!("Base de données", "Database")),
        "iso" | "img" | "dmg" => (Emoji::Disc, t!("Image disque", "Disk image")),
        "uplugin" | "vsix" | "xpi" => (Emoji::Plugin, t!("Extension", "Plugin")),
        _ => (Emoji::Document, t!("Fichier", "File")),
    }
}

/// Émoji d'un emplacement d'après son nom.
pub fn for_root(name: &str) -> Emoji {
    let n = name.to_lowercase();
    if n.contains("média") || n.contains("media") {
        Emoji::Image
    } else if n.contains("sauvegarde") || n.contains("backup") {
        Emoji::Backup
    } else if n.contains("partage") || n.contains("share") {
        Emoji::Share
    } else {
        Emoji::Computer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(kind("Photo.JPG", false).0, Emoji::Image);
        assert_eq!(kind("Desert.blend", false).0, Emoji::Model3d);
        assert_eq!(kind("sans-extension", false).0, Emoji::Document);
        assert_eq!(kind("dossier.png", true).0, Emoji::Folder);
        // Toutes les images se décodent.
        for e in [Emoji::Folder, Emoji::Image, Emoji::Program, Emoji::Game, Emoji::Backup] {
            assert!(image::load_from_memory(bytes(e)).is_ok());
        }
    }
}
