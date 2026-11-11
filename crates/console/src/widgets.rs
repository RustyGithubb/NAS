//! Composants visuels partagés entre les onglets (repris de WorkTable).

use eframe::egui::{self, CornerRadius, Margin, RichText, Stroke};

use crate::liquid::Liquid;
use crate::theme;

/// En-tête d'onglet, identique partout (comme WorkTable) : sur-titre discret en capitales +
/// grand titre (police « Titres »), suivis d'informations optionnelles sur la même ligne.
pub fn page_header_with(ui: &mut egui::Ui, overline: &str, title: &str, extra: impl FnOnce(&mut egui::Ui)) {
    ui.label(RichText::new(overline.to_uppercase()).small().color(theme::TEXT_MUTED()).strong());
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.set_min_height(HEADER_TITLE_H);
        ui.label(RichText::new(title).font(theme::title_font(32.0)).color(theme::TEXT()));
        ui.add_space(4.0);
        extra(ui);
    });
    ui.add_space(18.0);
}

/// Information à côté du titre (en rouge pour un problème).
pub fn header_badge(ui: &mut egui::Ui, text: &str, alert: bool) {
    ui.label(RichText::new(text).size(14.0).color(if alert { theme::DANGER() } else { theme::TEXT_MUTED() }));
}

/// Hauteur de la ligne de titre (alignement identique d'un onglet à l'autre).
const HEADER_TITLE_H: f32 = 40.0;

/// Largeur de la colonne de gauche de tous les onglets (comme WorkTable).
pub const SIDE_W: f32 = 260.0;

/// Panneau plein : occupe toute la place qu'on lui donne (colonnes des onglets).
pub fn pane<R>(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let jolt = shake_offset(ui.ctx(), ui.max_rect().left() * 0.011);
    shifted(ui, jolt, |ui| pane_inner(ui, body))
}

fn pane_inner<R>(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::SURFACE())
        .stroke(Stroke::new(1.0, theme::BORDER()))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            body(ui)
        })
        .inner
}

/// Découpe l'espace restant en colonnes de largeurs fixes (la dernière prend le reste)
/// et affiche un panneau dans chacune, en disposition verticale.
pub fn pane_columns(ui: &mut egui::Ui, fixed: &[f32], body: impl FnMut(usize, &mut egui::Ui)) {
    pane_columns_with(ui, fixed, |_| true, body);
}

/// Comme `pane_columns`, mais les colonnes pour lesquelles `framed` renvoie `false` reçoivent
/// leur rectangle nu (sans panneau) : elles dessinent leur propre cadre (terminal des Journaux).
pub fn pane_columns_with(ui: &mut egui::Ui, fixed: &[f32], framed: impl Fn(usize) -> bool, mut body: impl FnMut(usize, &mut egui::Ui)) {
    let full = ui.available_rect_before_wrap();
    let gap = ui.spacing().item_spacing.x;
    let mut x = full.left();
    for i in 0..=fixed.len() {
        let w = fixed.get(i).copied().unwrap_or(full.right() - x);
        let rect = egui::Rect::from_min_size(egui::pos2(x, full.top()), egui::vec2(w, full.height()));
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
            if framed(i) {
                pane(ui, |ui| body(i, ui));
            } else {
                let jolt = shake_offset(ui.ctx(), rect.left() * 0.011);
                shifted(ui, jolt, |ui| body(i, ui));
            }
        });
        x += w + gap;
    }
    ui.allocate_rect(full, egui::Sense::hover());
}

/// État vide : message centré, en gris.
pub fn empty_state(ui: &mut egui::Ui, title: &str, hint: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(12.0);
        ui.label(RichText::new(title).color(theme::TEXT_MUTED()));
        ui.label(RichText::new(hint).small().color(theme::TEXT_FAINT()));
    });
}

/// Petit titre en capitales des colonnes de gauche (« FILTRES », « SERVICES »…).
pub fn side_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).small().strong().color(theme::TEXT_MUTED()));
    ui.add_space(4.0);
}

/// Ton d'un élément de liste : normal, en alerte (problème : fond rouge) ou éteint
/// (arrêté, coupé : fond plus sombre, texte estompé).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Alert,
    Off,
}

impl Tone {
    /// Fond de l'élément selon son ton (et s'il est sélectionné).
    pub fn fill(self, selected: bool) -> egui::Color32 {
        match self {
            Tone::Normal => theme::gray(if selected { 0x26 } else { 0x1C }),
            Tone::Alert => theme::blend(theme::gray(0x1C), theme::DANGER(), if selected { 0.42 } else { 0.28 }),
            // Éteint : « creusé » dans le panneau (couleur du fond de la fenêtre), avec un contour.
            Tone::Off => {
                if selected {
                    theme::gray(0x1E)
                } else {
                    theme::BG()
                }
            }
        }
    }

    /// Couleur du texte principal.
    pub fn text(self, selected: bool) -> egui::Color32 {
        match self {
            Tone::Off if !selected => theme::TEXT_FAINT(),
            Tone::Alert => theme::TEXT(),
            _ if selected => theme::TEXT(),
            _ => theme::TEXT_MUTED(),
        }
    }
}

/// Dessin à gauche d'une ligne de navigation (pastille d'état…), au centre donné.
pub type Lead = dyn Fn(&egui::Painter, egui::Pos2);

/// Ligne de navigation des colonnes de gauche (comme WorkTable) : pastille dessinée par
/// `lead`, texte, nombre à droite ; la ligne active reçoit la pastille liquide.
pub fn nav_row(ui: &mut egui::Ui, text: &str, count: Option<usize>, active: bool, tone: Tone, hl: &mut Liquid, lead: Option<&Lead>) -> egui::Response {
    nav_row_h(ui, text, count, active, tone, hl, lead, 32.0)
}

/// `nav_row` de hauteur `h` (volet de l'explorateur, plus serré).
#[allow(clippy::too_many_arguments)]
pub fn nav_row_h(ui: &mut egui::Ui, text: &str, count: Option<usize>, active: bool, tone: Tone, hl: &mut Liquid, lead: Option<&Lead>, h: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    // Fond de ton (en échec, éteint) sous la pastille liquide.
    if tone != Tone::Normal {
        ui.painter().rect_filled(rect, CornerRadius::same(8), tone.fill(false));
        if tone == Tone::Off {
            ui.painter().rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0, theme::BORDER()), egui::StrokeKind::Inside);
        }
    }
    if active {
        hl.target(rect, text);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(6), theme::overlay(7));
    }
    let p = ui.painter();
    let text_x = match lead {
        Some(lead) => {
            lead(p, egui::pos2(rect.left() + 16.0, rect.center().y));
            32.0
        }
        None => 12.0,
    };
    let color = tone.text(active);
    let right = if let Some(n) = count {
        let g = p.layout_no_wrap(n.to_string(), egui::FontId::proportional(12.0), theme::TEXT_FAINT());
        let w = g.size().x;
        p.galley(egui::pos2(rect.right() - 10.0 - w, rect.center().y - g.size().y / 2.0), g, theme::TEXT_FAINT());
        w + 16.0
    } else {
        8.0
    };
    let font = egui::FontId::proportional(14.0);
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - text_x - right);
    let g = p.layout_job(job);
    p.galley(egui::pos2(rect.left() + text_x, rect.center().y - g.size().y / 2.0), g, color);
    resp
}

/// Ligne de titre d'un panneau (comme « Boîte de réception … Actualiser » dans WorkTable) :
/// titre à gauche, actions à droite.
pub fn pane_title(ui: &mut egui::Ui, title: &str, right: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(28.0);
        ui.label(RichText::new(title).size(16.0).color(theme::TEXT()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
    ui.add_space(8.0);
}

/// Élément de liste (carte arrondie). Le sélectionné est surligné ; en alerte, fond rouge et
/// liseré rouge à gauche ; éteint, fond plus sombre. Renvoie la réponse au clic.
pub fn list_item(ui: &mut egui::Ui, selected: bool, tone: Tone, body: impl FnOnce(&mut egui::Ui)) -> egui::Response {
    let fill = tone.fill(selected);
    let bar = (tone == Tone::Alert).then(theme::DANGER);
    let stroke = if tone == Tone::Off { Stroke::new(1.0, theme::BORDER()) } else { Stroke::NONE };
    let out = egui::Frame::new().fill(fill).stroke(stroke).corner_radius(CornerRadius::same(8)).inner_margin(Margin { left: 22, right: 12, top: 8, bottom: 8 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        body(ui);
    });
    let rect = out.response.rect;
    let resp = ui.interact(rect, ui.scope_id().with(("nas_item", rect.min.x as i32, rect.min.y as i32)), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    if let Some(c) = bar {
        let b = egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height()));
        ui.painter().rect_filled(b, CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 }, c);
    }
    if resp.hovered() && !selected {
        ui.painter().rect_filled(rect, CornerRadius::same(8), theme::overlay(5));
    }
    ui.add_space(6.0);
    resp
}

/// Fin d'un groupe de `nav_row` : dessine la pastille liquide (couleur de WorkTable).
pub fn nav_end(ui: &mut egui::Ui, hl: Liquid) {
    hl.end(ui, theme::gray(0x2A), 8.0);
}

/// Tremblement des panneaux pendant que le logo de la barre tourne : amplitude et fréquence
/// croissent avec la vitesse de la roue ; `seed` décale chaque panneau (ils ne tremblent pas en
/// bloc).
pub fn shake_offset(ctx: &egui::Context, seed: f32) -> egui::Vec2 {
    let k = crate::sidebar::spin_intensity();
    if k < 0.04 {
        return egui::Vec2::ZERO;
    }
    // Plus la roue tourne vite, plus c'est fort (progression plus que proportionnelle, plafonnée).
    let amp = (5.0 * k.powf(1.35)).min(16.0);
    let freq = 26.0 + 14.0 * k.min(3.0);
    let t = ctx.input(|i| i.time) as f32;
    let x = (t * freq + seed * 1.7).sin() + 0.5 * (t * freq * 1.9 + seed * 3.1).sin();
    let y = (t * freq * 1.3 + seed * 2.3).cos() + 0.5 * (t * freq * 2.3 + seed * 0.7).sin();
    ctx.request_repaint();
    egui::vec2(x, y) * amp / 1.5
}

/// Affiche `body` décalé de `offset` (effet visuel seulement), sans surcoût quand il est nul.
pub fn shifted<R>(ui: &mut egui::Ui, offset: egui::Vec2, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    if offset == egui::Vec2::ZERO { body(ui) } else { ui.with_visual_transform(egui::emath::TSTransform::from_translation(offset), body).inner }
}

/// Titre de section (cartes, colonnes) : police « Titres ».
pub fn section_title(text: &str) -> RichText {
    RichText::new(text).font(theme::title_font(15.0)).color(theme::TEXT())
}

/// Carte de contenu avec bordure fine.
pub fn card<R>(ui: &mut egui::Ui, title: &str, min_height: f32, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let jolt = shake_offset(ui.ctx(), ui.cursor().top() * 0.013 + title.len() as f32);
    shifted(ui, jolt, |ui| card_inner(ui, title, min_height, body))
}

fn card_inner<R>(ui: &mut egui::Ui, title: &str, min_height: f32, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::SURFACE())
        .stroke(Stroke::new(1.0, theme::BORDER()))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(20))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(min_height);
            if !title.is_empty() {
                ui.label(section_title(title));
                ui.add_space(10.0);
            }
            body(ui)
        })
        .inner
}

/// Sélecteur segmenté : la pastille de sélection glisse (animation liquide) d'une option à l'autre.
pub fn segmented<T: PartialEq + Copy>(ui: &mut egui::Ui, salt: &str, options: &[(T, &str)], value: &mut T) -> bool {
    let mut changed = false;
    egui::Frame::new().fill(theme::BG()).stroke(egui::Stroke::new(1.0, theme::BORDER())).corner_radius(CornerRadius::same(255)).inner_margin(egui::Margin::same(3)).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let mut hl = crate::liquid::Liquid::begin(ui, ("segmented", salt));
                for (i, (opt, label)) in options.iter().enumerate() {
                    let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(13.0), egui::Color32::PLACEHOLDER);
                    let size = egui::vec2(galley.size().x + 24.0, 26.0);
                    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                    if *opt == *value {
                        hl.target(rect, i);
                    }
                    let cov = hl.coverage(rect);
                    let base = if resp.hovered() { theme::TEXT() } else { theme::TEXT_MUTED() };
                    let color = theme::blend(base, theme::BG(), cov.clamp(0.0, 1.0));
                    let text_pos = rect.center() - galley.size() / 2.0;
                    ui.painter().galley(text_pos, galley, color);
                    if resp.clicked() && *opt != *value {
                        *value = *opt;
                        changed = true;
                    }
                }
                hl.end(ui, theme::TEXT(), 13.0);
            });
        },
    );
    changed
}
