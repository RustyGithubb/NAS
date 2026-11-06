//! Direction artistique : reprise de WorkTable (mêmes thèmes, mêmes couleurs).

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

// --- Thèmes ---------------------------------------------------------------------------------
//
// Les couleurs sont lues à chaque image depuis le thème choisi dans les Réglages
// (`BG()`, `TEXT()`… gardent leur nom en capitales : ce sont les « constantes » de la DA).

use std::sync::atomic::{AtomicU32, Ordering};

/// Thèmes proposés (sombres, parmi les plus utilisés des éditeurs de code).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ThemeId {
    #[default]
    Monochrome,
    GithubDark,
    TokyoNight,
    Dracula,
    Unreal,
    Discord,
    Noire,
    Ps2,
    GameBoy,
    Wii,
    WindowsXp,
    Gta6,
    /// Thème retiré d'une ancienne version (Nord, Gruvbox…) : remplacé par Monochrome.
    #[serde(other)]
    Retired,
}

/// Couleurs d'un thème : fond, surfaces, bordure, textes, liens, accent.
#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: u32,
    pub surface: u32,
    pub surface_hi: u32,
    pub border: u32,
    pub text: u32,
    pub muted: u32,
    pub faint: u32,
    pub link: u32,
    pub accent: u32,
}

impl ThemeId {
    pub const ALL: [ThemeId; 12] = [
        ThemeId::Monochrome,
        ThemeId::GithubDark,
        ThemeId::TokyoNight,
        ThemeId::Dracula,
        ThemeId::Unreal,
        ThemeId::Discord,
        ThemeId::Noire,
        ThemeId::Ps2,
        ThemeId::GameBoy,
        ThemeId::Wii,
        ThemeId::WindowsXp,
        ThemeId::Gta6,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ThemeId::Monochrome | ThemeId::Retired => "Monochrome",
            ThemeId::GithubDark => "GitHub Dark",
            ThemeId::TokyoNight => "Tokyo Night",
            ThemeId::Dracula => "Dracula",
            ThemeId::Unreal => "Unreal Engine 5.8",
            ThemeId::Discord => "Discord",
            ThemeId::Noire => t!("Noire", "Black"),
            ThemeId::Ps2 => "PS2",
            ThemeId::GameBoy => "Game Boy",
            ThemeId::Wii => "Wii",
            ThemeId::WindowsXp => "Windows XP",
            ThemeId::Gta6 => "GTA VI",
        }
    }

    pub fn palette(self) -> Palette {
        let p = |bg, surface, surface_hi, border, text, muted, faint, link, accent| Palette { bg, surface, surface_hi, border, text, muted, faint, link, accent };
        match self {
            ThemeId::Monochrome | ThemeId::Retired => p(0x0E0E0E, 0x161616, 0x1F1F1F, 0x2A2A2A, 0xEDEDED, 0x8C8C8C, 0x555555, 0x4C9AFF, 0xD06A4E),
            ThemeId::GithubDark => p(0x0D1117, 0x161B22, 0x21262D, 0x30363D, 0xE6EDF3, 0x8B949E, 0x484F58, 0x58A6FF, 0xF78166),
            ThemeId::TokyoNight => p(0x16161E, 0x1A1B26, 0x24283B, 0x2F3549, 0xC0CAF5, 0x9AA5CE, 0x565F89, 0x7AA2F7, 0xF7768E),
            ThemeId::Dracula => p(0x1E1F29, 0x282A36, 0x343746, 0x44475A, 0xF8F8F2, 0xA9AECB, 0x6272A4, 0x8BE9FD, 0xFF79C6),
            // Éditeur Unreal Engine 5 : gris anthracite, sélection bleue, accent orange.
            ThemeId::Unreal => p(0x0F0F0F, 0x1A1A1A, 0x242424, 0x383838, 0xE6E6E6, 0x9E9E9E, 0x5C5C5C, 0x26BBFF, 0xF08D1A),
            // Discord : gris bleutés, « blurple ».
            ThemeId::Discord => p(0x1E1F22, 0x2B2D31, 0x313338, 0x3F4147, 0xF2F3F5, 0xB5BAC1, 0x6D6F78, 0x00A8FC, 0x5865F2),
            // Noire : noir absolu, blanc pur, rouge cinéma.
            ThemeId::Noire => p(0x000000, 0x080808, 0x121212, 0x1E1E1E, 0xFFFFFF, 0x8A8A8A, 0x464646, 0xBFBFBF, 0xD11F2E),
            // PS2 : bleu nuit du menu de démarrage, bleu vif.
            ThemeId::Ps2 => p(0x03060F, 0x0A1230, 0x13204F, 0x1F306E, 0xE6ECFF, 0x8E9CC8, 0x4A5788, 0x4DA3FF, 0x2F6BFF),
            // Game Boy (DMG) : les quatre verts de l'écran.
            ThemeId::GameBoy => p(0x0F380F, 0x1A461A, 0x306230, 0x3F7232, 0x9BBC0F, 0x8BAC0F, 0x5E8A1E, 0xC6DE5A, 0xE2F28C),
            // Wii : menu blanc et gris perle, bleu Wii (thème clair).
            ThemeId::Wii => p(0xE6EBEF, 0xFFFFFF, 0xF1F4F7, 0xC9D2D9, 0x2B3035, 0x6B747C, 0xA3ABB2, 0x1E9BD7, 0x34BEED),
            // Windows XP (Luna) : beige système, fenêtres blanches, bleu et vert du bouton Démarrer (thème clair).
            ThemeId::WindowsXp => p(0xECE9D8, 0xFFFFFF, 0xF5F3E8, 0xACA899, 0x1B1B1B, 0x555555, 0x8C8C8C, 0x245EDC, 0x3C9A2A),
            // GTA VI : coucher de soleil de Vice City, violet nuit, néons rose et turquoise.
            ThemeId::Gta6 => p(0x120A1F, 0x1C1030, 0x2A1846, 0x3D2560, 0xFCE9F7, 0xC59BD6, 0x7A5A95, 0x3FE0D0, 0xFF4FA3),
        }
    }
}

pub fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

static COLORS: [AtomicU32; 9] = [
    AtomicU32::new(0x0E0E0E),
    AtomicU32::new(0x161616),
    AtomicU32::new(0x1F1F1F),
    AtomicU32::new(0x2A2A2A),
    AtomicU32::new(0xEDEDED),
    AtomicU32::new(0x8C8C8C),
    AtomicU32::new(0x555555),
    AtomicU32::new(0x4C9AFF),
    AtomicU32::new(0xD06A4E),
];

fn color(i: usize) -> Color32 {
    rgb(COLORS[i].load(Ordering::Relaxed))
}

#[allow(non_snake_case)]
pub fn BG() -> Color32 {
    color(0)
}
#[allow(non_snake_case)]
pub fn SURFACE() -> Color32 {
    color(1)
}
#[allow(non_snake_case)]
pub fn SURFACE_HI() -> Color32 {
    color(2)
}
#[allow(non_snake_case)]
pub fn BORDER() -> Color32 {
    color(3)
}
#[allow(non_snake_case)]
pub fn TEXT() -> Color32 {
    color(4)
}
#[allow(non_snake_case)]
pub fn TEXT_MUTED() -> Color32 {
    color(5)
}
#[allow(non_snake_case, dead_code)]
pub fn TEXT_FAINT() -> Color32 {
    color(6)
}
/// Couleur des liens.
#[allow(non_snake_case, dead_code)]
pub fn LINK() -> Color32 {
    color(7)
}
/// Accent du thème (terracotta en Monochrome) : jour actuel, non lus, pastilles.
#[allow(non_snake_case)]
pub fn TODAY() -> Color32 {
    color(8)
}
/// Fond de la barre latérale : légèrement translucide, elle flotte au-dessus du contenu.
#[allow(non_snake_case)]
pub fn SIDEBAR_BG() -> Color32 {
    let s = SURFACE();
    let b = BG();
    let mix = |a: u8, c: u8| ((a as u16 + c as u16) / 2 + 6).min(255) as u8;
    Color32::from_rgba_unmultiplied(mix(s.r(), b.r()), mix(s.g(), b.g()), mix(s.b(), b.b()), 0xF2)
}

/// Thème clair (fond lumineux) ?
pub fn is_light() -> bool {
    let b = BG();
    (b.r() as u32 + b.g() as u32 + b.b() as u32) > 3 * 128
}

/// Voile de survol (blanc translucide en sombre, couleur du texte en clair).
pub fn overlay(alpha: u8) -> Color32 {
    let c = TEXT();
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

/// Mélange de deux couleurs (t = 0 → a, t = 1 → b).
pub fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Applique un thème (couleurs + styles egui).
pub fn set_theme(ctx: &egui::Context, id: ThemeId) {
    let p = id.palette();
    for (i, c) in [p.bg, p.surface, p.surface_hi, p.border, p.text, p.muted, p.faint, p.link, p.accent].into_iter().enumerate() {
        COLORS[i].store(c, Ordering::Relaxed);
    }
    apply_visuals(ctx);
}

/// Espace réservé à gauche pour que la barre flottante ne recouvre jamais le contenu.
/// = marge écran (16) + largeur pilule (56) + respiration (32).
pub const CONTENT_LEFT_INSET: i8 = 104;

/// Famille du groupe « Titres » (titres d'onglets et de cartes, toujours en gras).
pub const TITLE_FONT: &str = "nas-title";

/// Police des titres.
pub fn title_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(TITLE_FONT.into()))
}

/// Polices : interface en Ubuntu (intégrée à egui), titres en Inter gras, code en Hack.
fn set_fonts(ctx: &egui::Context) {
    use std::sync::Arc;
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("inter-bold".into(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/Inter-Bold.otf"))));
    fonts.font_data.insert("inter".into(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.otf"))));
    let proportional = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    // Inter en repli juste après la police principale : symboles absents d'Ubuntu.
    let mut ui_chain = proportional.clone();
    ui_chain.insert(1.min(ui_chain.len()), "inter".into());
    fonts.families.insert(FontFamily::Proportional, ui_chain);
    let mut title = vec!["inter-bold".to_string()];
    title.extend(proportional);
    fonts.families.insert(FontFamily::Name(TITLE_FONT.into()), title);
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    set_fonts(ctx);
    ctx.set_visuals(egui::Visuals::dark());
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::new(28.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
        ]
        .into();

        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
    });
    apply_visuals(ctx);
}

/// Couleurs des widgets egui d'après le thème courant.
pub fn apply_visuals(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        v.dark_mode = !is_light();
        v.override_text_color = Some(TEXT());
        v.panel_fill = BG();
        v.window_fill = SURFACE();
        v.extreme_bg_color = BG();
        v.faint_bg_color = SURFACE();
        v.window_stroke = Stroke::new(1.0, BORDER());
        v.selection.bg_fill = blend(SURFACE_HI(), TEXT(), 0.18);
        v.selection.stroke = Stroke::new(1.0, TEXT());
        v.hyperlink_color = TEXT();

        let r = CornerRadius::same(8);
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = r;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER());
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_MUTED());
        v.widgets.inactive.bg_fill = SURFACE_HI();
        v.widgets.inactive.weak_bg_fill = SURFACE_HI();
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT());
        v.widgets.hovered.bg_fill = blend(SURFACE_HI(), TEXT(), 0.06);
        v.widgets.hovered.weak_bg_fill = blend(SURFACE_HI(), TEXT(), 0.06);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, blend(BORDER(), TEXT(), 0.15));
        v.widgets.active.bg_fill = blend(SURFACE_HI(), TEXT(), 0.1);
        v.widgets.active.weak_bg_fill = blend(SURFACE_HI(), TEXT(), 0.1);
        // Texte des widgets survolés / actifs (et du texte « fort ») : couleur du thème.
        v.widgets.hovered.fg_stroke = Stroke::new(1.5, TEXT());
        v.widgets.active.fg_stroke = Stroke::new(2.0, TEXT());
        v.widgets.open.fg_stroke = Stroke::new(1.0, TEXT());
        v.widgets.open.bg_fill = SURFACE_HI();
        v.widgets.open.weak_bg_fill = SURFACE_HI();
        v.widgets.noninteractive.bg_fill = SURFACE();
        v.widgets.noninteractive.weak_bg_fill = SURFACE();
        v.window_shadow.color = Color32::from_black_alpha(if is_light() { 40 } else { 96 });
        v.popup_shadow.color = Color32::from_black_alpha(if is_light() { 40 } else { 96 });
    });
}
