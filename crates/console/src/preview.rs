//! Aperçu du fichier sélectionné (volet Détails des Fichiers) : images et début des textes.

use eframe::egui;

/// Taille maximale d'une image prévisualisée.
const IMAGE_MAX: u64 = 25 * 1024 * 1024;
/// Début d'un texte lu pour l'aperçu.
const TEXT_MAX: u64 = 64 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreviewKind {
    Image,
    Text,
}

impl PreviewKind {
    /// Octets lus au plus.
    pub fn limit(self) -> u64 {
        match self {
            PreviewKind::Image => IMAGE_MAX,
            PreviewKind::Text => TEXT_MAX,
        }
    }
}

/// Contenu prêt à afficher.
pub enum Preview {
    Image(egui::ColorImage),
    /// Début du texte ; `true` s'il est coupé.
    Text(String, bool),
}

/// Le fichier peut-il être prévisualisé (d'après son extension et sa taille) ?
pub fn kind_of(name: &str, size: u64) -> Option<PreviewKind> {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" if size <= IMAGE_MAX => Some(PreviewKind::Image),
        "txt" | "md" | "markdown" | "log" | "csv" | "tsv" | "json" | "toml" | "yaml" | "yml" | "xml" | "ini" | "cfg" | "conf" | "env" | "rs" | "py" | "js" | "ts" | "c" | "cpp"
        | "h" | "hpp" | "cs" | "java" | "go" | "sh" | "ps1" | "bat" | "lua" | "sql" | "html" | "htm" | "css" | "php" | "gd" | "glsl" | "hlsl" | "shader" | "usf" | "ush"
        | "service" | "timer" => Some(PreviewKind::Text),
        _ => None,
    }
}

/// Décode ce qui a été lu (appelé hors de l'interface : le décodage d'une image prend du temps).
pub fn decode(bytes: &[u8], kind: PreviewKind) -> Result<Preview, String> {
    match kind {
        PreviewKind::Image => {
            let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
            // Réduite : le volet ne fait que quelques centaines de points de large.
            let img = if img.width() > 1024 || img.height() > 1024 { img.thumbnail(1024, 1024) } else { img };
            let rgba = img.to_rgba8();
            Ok(Preview::Image(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())))
        }
        PreviewKind::Text => {
            // Des octets nuls : ce n'est pas du texte.
            if bytes.contains(&0) {
                return Err(t!("fichier binaire", "binary file").into());
            }
            let cut = bytes.len() as u64 >= TEXT_MAX;
            Ok(Preview::Text(String::from_utf8_lossy(bytes).into_owned(), cut))
        }
    }
}

/// Vignette d'une image (vue en grandes icônes), réduite à 256 points au plus.
pub fn thumbnail(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?.thumbnail(256, 256).to_rgba8();
    Ok(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(kind_of("Photo.JPG", 10), Some(PreviewKind::Image));
        assert_eq!(kind_of("enorme.png", IMAGE_MAX + 1), None);
        assert_eq!(kind_of("notes.md", u64::MAX), Some(PreviewKind::Text));
        assert_eq!(kind_of("film.mp4", 10), None);
    }

    #[test]
    fn text() {
        assert!(matches!(decode(b"bonjour", PreviewKind::Text), Ok(Preview::Text(t, false)) if t == "bonjour"));
        assert!(decode(b"a\0b", PreviewKind::Text).is_err());
    }
}
