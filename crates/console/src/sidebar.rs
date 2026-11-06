//! Barre de navigation verticale en forme de pilule, flottante à gauche (reprise de WorkTable) :
//! logo, onglets principaux, et pilule Réglages ancrée en bas.

use eframe::egui::{self, Color32, CornerRadius, Margin, Order, Painter, Pos2, Rect, Sense, Shadow, Stroke, vec2};

use crate::app::Tab;
use crate::liquid::Liquid;
use crate::theme;

const PILL_WIDTH: f32 = 56.0;
const BUTTON: f32 = 40.0;
const SCREEN_MARGIN: f32 = 16.0;

/// Onglets de la pilule du bas.
const BOTTOM: [Tab; 1] = [Tab::Settings];
/// Hauteur de la pilule du bas (boutons + espacement + marges).
const BOTTOM_H: f32 = BOTTOM.len() as f32 * BUTTON + (BOTTOM.len() as f32 - 1.0) * 8.0 + 16.0;

/// Affiche la barre principale et renvoie l'onglet cliqué.
pub fn show(ctx: &egui::Context, window: Rect, current: Tab) -> Option<Tab> {
    // Hauteur : logo + séparateur + boutons.
    let n = Tab::ALL.len() as f32;
    let inner_h = BUTTON + 17.0 + n * BUTTON + (n - 1.0) * 8.0;
    let pill_h = inner_h + 16.0;
    // Centrée, sans jamais chevaucher la pilule du bas.
    let max_y = window.bottom() - 2.0 * SCREEN_MARGIN - BOTTOM_H - pill_h;
    let y = (window.center().y - pill_h / 2.0).min(max_y).max(window.top() + SCREEN_MARGIN);
    let pos = Pos2::new(window.left() + SCREEN_MARGIN, y);

    let mut clicked = None;
    egui::Area::new(egui::Id::unique("nas_sidebar")).order(Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        pill_frame().show(ui, |ui| {
            ui.set_width(BUTTON);
            ui.spacing_mut().item_spacing.y = 8.0;

            // Logo
            let (logo, _) = ui.allocate_exact_size(vec2(BUTTON, BUTTON), Sense::hover());
            paint_logo(ui.painter(), Rect::from_center_size(logo.center(), vec2(30.0, 30.0)), theme::TEXT());

            // Séparateur
            let (sep, _) = ui.allocate_exact_size(vec2(BUTTON, 1.0), Sense::hover());
            ui.painter().hline(sep.x_range().shrink(8.0), sep.center().y, Stroke::new(1.0, theme::BORDER()));

            // La pastille de sélection s'écoule d'un onglet à l'autre.
            let mut hl = Liquid::begin(ui, "sidebar_nav");
            for tab in Tab::ALL {
                if nav_button(ui, tab, tab == current, &mut hl).clicked() {
                    clicked = Some(tab);
                }
            }
            hl.end(ui, theme::TEXT(), BUTTON / 2.0);
        });
    });
    clicked
}

/// Pilule Réglages, ancrée en bas à gauche. Renvoie l'onglet cliqué.
pub fn bottom_buttons(ctx: &egui::Context, window: Rect, current: Tab) -> Option<Tab> {
    let pos = Pos2::new(window.left() + SCREEN_MARGIN, window.bottom() - SCREEN_MARGIN - BOTTOM_H);
    let mut clicked = None;
    egui::Area::new(egui::Id::unique("nas_settings")).order(Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        pill_frame().show(ui, |ui| {
            ui.set_width(BUTTON);
            ui.spacing_mut().item_spacing.y = 8.0;
            let mut hl = Liquid::begin(ui, "sidebar_settings");
            for tab in BOTTOM {
                if nav_button(ui, tab, tab == current, &mut hl).clicked() {
                    clicked = Some(tab);
                }
            }
            hl.end(ui, theme::TEXT(), BUTTON / 2.0);
        });
    });
    clicked
}

/// Logo de l'app, dessiné en vectoriel : équerres aux coins et cadre arrondi (comme WorkTable),
/// trois baies de disque à l'intérieur.
pub fn paint_logo(p: &Painter, r: Rect, color: Color32) {
    let k = r.width() / 100.0;
    let at = |x: f32, y: f32| r.min + vec2(x * k, y * k);
    let w = 5.0 * k;
    let stroke = Stroke::new(w, color);
    for (x0, y0, dx, dy) in [(8.0, 8.0, 1.0, 1.0), (92.0, 8.0, -1.0, 1.0), (8.0, 92.0, 1.0, -1.0), (92.0, 92.0, -1.0, -1.0)] {
        let pts = [at(x0, y0 + 14.0 * dy), at(x0, y0), at(x0 + 14.0 * dx, y0)];
        p.line_segment([pts[0], pts[1]], stroke);
        p.line_segment([pts[1], pts[2]], stroke);
        for q in pts {
            p.circle_filled(q, w / 2.0, color);
        }
    }
    p.rect_stroke(Rect::from_min_max(at(18.0, 18.0), at(82.0, 82.0)), CornerRadius::same((10.0 * k) as u8), stroke, egui::StrokeKind::Middle);
    // Baies : barre pleine + voyant creux à droite.
    let h = (40.0 - 2.0 * 5.0) / 3.0;
    for i in 0..3 {
        let y = 30.0 + i as f32 * (h + 5.0);
        p.rect_filled(Rect::from_min_max(at(30.0, y), at(58.0, y + h)), CornerRadius::same((2.5 * k).round() as u8), color);
        p.circle_filled(at(66.0, y + h / 2.0), h * k * 0.42, color);
    }
}

fn nav_button(ui: &mut egui::Ui, tab: Tab, active: bool, hl: &mut Liquid) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(BUTTON, BUTTON), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tab.label());
    if active {
        hl.target(rect, tab.key());
    }
    let phase = click_phase(ui, tab.key(), resp.clicked());
    let zoom = magnify(ui, tab.key(), rect);
    paint_button(ui, rect, &resp, hl.coverage(rect), zoom, phase, |p, r, c, a| draw_icon(p, r, tab, c, a));
    resp
}

// --- Effets : animation au clic, loupe façon Dock macOS --------------------------

/// Durée de l'animation de clic d'une icône.
const CLICK_ANIM: f64 = 0.43;
/// Durée de la transition de survol (loupe).
const HOVER_ANIM: f32 = 0.08;

/// Avancement (0 → 1) de l'animation de clic de l'icône `key` ; 1 = au repos.
fn click_phase(ui: &egui::Ui, key: &str, clicked: bool) -> f32 {
    let id = egui::Id::unique(("nas_icon_click", key));
    let now = ui.input(|i| i.time);
    if clicked {
        ui.ctx().data_mut(|d| d.insert_temp(id, now));
    }
    let Some(start) = ui.ctx().data(|d| d.get_temp::<f64>(id)) else { return 1.0 };
    let a = ((now - start) / CLICK_ANIM) as f32;
    if a < 1.0 {
        ui.ctx().request_repaint();
        a.max(0.0)
    } else {
        1.0
    }
}

/// Loupe façon Dock macOS, limitée à l'icône survolée.
fn magnify(ui: &egui::Ui, key: &str, rect: Rect) -> f32 {
    const MAX: f32 = 0.42;
    let hovered = ui.ctx().pointer_hover_pos().is_some_and(|p| rect.contains(p));
    let target = if hovered { MAX } else { 0.0 };
    1.0 + ui.ctx().animate_value_with_time(egui::Id::unique(("nas_icon_zoom", key)), target, HOVER_ANIM)
}

fn pill_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::SIDEBAR_BG())
        .stroke(Stroke::new(1.0, theme::BORDER()))
        .corner_radius(CornerRadius::same((PILL_WIDTH / 2.0) as u8))
        .inner_margin(Margin::same(8))
        .shadow(Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(140) })
}

fn paint_button(ui: &egui::Ui, rect: Rect, resp: &egui::Response, coverage: f32, zoom: f32, phase: f32, icon: impl FnOnce(&Painter, Rect, Color32, f32)) {
    // `coverage` : part du bouton recouverte par la pastille liquide (l'icône s'inverse).
    let base = if resp.hovered() { theme::TEXT() } else { theme::TEXT_MUTED() };
    let fg = theme::blend(base, theme::BG(), coverage.clamp(0.0, 1.0));
    let painter = ui.painter();
    if resp.hovered() && coverage < 0.5 {
        painter.rect_filled(rect, CornerRadius::same((BUTTON / 2.0) as u8), theme::SURFACE_HI());
    }
    // Petit « enfoncement » au clic, puis loupe : l'icône grossit sur place.
    let press = 1.0 - 0.09 * (std::f32::consts::PI * phase.min(1.0)).sin().max(0.0) * (1.0 - phase).max(0.0).sqrt();
    let size = 18.0 * zoom * press;
    icon(painter, Rect::from_center_size(rect.center(), vec2(size, size)), fg, phase);
}

/// Lissage « ease in-out » pour les animations d'icônes.
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Enveloppe aller-retour : 0 → 1 → 0.
fn bump(t: f32) -> f32 {
    (std::f32::consts::PI * t.clamp(0.0, 1.0)).sin()
}

/// Engrenage : au clic, il tourne d'un huitième de tour.
fn draw_gear(p: &Painter, r: Rect, c: Color32, phase: f32) {
    let center = r.center();
    let rad = r.width() / 2.0;
    let spin = ease(phase) * std::f32::consts::FRAC_PI_8;
    p.circle_stroke(center, rad * 0.62, Stroke::new(1.6, c));
    p.circle_stroke(center, rad * 0.24, Stroke::new(1.6, c));
    for i in 0..8 {
        let a = spin + i as f32 * std::f32::consts::TAU / 8.0;
        let d = vec2(a.cos(), a.sin());
        p.line_segment([center + d * rad * 0.62, center + d * rad], Stroke::new(2.6, c));
    }
}

/// Icônes vectorielles ; `phase` (0 → 1) anime l'icône juste après un clic :
/// tableau de bord = les tuiles s'écartent, services = les voyants s'allument tour à tour,
/// journaux = le curseur clignote, fichiers = le rabat s'entrouvre, réglages = l'engrenage tourne.
pub fn draw_icon(p: &Painter, r: Rect, tab: Tab, c: Color32, phase: f32) {
    let w = (r.width() / 18.0).max(0.5);
    let s = Stroke::new(1.6 * w.sqrt(), c);
    let k = CornerRadius::same(2);
    match tab {
        Tab::Settings => draw_gear(p, r, c, phase),
        Tab::Dashboard => {
            let g = 3.0 * w;
            let side = (r.width() - g) / 2.0;
            for (n, (i, j)) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].into_iter().enumerate() {
                let spread = bump((phase - n as f32 * 0.08) / 0.75) * 1.6 * w;
                let dir = vec2(i * 2.0 - 1.0, j * 2.0 - 1.0);
                let min = r.min + vec2(i * (side + g), j * (side + g)) + dir * spread;
                p.rect_stroke(Rect::from_min_size(min, vec2(side, side)), k, s, egui::StrokeKind::Inside);
            }
        }
        Tab::Services => {
            // Baie de serveur : trois tiroirs, un voyant chacun.
            let g = 2.0 * w;
            let h = (r.height() - 2.0 * g) / 3.0;
            for i in 0..3 {
                let row = Rect::from_min_size(r.min + vec2(0.0, i as f32 * (h + g)), vec2(r.width(), h));
                p.rect_stroke(row, k, s, egui::StrokeKind::Inside);
                let dot = Pos2::new(row.left() + 3.2 * w, row.center().y);
                let lit = bump((phase - i as f32 * 0.18) / 0.5);
                p.circle_filled(dot, 1.1 * w * (1.0 + 0.5 * lit), c);
                p.line_segment(
                    [Pos2::new(row.left() + 6.5 * w, row.center().y), Pos2::new(row.right() - 3.0 * w, row.center().y)],
                    Stroke::new(1.2 * w.sqrt(), c.gamma_multiply(0.7)),
                );
            }
        }
        Tab::Logs => {
            // Terminal : cadre, invite « > » et curseur « _ ».
            let frame = r.shrink(0.5 * w);
            p.rect_stroke(frame, CornerRadius::same((4.0 * w) as u8), s, egui::StrokeKind::Inside);
            let a = frame.left_top() + vec2(4.0 * w, 5.0 * w);
            let thick = Stroke::new(2.0 * w.sqrt(), c);
            p.add(egui::Shape::line(vec![a, a + vec2(3.2 * w, 3.0 * w), a + vec2(0.0, 6.0 * w)], thick));
            // Curseur : il s'efface puis réapparaît au clic.
            let blink = 1.0 - bump(phase * 2.0 % 1.0) * if phase < 1.0 { 1.0 } else { 0.0 };
            let y = a.y + 6.0 * w;
            p.line_segment([Pos2::new(a.x + 5.5 * w, y), Pos2::new(a.x + 10.0 * w, y)], Stroke::new(2.0 * w.sqrt(), c.gamma_multiply(blink)));
        }
        Tab::Files => {
            // Dossier ; au clic, le rabat avant s'entrouvre.
            let tab_w = r.width() * 0.42;
            let back = Rect::from_min_max(r.min + vec2(0.0, 3.0 * w), r.max - vec2(0.0, 1.0 * w));
            let tab_top = back.top() - 2.5 * w;
            p.add(egui::Shape::line(
                vec![
                    Pos2::new(back.left(), back.top() + 2.0 * w),
                    Pos2::new(back.left(), tab_top),
                    Pos2::new(back.left() + tab_w - 2.0 * w, tab_top),
                    Pos2::new(back.left() + tab_w, back.top()),
                    Pos2::new(back.right(), back.top()),
                    Pos2::new(back.right(), back.top() + 3.0 * w),
                ],
                s,
            ));
            let lift = bump(phase) * 1.6 * w;
            let front = Rect::from_min_max(Pos2::new(back.left(), back.top() + 3.5 * w - lift), back.max);
            p.rect_stroke(front, k, s, egui::StrokeKind::Inside);
        }
    }
}
