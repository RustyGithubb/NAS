//! Overlay façon recherche Ctrl+Espace de WorkTable : fond flouté et cadre de la boîte.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use eframe::egui::{self, Color32, ColorImage, CornerRadius, Pos2, Rect, Sense, Stroke};

/// Durée du fondu, à l'ouverture comme à la fermeture.
const FADE: f32 = 0.14;

/// Fond d'un overlay : capture floutée de l'app, voile sombre, apparition en fondu et
/// disparition par le même fondu inversé ; un clic dessus ferme l'overlay.
#[derive(Default)]
pub struct Backdrop {
    shot: Arc<Mutex<Option<Arc<ColorImage>>>>,
    texture: Option<egui::TextureHandle>,
    opened_at: Option<Instant>,
    /// Fermeture en cours : début et avancement du fondu à ce moment (fermé en pleine ouverture).
    closing: Option<(Instant, f32)>,
}

impl Backdrop {
    /// Ouverture : capture de l'image affichée (l'overlay est encore transparent).
    pub fn start(&mut self, ctx: &egui::Context) {
        // Rouvert pendant la disparition : le fondu repart de là où il en est, avec la même capture
        // (une nouvelle contiendrait le panneau à moitié effacé).
        if self.closing.is_some() {
            let k = self.progress();
            self.closing = None;
            self.opened_at = Instant::now().checked_sub(std::time::Duration::from_secs_f32(k * FADE));
            ctx.request_repaint();
            return;
        }
        self.texture = None;
        self.closing = None;
        // Capture d'une ouverture précédente arrivée trop tard : écartée.
        if let Ok(mut s) = self.shot.lock() {
            *s = None;
        }
        self.opened_at = Some(Instant::now());
        let slot = self.shot.clone();
        let c = ctx.clone();
        ctx.request_screenshot(move |img| {
            let blurred = blur(&img, 4, 3);
            if let Ok(mut s) = slot.lock() {
                *s = Some(Arc::new(blurred));
            }
            c.request_repaint();
        });
    }

    pub fn clear(&mut self) {
        self.texture = None;
        self.closing = None;
    }

    /// Avancement du fondu : 0 = invisible, 1 = ouvert.
    fn progress(&self) -> f32 {
        let opening = self.opened_at.map(|t| (t.elapsed().as_secs_f32() / FADE).min(1.0)).unwrap_or(1.0);
        match self.closing {
            Some((t, from)) => (from - t.elapsed().as_secs_f32() / FADE).max(0.0),
            None => opening,
        }
    }

    /// Lance la disparition : le même fondu qu'à l'ouverture, à l'envers.
    pub fn close(&mut self, ctx: &egui::Context) {
        if self.closing.is_none() {
            self.closing = Some((Instant::now(), self.progress()));
        }
        // Sans nouvelle image demandée, la disparition attendrait le prochain mouvement de souris.
        ctx.request_repaint();
    }

    pub fn is_closing(&self) -> bool {
        self.closing.is_some()
    }

    /// Disparition terminée : l'overlay peut être retiré.
    pub fn is_closed(&self) -> bool {
        self.closing.is_some() && self.progress() <= 0.0
    }

    /// Dessine le fond ; renvoie (avancement du fondu 0..1, clic sur le fond).
    pub fn show(&mut self, ctx: &egui::Context, id: &str) -> (f32, bool) {
        self.show_in(ctx, id, egui::Order::Tooltip)
    }

    /// Même fond, à un autre niveau (Réglages : leurs listes déroulantes passent devant).
    pub fn show_in(&mut self, ctx: &egui::Context, id: &str, order: egui::Order) -> (f32, bool) {
        let screen = ctx.content_rect();
        if let Some(img) = self.shot.lock().ok().and_then(|mut s| s.take()) {
            self.texture = Some(ctx.load_texture(format!("{id}_tex"), img, egui::TextureOptions::LINEAR));
        }
        let k = self.progress();
        if k < 1.0 {
            ctx.request_repaint();
        }
        let mut clicked = false;
        // Pendant la disparition, le fond ne retient plus les clics : l'app redevient utilisable.
        egui::Area::new(egui::Id::unique(id)).order(order).fixed_pos(screen.min).interactable(!self.is_closing()).show(ctx, |ui| {
            let (r, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            let p = ui.painter();
            if let Some(t) = &self.texture {
                let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                p.image(t.id(), r, uv, crate::theme::overlay((255.0 * k) as u8));
            }
            p.rect_filled(r, CornerRadius::ZERO, Color32::from_black_alpha((80.0 * k) as u8));
            clicked = resp.clicked();
        });
        (k, clicked)
    }
}

/// Cadre de la boîte d'un overlay.
pub fn overlay_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgba_premultiplied(0x1C, 0x1C, 0x1C, 0xF4))
        .stroke(Stroke::new(1.0, crate::theme::overlay(28)))
        .corner_radius(CornerRadius::same(16))
        .shadow(egui::Shadow { offset: [0, 18], blur: 48, spread: 0, color: Color32::from_black_alpha(170) })
        .inner_margin(egui::Margin::same(14))
}

/// Flou approchant un flou gaussien : réduction d'un facteur `down`, puis `passes` flous boîte.
fn blur(img: &ColorImage, down: usize, passes: usize) -> ColorImage {
    let (w, h) = (img.size[0], img.size[1]);
    let (sw, sh) = ((w / down).max(1), (h / down).max(1));
    let mut px: Vec<[f32; 3]> = vec![[0.0; 3]; sw * sh];
    for y in 0..sh {
        for x in 0..sw {
            let mut acc = [0.0f32; 3];
            let mut n = 0.0;
            for dy in 0..down {
                for dx in 0..down {
                    let (ix, iy) = (x * down + dx, y * down + dy);
                    if ix < w && iy < h {
                        let c = img.pixels[iy * w + ix];
                        acc[0] += c.r() as f32;
                        acc[1] += c.g() as f32;
                        acc[2] += c.b() as f32;
                        n += 1.0;
                    }
                }
            }
            px[y * sw + x] = [acc[0] / n, acc[1] / n, acc[2] / n];
        }
    }
    let r = 3i64;
    for _ in 0..passes {
        let mut tmp = px.clone();
        for y in 0..sh {
            for x in 0..sw {
                let mut acc = [0.0f32; 3];
                let mut n = 0.0;
                for k in -r..=r {
                    let xx = (x as i64 + k).clamp(0, sw as i64 - 1) as usize;
                    let c = px[y * sw + xx];
                    acc[0] += c[0];
                    acc[1] += c[1];
                    acc[2] += c[2];
                    n += 1.0;
                }
                tmp[y * sw + x] = [acc[0] / n, acc[1] / n, acc[2] / n];
            }
        }
        for y in 0..sh {
            for x in 0..sw {
                let mut acc = [0.0f32; 3];
                let mut n = 0.0;
                for k in -r..=r {
                    let yy = (y as i64 + k).clamp(0, sh as i64 - 1) as usize;
                    let c = tmp[yy * sw + x];
                    acc[0] += c[0];
                    acc[1] += c[1];
                    acc[2] += c[2];
                    n += 1.0;
                }
                px[y * sw + x] = [acc[0] / n, acc[1] / n, acc[2] / n];
            }
        }
    }
    let rgba: Vec<u8> = px.iter().flat_map(|c| [c[0] as u8, c[1] as u8, c[2] as u8, 255]).collect();
    ColorImage::from_rgba_unmultiplied([sw, sh], &rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_keeps_size_ratio() {
        let img = ColorImage::new([40, 20], vec![Color32::WHITE; 800]);
        let b = blur(&img, 4, 2);
        assert_eq!(b.size, [10, 5]);
    }
}
