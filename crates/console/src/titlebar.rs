//! Barre de titre intégrée à l'app, reprise de WorkTable (la fenêtre n'a pas de cadre Windows) : bande invisible en
//! haut pour déplacer la fenêtre (double-clic : agrandir / restaurer), boutons réduire,
//! agrandir et fermer en haut à droite, bords pour redimensionner.

use eframe::egui::viewport::ResizeDirection;
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, ViewportCommand, vec2};

use crate::theme;

/// Hauteur de la bande de déplacement.
pub const HEIGHT: f32 = 32.0;
const BUTTON_W: f32 = 46.0;
/// Épaisseur des bords de redimensionnement.
const EDGE: f32 = 5.0;

#[derive(Clone, Copy)]
enum Button {
    Minimize,
    Maximize,
    Close,
}

pub fn show(ctx: &egui::Context, window: Rect) {
    show_inner(ctx, window, None);
}

/// Barre de titre d'une fenêtre volante : titre à gauche et bouton « toujours au premier plan »
/// (épingle) avant réduire / agrandir / fermer.
pub fn show_float(ctx: &egui::Context, window: Rect, title: &str, on_top: &mut bool) {
    show_inner(ctx, window, Some((title, on_top)));
}

fn show_inner(ctx: &egui::Context, window: Rect, mut float: Option<(&str, &mut bool)>) {
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    egui::Area::new(egui::Id::unique("wt_titlebar")).order(egui::Order::Foreground).fixed_pos(window.min).show(ctx, |ui| {
        let bar = Rect::from_min_size(window.min, vec2(window.width(), HEIGHT));
        let extra = if float.is_some() { BUTTON_W } else { 0.0 };
        let buttons = Rect::from_min_max(Pos2::new(bar.right() - 3.0 * BUTTON_W, bar.top()), bar.max);
        // Déplacement : toute la bande sauf les boutons.
        let drag = Rect::from_min_max(bar.min, Pos2::new(buttons.left() - extra, bar.bottom()));
        if let Some((title, on_top)) = float.as_mut() {
            ui.painter().text(Pos2::new(bar.left() + 14.0, bar.center().y), egui::Align2::LEFT_CENTER, *title, egui::FontId::proportional(12.5), theme::TEXT_MUTED());
            // Épingle : garde la fenêtre au-dessus des autres (allumée quand c'est le cas).
            let r = Rect::from_min_size(Pos2::new(buttons.left() - BUTTON_W, bar.top()), vec2(BUTTON_W, HEIGHT));
            let tip = if **on_top { t!("Ne plus garder au premier plan", "Stop keeping on top") } else { t!("Toujours au premier plan", "Always on top") };
            let resp = ui.interact(r, egui::Id::unique("wt_titlebar_pin"), Sense::click()).on_hover_text(tip);
            let p = ui.painter();
            if **on_top {
                p.rect_filled(r, CornerRadius::ZERO, theme::overlay(16));
            } else if resp.hovered() {
                p.rect_filled(r, CornerRadius::ZERO, theme::overlay(22));
            }
            pin_icon(p, r.center(), if **on_top { theme::TEXT() } else { theme::TEXT_MUTED() }, **on_top);
            if resp.clicked() {
                **on_top = !**on_top;
            }
        }
        let resp = ui.interact(drag, egui::Id::unique("wt_titlebar_drag"), Sense::click_and_drag());
        if resp.double_clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        } else if resp.drag_started_by(egui::PointerButton::Primary) {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        for (i, b) in [Button::Minimize, Button::Maximize, Button::Close].into_iter().enumerate() {
            let r = Rect::from_min_size(Pos2::new(buttons.left() + i as f32 * BUTTON_W, bar.top()), vec2(BUTTON_W, HEIGHT));
            let resp = ui.interact(r, egui::Id::unique(("wt_titlebar_btn", i)), Sense::click());
            let tip = match b {
                Button::Minimize => t!("Réduire", "Minimise"),
                Button::Maximize if maximized => t!("Restaurer", "Restore"),
                Button::Maximize => t!("Agrandir", "Maximise"),
                Button::Close => t!("Fermer", "Close"),
            };
            let resp = resp.on_hover_text(tip);
            let p = ui.painter();
            let mut ink = theme::TEXT_MUTED();
            if resp.hovered() {
                let fill = if matches!(b, Button::Close) { Color32::from_rgb(196, 43, 28) } else { theme::overlay(22) };
                p.rect_filled(r, CornerRadius::ZERO, fill);
                ink = if matches!(b, Button::Close) { Color32::WHITE } else { theme::TEXT() };
            }
            let c = r.center();
            let s = Stroke::new(1.0, ink);
            match b {
                Button::Minimize => {
                    p.line_segment([c + vec2(-5.0, 0.5), c + vec2(5.0, 0.5)], s);
                }
                Button::Maximize if maximized => {
                    p.rect_stroke(Rect::from_center_size(c + vec2(-1.0, 1.0), vec2(8.0, 8.0)), CornerRadius::same(1), s, egui::StrokeKind::Middle);
                    p.line_segment([c + vec2(-2.0, -4.0), c + vec2(5.0, -4.0)], s);
                    p.line_segment([c + vec2(5.0, -4.0), c + vec2(5.0, 3.0)], s);
                }
                Button::Maximize => {
                    p.rect_stroke(Rect::from_center_size(c, vec2(10.0, 10.0)), CornerRadius::same(1), s, egui::StrokeKind::Middle);
                }
                Button::Close => {
                    p.line_segment([c + vec2(-5.0, -5.0), c + vec2(5.0, 5.0)], s);
                    p.line_segment([c + vec2(-5.0, 5.0), c + vec2(5.0, -5.0)], s);
                }
            }
            if resp.clicked() {
                ctx.send_viewport_cmd(match b {
                    Button::Minimize => ViewportCommand::Minimized(true),
                    Button::Maximize => ViewportCommand::Maximized(!maximized),
                    Button::Close => ViewportCommand::Close,
                });
            }
        }

        // Redimensionnement par les bords et les coins (fenêtre non agrandie).
        if !maximized {
            let w = window;
            let edges = [
                (Rect::from_min_max(w.min, Pos2::new(w.min.x + EDGE * 2.0, w.min.y + EDGE * 2.0)), ResizeDirection::NorthWest, egui::CursorIcon::ResizeNorthWest),
                (
                    Rect::from_min_max(Pos2::new(w.max.x - EDGE * 2.0, w.min.y), Pos2::new(w.max.x, w.min.y + EDGE * 2.0)),
                    ResizeDirection::NorthEast,
                    egui::CursorIcon::ResizeNorthEast,
                ),
                (
                    Rect::from_min_max(Pos2::new(w.min.x, w.max.y - EDGE * 2.0), Pos2::new(w.min.x + EDGE * 2.0, w.max.y)),
                    ResizeDirection::SouthWest,
                    egui::CursorIcon::ResizeSouthWest,
                ),
                (Rect::from_min_max(w.max - vec2(EDGE * 2.0, EDGE * 2.0), w.max), ResizeDirection::SouthEast, egui::CursorIcon::ResizeSouthEast),
                (Rect::from_min_max(w.min, Pos2::new(w.max.x, w.min.y + EDGE)), ResizeDirection::North, egui::CursorIcon::ResizeNorth),
                (Rect::from_min_max(Pos2::new(w.min.x, w.max.y - EDGE), w.max), ResizeDirection::South, egui::CursorIcon::ResizeSouth),
                (Rect::from_min_max(w.min, Pos2::new(w.min.x + EDGE, w.max.y)), ResizeDirection::West, egui::CursorIcon::ResizeWest),
                (Rect::from_min_max(Pos2::new(w.max.x - EDGE, w.min.y), w.max), ResizeDirection::East, egui::CursorIcon::ResizeEast),
            ];
            for (i, (r, dir, cursor)) in edges.into_iter().enumerate() {
                let resp = ui.interact(r, egui::Id::unique(("wt_resize", i)), Sense::drag()).on_hover_cursor(cursor);
                if resp.drag_started() {
                    ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
                }
            }
        }
    });
}

/// Punaise dessinée : tête, aiguille ; pleine quand l'option est active.
fn pin_icon(p: &egui::Painter, c: Pos2, color: Color32, on: bool) {
    let s = Stroke::new(1.2, color);
    let head = Rect::from_center_size(c + vec2(0.0, -3.0), vec2(8.0, 6.0));
    if on {
        p.rect_filled(head, CornerRadius::same(1), color);
    } else {
        p.rect_stroke(head, CornerRadius::same(1), s, egui::StrokeKind::Middle);
    }
    p.line_segment([c + vec2(-6.0, 0.5), c + vec2(6.0, 0.5)], s);
    p.line_segment([c + vec2(0.0, 0.5), c + vec2(0.0, 7.0)], s);
}
