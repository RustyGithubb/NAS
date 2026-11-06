//! Surlignage « liquide » : la forme de sélection s'écoule d'un élément à l'autre.
//!
//! Le bord avant part en premier, le bord arrière suit avec un léger retard : la forme
//! s'étire pendant le trajet puis se resserre sur la cible, en s'amincissant au milieu
//! du mouvement. Repris de WorkTable (vitesse et courbe fixes ici).
//!
//! Utilisation :
//! ```ignore
//! let mut hl = Liquid::begin(ui, "mail_nav");      // avant les éléments
//! … pour l'élément sélectionné : hl.target(rect, clé);
//! hl.end(ui, couleur, rayon);                      // après les éléments
//! ```

use std::f32::consts::PI;

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Shape, layers::ShapeIdx, vec2};

/// Réglages de l'animation (fixes pour l'instant).
#[derive(Clone, Copy)]
struct Animation {
    duration_ms: f32,
    opacity: f32,
    curve: Curve,
}

const ANIM: Animation = Animation { duration_ms: 380.0, opacity: 1.0, curve: Curve::EaseInOut };

/// Courbe d'accélération.
#[derive(Clone, Copy)]
pub enum Curve {
    EaseInOut,
}

impl Curve {
    pub fn apply(self, t: f32) -> f32 {
        match self {
            Curve::EaseInOut => egui::emath::easing::cubic_in_out(t.clamp(0.0, 1.0)),
        }
    }
}

#[derive(Clone, Default)]
struct State {
    /// Rectangles relatifs à l'origine du conteneur (suivent le défilement).
    from: Option<Rect>,
    to: Option<Rect>,
    key: Option<u64>,
    start: f64,
}

pub struct Liquid {
    id: egui::Id,
    slot: ShapeIdx,
    origin: Pos2,
    /// Forme affichée cette frame (coordonnées écran) et son opacité.
    current: Option<(Rect, f32)>,
    target: Option<(Rect, u64)>,
}

impl Liquid {
    pub fn begin(ui: &egui::Ui, salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        let id = egui::Id::unique(("wt_liquid", salt));
        let origin = ui.min_rect().min;
        let slot = ui.painter().add(Shape::Noop);
        let state: State = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
        let anim = ANIM;
        let now = ui.input(|i| i.time);
        let current = frame_rect(&state, now, &anim).map(|(r, a)| (r.translate(origin.to_vec2()), a));
        Liquid { id, slot, origin, current, target: None }
    }

    /// Déclare l'élément sélectionné (`key` identifie l'élément ; un changement de clé déclenche l'animation).
    pub fn target(&mut self, rect: Rect, key: impl std::hash::Hash + std::fmt::Debug) {
        self.target = Some((rect, egui::Id::unique(key).value()));
    }

    /// Part de `rect` recouverte par la forme (0 à 1) : sert à inverser la couleur du contenu.
    pub fn coverage(&self, rect: Rect) -> f32 {
        let Some((cur, alpha)) = self.current else { return 0.0 };
        let i = cur.intersect(rect);
        if i.width() <= 0.0 || i.height() <= 0.0 {
            return 0.0;
        }
        (i.area() / rect.area()).clamp(0.0, 1.0) * alpha
    }

    pub fn end(self, ui: &egui::Ui, fill: Color32, radius: f32) {
        let ctx = ui.ctx();
        let anim = ANIM;
        let now = ui.input(|i| i.time);
        let mut state: State = ctx.data(|d| d.get_temp(self.id)).unwrap_or_default();
        let new_to = self.target.map(|(r, _)| r.translate(-self.origin.to_vec2()));
        let new_key = self.target.map(|(_, k)| k);

        if new_key != state.key {
            // Nouvelle sélection : on part de la forme actuellement affichée.
            state.from = frame_rect(&state, now, &anim).map(|(r, _)| r);
            state.to = new_to;
            state.key = new_key;
            state.start = now;
        } else {
            // Même élément : il a pu bouger (redimensionnement) → on suit sans animer.
            state.to = new_to;
        }

        let shown = frame_rect(&state, now, &anim);
        if let Some((r, alpha)) = shown {
            let r = r.translate(self.origin.to_vec2());
            let rr = radius.min(r.height() / 2.0).min(r.width() / 2.0).max(0.0) as u8;
            let color = fill.gamma_multiply((anim.opacity * alpha).clamp(0.0, 1.0));
            ui.painter().set(self.slot, Shape::rect_filled(r, CornerRadius::same(rr), color));
        }
        if progress(&state, now, &anim) < 1.0 {
            ctx.request_repaint();
        }
        ctx.data_mut(|d| d.insert_temp(self.id, state));
    }
}

fn progress(s: &State, now: f64, a: &Animation) -> f32 {
    let dur = (a.duration_ms / 1000.0).max(0.01) as f64;
    ((now - s.start) / dur).clamp(0.0, 1.0) as f32
}

/// Forme (relative) et opacité à l'instant `now`.
fn frame_rect(s: &State, now: f64, a: &Animation) -> Option<(Rect, f32)> {
    let t = progress(s, now, a);
    match (s.from, s.to) {
        (Some(from), Some(to)) => Some((flow(from, to, t, a.curve), 1.0)),
        // Apparition : la goutte gonfle depuis son centre.
        (None, Some(to)) => {
            let e = a.curve.apply(t).clamp(0.0, 1.2);
            Some((Rect::from_center_size(to.center(), to.size() * (0.55 + 0.45 * e)), t.min(1.0)))
        }
        // Disparition : elle se résorbe.
        (Some(from), None) => {
            if t >= 1.0 {
                return None;
            }
            let e = a.curve.apply(t);
            Some((Rect::from_center_size(from.center(), from.size() * (1.0 - 0.45 * e)), 1.0 - t))
        }
        (None, None) => None,
    }
}

/// Déplacement liquide de `from` vers `to`.
pub fn flow(from: Rect, to: Rect, t: f32, curve: Curve) -> Rect {
    if t >= 1.0 {
        return to;
    }
    // Bord avant en avance, bord arrière en retard.
    let lead = curve.apply((t / 0.8).min(1.0));
    let trail = curve.apply(((t - 0.2) / 0.8).max(0.0));
    let lerp = |a: f32, b: f32, k: f32| a + (b - a) * k;
    let (dx, dy) = (to.center().x - from.center().x, to.center().y - from.center().y);

    let (left, right) = if dx >= 0.0 {
        (lerp(from.left(), to.left(), trail), lerp(from.right(), to.right(), lead))
    } else {
        (lerp(from.left(), to.left(), lead), lerp(from.right(), to.right(), trail))
    };
    let (top, bottom) = if dy >= 0.0 {
        (lerp(from.top(), to.top(), trail), lerp(from.bottom(), to.bottom(), lead))
    } else {
        (lerp(from.top(), to.top(), lead), lerp(from.bottom(), to.bottom(), trail))
    };
    let mut r = Rect::from_min_max(Pos2::new(left.min(right), top.min(bottom)), Pos2::new(left.max(right), top.max(bottom)));

    // Amincissement perpendiculaire au mouvement, maximal à mi-parcours.
    let squeeze = 1.0 - 0.22 * (PI * t).sin();
    let c = r.center();
    if dy.abs() >= dx.abs() {
        r = Rect::from_center_size(c, vec2(r.width() * squeeze, r.height()));
    } else {
        r = Rect::from_center_size(c, vec2(r.width(), r.height() * squeeze));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_stretches_then_lands() {
        let a = Rect::from_min_size(Pos2::new(0.0, 0.0), vec2(40.0, 40.0));
        let b = Rect::from_min_size(Pos2::new(0.0, 200.0), vec2(40.0, 40.0));
        assert_eq!(flow(a, b, 0.0, Curve::EaseInOut), Rect::from_center_size(a.center(), a.size()));
        let mid = flow(a, b, 0.5, Curve::EaseInOut);
        assert!(mid.height() > 40.0, "étirée pendant le trajet : {mid:?}");
        assert!(mid.width() < 40.0, "amincie : {mid:?}");
        assert_eq!(flow(a, b, 1.0, Curve::EaseInOut), b);
    }
}
