//! Tableau de bord : état du serveur reçu de l'agent. Quatre tuiles, le processeur sur toute
//! la largeur (cœurs en grille), puis le réseau et le stockage côte à côte jusqu'en bas.

use eframe::egui::{self, CornerRadius, Margin, Rect, Sense, Stroke, vec2};
use nas_proto::{Network, SpeedState};

use super::{header, unavailable};
use crate::server::{Link, NetError, Server};
use crate::{theme, widgets};

/// Seuil au-delà duquel une jauge ou une tuile passe au rouge (%).
const HIGH: f32 = 90.0;
/// Température au-delà de laquelle la tuile passe au rouge (°C) : sans ventilateur, le Mac mini
/// doit rester en dessous.
const HOT_C: f32 = 85.0;
/// Signal Wi-Fi sous lequel la ligne passe au rouge (sur 100).
const WEAK_SIGNAL: u8 = 40;
/// Part de pings perdus à partir de laquelle la tuile passe au rouge (%).
const LOSS_ALERT: f32 = 5.0;
/// Coupure considérée comme récente (ms).
const RECENT_DROP_MS: i64 = 3600 * 1000;
/// Part de la largeur donnée au réseau (le stockage prend le reste).
const NET_SHARE: f32 = 0.62;

pub fn show(ui: &mut egui::Ui, link: &Link, addr: &str, server: &Server) {
    let overline = t!("Tableau de bord", "Dashboard");
    if unavailable(ui, overline, t!("Serveur", "Server"), link, addr) {
        return;
    }
    let Link::Online(s) = link else { return };
    server.poll_network();
    let net = server.data().network.clone();
    let alerts = net.as_ref().and_then(|n| n.as_ref().ok()).map(net_alerts).unwrap_or_default();
    let mem = pct(s.memory.used, s.memory.total);
    let hot = s.temperature_c.is_some_and(|t| t >= HOT_C);
    let disk_full = s.disks.iter().any(|d| pct(d.used, d.total) >= HIGH);
    // En alerte, l'information à côté du nom dit pourquoi (sinon : le système).
    let reasons: Vec<&str> = [
        (s.cpu.usage >= HIGH, t!("Processeur saturé", "CPU saturated")),
        (mem >= HIGH, t!("Mémoire saturée", "Memory saturated")),
        (hot, t!("Trop chaud", "Too hot")),
        (disk_full, t!("Disque presque plein", "Disk almost full")),
        (alerts.weak, t!("Wi-Fi faible", "Weak Wi-Fi")),
        (alerts.loss, t!("Paquets perdus", "Packet loss")),
        (alerts.dropped, t!("Coupure récente", "Recent drop")),
    ]
    .into_iter()
    .filter_map(|(on, why)| on.then_some(why))
    .collect();
    let badge = if reasons.is_empty() { s.os.clone() } else { reasons.join("  ·  ") };
    header(ui, overline, &s.hostname, &badge, !reasons.is_empty());

    // Une seule grille, un seul espacement : le même écart entre les tuiles, entre les tuiles et
    // les panneaux, et entre les panneaux (celui des colonnes de tous les onglets).
    let gap = ui.spacing().item_spacing.x;
    let full = ui.available_rect_before_wrap();
    let tile_w = (full.width() - 3.0 * gap) / 4.0;
    let temp = s.temperature_c.map(|t| format!("{t:.0} °C")).unwrap_or_else(|| "—".into());
    let temp_note = if s.temperature_c.is_some() { t!("la plus haute", "highest") } else { t!("non disponible", "not available") };
    let tiles: [Tile; 4] = [
        ("cpu", t!("Processeur", "CPU"), format!("{:.0} %", s.cpu.usage), tf!("{} cœurs", "{} cores", s.cpu.per_core.len()), Some(s.cpu.usage), s.cpu.usage >= HIGH),
        ("mem", t!("Mémoire", "Memory"), format!("{mem:.0} %"), format!("{} / {}", bytes(s.memory.used), bytes(s.memory.total)), Some(mem), mem >= HIGH),
        ("temp", t!("Température", "Temperature"), temp, temp_note.to_string(), None, hot),
        ("up", t!("En marche depuis", "Up for"), uptime(s.uptime_s), tf!("agent v{}", "agent v{}", s.agent_version), None, false),
    ];
    for (i, (key, label, value, note, gauge, alert)) in tiles.iter().enumerate() {
        let r = Rect::from_min_size(full.min + vec2(i as f32 * (tile_w + gap), 0.0), vec2(tile_w, TILE_H));
        ui.scope_builder(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min)), |ui| tile(ui, key, label, value, note, *gauge, *alert));
    }

    // Processeur sur toute la largeur, cœurs en grille ; stockage en dessous, jusqu'en bas.
    let inner_w = full.width() - 2.0 * PANE_PAD;
    let fit = ((inner_w + CELL_GAP) / (CORE_CELL_MIN + CELL_GAP)).floor().clamp(1.0, 8.0) as usize;
    let n = s.cpu.per_core.len().max(1);
    // Lignes aussi pleines que possible : 16 cœurs sur 4 colonnes → 4 lignes complètes.
    let core_rows = n.div_ceil(fit);
    let cols = n.div_ceil(core_rows);
    let swap_rows = usize::from(s.memory.swap_total > 0);
    let cpu_h = PANE_PAD * 2.0 + TITLE_H + (core_rows + swap_rows) as f32 * ROW_H;
    let cpu_rect = Rect::from_min_size(full.min + vec2(0.0, TILE_H + gap), vec2(full.width(), cpu_h));
    // En bas : réseau à gauche, stockage à droite.
    let bottom = Rect::from_min_max(egui::pos2(full.left(), cpu_rect.bottom() + gap), full.max);
    let net_w = ((bottom.width() - gap) * NET_SHARE).round();
    let net_rect = Rect::from_min_size(bottom.min, vec2(net_w, bottom.height()));
    let disk_rect = Rect::from_min_max(egui::pos2(net_rect.right() + gap, bottom.top()), bottom.max);
    in_rect(ui, net_rect, |ui| widgets::pane(ui, |ui| network(ui, server, &net, &alerts)));

    in_rect(ui, cpu_rect, |ui| {
        widgets::pane(ui, |ui| {
            // Modèle à droite du titre, en petit.
            widgets::pane_title(ui, t!("Processeur", "CPU"), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&s.cpu.model).small().color(theme::TEXT_FAINT())).truncate());
            });
            let top = ui.cursor().min;
            let w = ui.available_width();
            let cell_w = (w - (cols as f32 - 1.0) * CELL_GAP) / cols as f32;
            for (i, u) in s.cpu.per_core.iter().enumerate() {
                let (c, r) = (i % cols, i / cols);
                let cell = Rect::from_min_size(top + vec2(c as f32 * (cell_w + CELL_GAP), r as f32 * ROW_H), vec2(cell_w, ROW_H));
                gauge_row(ui, cell, ("core", i), &tf!("Cœur {}", "Core {}", i + 1), 72.0, *u, &format!("{u:.0} %"), 44.0);
            }
            if swap_rows > 0 {
                let cell = Rect::from_min_size(top + vec2(0.0, core_rows as f32 * ROW_H), vec2(w, ROW_H));
                let swap = pct(s.memory.swap_used, s.memory.swap_total);
                gauge_row(ui, cell, "swap", t!("Mémoire d'échange", "Swap"), 150.0, swap, &format!("{} / {}", bytes(s.memory.swap_used), bytes(s.memory.swap_total)), 150.0);
            }
            ui.allocate_space(vec2(w, (core_rows + swap_rows) as f32 * ROW_H));
        });
    });

    in_rect(ui, disk_rect, |ui| {
        widgets::pane(ui, |ui| {
            widgets::pane_title(ui, t!("Stockage", "Storage"), |ui| {
                ui.label(egui::RichText::new(tf!("{} disques", "{} disks", s.disks.len())).color(theme::TEXT_FAINT()));
            });
            egui::ScrollArea::vertical().id_salt("dash_disks").auto_shrink([false, false]).show(ui, |ui| {
                if s.disks.is_empty() {
                    widgets::empty_state(ui, t!("Aucun disque détecté", "No disk detected"), "");
                }
                let w = ui.available_width();
                for (i, d) in s.disks.iter().enumerate() {
                    let label = format!("{}  ({})", d.mount, d.fs);
                    let text = tf!("{} / {}  ·  {} disponibles", "{} / {}  ·  {} available", bytes(d.used), bytes(d.total), bytes(d.available));
                    if w >= 640.0 {
                        let (cell, _) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::hover());
                        gauge_row(ui, cell, ("disk", i), &label, 220.0, pct(d.used, d.total), &text, 240.0);
                    } else {
                        // Panneau étroit : nom puis chiffres, jauge sur toute la largeur en dessous.
                        let (cell, _) = ui.allocate_exact_size(vec2(w, DISK_NARROW_H), Sense::hover());
                        let p = ui.painter();
                        p.with_clip_rect(cell).text(cell.left_top() + vec2(0.0, 2.0), egui::Align2::LEFT_TOP, &label, egui::FontId::proportional(13.0), theme::TEXT());
                        p.with_clip_rect(cell).text(cell.left_top() + vec2(0.0, 20.0), egui::Align2::LEFT_TOP, &text, egui::FontId::proportional(12.0), theme::TEXT_MUTED());
                        let g = Rect::from_min_size(cell.left_top() + vec2(0.0, 40.0), vec2(w, 6.0));
                        bar(ui, ("disk", i), g, pct(d.used, d.total));
                    }
                }
            });
        });
    });
    ui.allocate_rect(full, Sense::hover());
}

/// Ce qui, dans le réseau, mérite le rouge.
#[derive(Default)]
struct NetAlerts {
    weak: bool,
    loss: bool,
    dropped: bool,
}

fn net_alerts(n: &Network) -> NetAlerts {
    NetAlerts {
        weak: n.wifi.as_ref().is_some_and(|w| w.signal < WEAK_SIGNAL),
        loss: n.ping_gateway.loss_pct >= LOSS_ALERT || n.ping_internet.loss_pct >= LOSS_ALERT,
        dropped: n.last_disconnect.as_ref().is_some_and(|(at, _)| now_ms() - at < RECENT_DROP_MS),
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Panneau Réseau : quatre chiffres clés, la connexion à gauche, le débit en direct à droite.
fn network(ui: &mut egui::Ui, server: &Server, net: &Option<Result<Network, NetError>>, alerts: &NetAlerts) {
    let n = match net {
        None => {
            widgets::pane_title(ui, t!("Réseau", "Network"), |_| {});
            ui.vertical_centered(|ui| ui.spinner());
            return;
        }
        Some(Err(NetError::Outdated)) => {
            widgets::pane_title(ui, t!("Réseau", "Network"), |_| {});
            ui.add_space(24.0);
            widgets::empty_state(ui, t!("Agent trop ancien", "Agent too old"), t!("Mets-le à jour : Réglages › Serveur › Rechercher", "Update it: Settings › Server › Check now"));
            return;
        }
        Some(Err(NetError::Other(e))) => {
            widgets::pane_title(ui, t!("Réseau", "Network"), |_| {});
            ui.add_space(24.0);
            widgets::empty_state(ui, t!("Réseau illisible", "Network unavailable"), e);
            return;
        }
        Some(Ok(n)) => n,
    };
    let running = n.speedtest.state == SpeedState::Running;
    widgets::pane_title(ui, t!("Réseau", "Network"), |ui| {
        if running {
            ui.spinner();
            ui.label(egui::RichText::new(t!("Test en cours…", "Testing…")).color(theme::TEXT_MUTED()));
        } else if ui.button(t!("Tester le débit", "Speed test")).on_hover_text(t!("Télécharge 100 Mo depuis un serveur d'OVH", "Downloads 100 MB from an OVH server")).clicked()
        {
            server.speedtest();
        }
        if n.simulated {
            ui.label(egui::RichText::new(t!("simulé", "simulated")).small().color(theme::TEXT_FAINT()));
        }
    });

    egui::ScrollArea::vertical().id_salt("dash_net").auto_shrink([false, false]).show(ui, |ui| {
        // Quatre chiffres clés.
        let gap = 12.0;
        let w = ui.available_width();
        let (row, _) = ui.allocate_exact_size(vec2(w, STAT_H), Sense::hover());
        let cw = (w - 3.0 * gap) / 4.0;
        let ms = |p: &nas_proto::Ping| p.avg_ms.map(|v| format!("{v:.0} ms")).unwrap_or_else(|| "—".into());
        let loss = |p: &nas_proto::Ping| tf!("{:.0} % perdus", "{:.0} % lost", p.loss_pct);
        let drops = match &n.last_disconnect {
            Some((at, _)) => tf!("dernière {}", "last {}", ago(*at)),
            None => t!("aucune", "none").to_string(),
        };
        let speed = match &n.speedtest {
            t if t.state == SpeedState::Done => (rate(t.bytes_per_s), format!("{:.0} Mbit/s · {}", t.bytes_per_s as f64 * 8.0 / 1e6, ago(t.at_ms))),
            t if t.state == SpeedState::Failed => ("✗".into(), t.message.clone()),
            _ => ("—".into(), t!("pas encore testé", "not tested yet").to_string()),
        };
        let stats = [
            (t!("Latence box", "Router latency"), ms(&n.ping_gateway), loss(&n.ping_gateway), n.ping_gateway.loss_pct >= LOSS_ALERT),
            (t!("Latence internet", "Internet latency"), ms(&n.ping_internet), loss(&n.ping_internet), n.ping_internet.loss_pct >= LOSS_ALERT),
            (t!("Coupures Wi-Fi", "Wi-Fi drops"), n.disconnects.to_string(), drops, alerts.dropped),
            (t!("Débit internet", "Internet speed"), speed.0, speed.1, n.speedtest.state == SpeedState::Failed),
        ];
        for (i, (label, value, note, alert)) in stats.iter().enumerate() {
            let r = Rect::from_min_size(row.min + vec2(i as f32 * (cw + gap), 0.0), vec2(cw, STAT_H));
            stat(ui, r, label, value, note, *alert);
        }
        ui.add_space(12.0);

        // Connexion à gauche, débit en direct à droite.
        let w = ui.available_width();
        let left_w = (w * 0.48).max(220.0).min(w);
        let top = ui.cursor().min;
        let mut rows: Vec<(&str, String, bool)> = Vec::new();
        if let Some(wf) = &n.wifi {
            rows.push((t!("Wi-Fi", "Wi-Fi"), wf.ssid.clone(), false));
            rows.push((t!("Signal", "Signal"), format!("{} %", wf.signal), wf.signal < WEAK_SIGNAL));
            rows.push((t!("Bande · canal", "Band · channel"), format!("{} · {}", wf.band(), wf.channel), false));
            rows.push((t!("Liaison", "Link"), format!("{} Mbit/s", wf.rate_mbps), false));
            rows.push((t!("Point d'accès", "Access point"), wf.bssid.clone(), false));
        } else if !n.iface.is_empty() {
            rows.push((t!("Connexion", "Connection"), if n.wireless { "Wi-Fi".into() } else { t!("Câble", "Cable").into() }, false));
        }
        if let Some(since) = n.connected_since_ms {
            rows.push((t!("Connecté", "Connected"), ago(since), false));
        }
        rows.push((t!("Adresse IP", "IP address"), n.ipv4.clone(), false));
        rows.push((t!("Passerelle", "Gateway"), n.gateway.clone(), false));
        rows.push(("DNS", n.dns.join(", "), false));
        rows.push((t!("Interface", "Interface"), format!("{}  ·  {}", n.iface, n.mac), false));
        if let Some((at, why)) = &n.last_disconnect {
            rows.push((t!("Dernière coupure", "Last drop"), format!("{} · {why}", ago(*at)), alerts.dropped));
        }
        let font = egui::FontId::proportional(13.0);
        for (i, (k, v, alert)) in rows.iter().enumerate() {
            let y = top.y + i as f32 * INFO_H;
            let p = ui.painter();
            p.text(egui::pos2(top.x, y + INFO_H / 2.0), egui::Align2::LEFT_CENTER, *k, font.clone(), theme::TEXT_MUTED());
            let color = if *alert { theme::DANGER() } else { theme::TEXT() };
            let clip = Rect::from_min_max(egui::pos2(top.x + 120.0, y), egui::pos2(top.x + left_w - 8.0, y + INFO_H));
            p.with_clip_rect(clip).text(egui::pos2(clip.left(), y + INFO_H / 2.0), egui::Align2::LEFT_CENTER, v, font.clone(), color);
        }
        let info_h = rows.len() as f32 * INFO_H;
        let graph = Rect::from_min_size(egui::pos2(top.x + left_w + 12.0, top.y), vec2((w - left_w - 12.0).max(0.0), info_h.max(160.0)));
        if graph.width() > 120.0 {
            traffic(ui, graph, n);
        }
        ui.allocate_space(vec2(w, info_h.max(160.0)));
    });
}

/// Hauteur d'un disque en panneau étroit (deux lignes et la jauge).
const DISK_NARROW_H: f32 = 58.0;
/// Hauteur des chiffres clés et d'une ligne d'information du réseau.
const STAT_H: f32 = 74.0;
const INFO_H: f32 = 22.0;

/// Chiffre clé du réseau : intitulé, valeur, note ; rouge en alerte.
fn stat(ui: &egui::Ui, r: Rect, label: &str, value: &str, note: &str, alert: bool) {
    let p = ui.painter();
    let (fill, border, color) =
        if alert { (theme::blend(theme::SURFACE(), theme::DANGER(), 0.22), theme::DANGER(), theme::DANGER()) } else { (theme::SURFACE_HI(), theme::BORDER(), theme::TEXT()) };
    p.rect(r, CornerRadius::same(10), fill, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    let x = r.left() + 12.0;
    p.text(egui::pos2(x, r.top() + 16.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(12.0), theme::TEXT_MUTED());
    p.text(egui::pos2(x, r.top() + 38.0), egui::Align2::LEFT_CENTER, value, theme::title_font(20.0), color);
    let clip = r.shrink(4.0);
    p.with_clip_rect(clip).text(egui::pos2(x, r.top() + 60.0), egui::Align2::LEFT_CENTER, note, egui::FontId::proportional(11.5), theme::TEXT_MUTED());
}

/// Courbe du débit reçu (plein) et envoyé (trait) sur les 5 dernières minutes.
fn traffic(ui: &egui::Ui, r: Rect, n: &Network) {
    let p = ui.painter();
    let font = egui::FontId::proportional(12.5);
    p.text(r.left_top(), egui::Align2::LEFT_TOP, tf!("↓ {}   ↑ {}", "↓ {}   ↑ {}", rate(n.rx_bps), rate(n.tx_bps)), egui::FontId::proportional(15.0), theme::TEXT());
    p.text(
        egui::pos2(r.left(), r.top() + 22.0),
        egui::Align2::LEFT_TOP,
        tf!("Depuis le démarrage : ↓ {}  ↑ {}", "Since boot: ↓ {}  ↑ {}", bytes(n.rx_total), bytes(n.tx_total)),
        font.clone(),
        theme::TEXT_MUTED(),
    );
    let g = Rect::from_min_max(egui::pos2(r.left(), r.top() + 46.0), r.max);
    p.rect_filled(g, CornerRadius::same(8), theme::SURFACE_HI());
    let h = &n.history;
    if h.len() < 2 {
        return;
    }
    let max = h.iter().map(|(a, b)| (*a).max(*b)).max().unwrap_or(1).max(10_000) as f32 * 1.15;
    let inner = g.shrink(6.0);
    let step = inner.width() / (300.0 - 1.0);
    let start = 300 - h.len();
    let pt = |i: usize, v: u64| egui::pos2(inner.left() + (start + i) as f32 * step, inner.bottom() - v as f32 / max * inner.height());
    let rx: Vec<egui::Pos2> = h.iter().enumerate().map(|(i, (v, _))| pt(i, *v)).collect();
    let tx: Vec<egui::Pos2> = h.iter().enumerate().map(|(i, (_, v))| pt(i, *v)).collect();
    // Surface sous la courbe reçue, en bandes verticales (forme non convexe).
    for w in rx.windows(2) {
        let quad = vec![w[0], w[1], egui::pos2(w[1].x, inner.bottom()), egui::pos2(w[0].x, inner.bottom())];
        p.add(egui::Shape::convex_polygon(quad, theme::overlay(18), Stroke::NONE));
    }
    p.add(egui::Shape::line(rx, Stroke::new(1.5, theme::TEXT())));
    p.add(egui::Shape::line(tx, Stroke::new(1.2, theme::TEXT_MUTED())));
    p.text(egui::pos2(inner.right(), inner.top()), egui::Align2::RIGHT_TOP, rate(max as u64), egui::FontId::proportional(11.0), theme::TEXT_FAINT());
    p.text(egui::pos2(inner.left(), inner.bottom()), egui::Align2::LEFT_BOTTOM, t!("il y a 5 min", "5 min ago"), egui::FontId::proportional(11.0), theme::TEXT_FAINT());
}

/// Débit lisible : « 8,2 Mo/s ».
fn rate(bps: u64) -> String {
    format!("{}/s", bytes(bps))
}

/// « il y a 40 min », « il y a 2 h », « il y a 12 s ».
fn ago(ms: i64) -> String {
    let s = ((now_ms() - ms) / 1000).max(0) as u64;
    let d = match s {
        0..60 => format!("{s} s"),
        60..3600 => format!("{} min", s / 60),
        3600..86_400 => format!("{} h {:02}", s / 3600, s % 3600 / 60),
        _ => tf!("{} j", "{} d", s / 86_400),
    };
    tf!("il y a {}", "{} ago", d)
}

/// Marge intérieure des panneaux (celle de `widgets::pane`).
const PANE_PAD: f32 = 16.0;
/// Ligne de titre d'un panneau (`widgets::pane_title`), espace compris.
const TITLE_H: f32 = 44.0;
/// Hauteur d'une ligne de jauge.
const ROW_H: f32 = 30.0;
/// Largeur minimale d'une case « cœur » dans la grille, et écart entre les cases.
const CORE_CELL_MIN: f32 = 220.0;
const CELL_GAP: f32 = 24.0;

/// Dispose `body` dans `r`, en haut à gauche.
fn in_rect(ui: &mut egui::Ui, r: Rect, body: impl FnOnce(&mut egui::Ui)) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min)), body);
}

/// Tuile : clé d'animation, intitulé, valeur, note, jauge facultative, en alerte.
type Tile<'a> = (&'a str, &'a str, String, String, Option<f32>, bool);

/// Hauteur des tuiles.
const TILE_H: f32 = 112.0;

/// Tuile : intitulé discret, grande valeur, note, et jauge facultative (0 à 100). En alerte
/// (seuil dépassé) : fond et contour rouges, valeur en rouge.
fn tile(ui: &mut egui::Ui, key: &str, label: &str, value: &str, note: &str, gauge: Option<f32>, alert: bool) {
    let jolt = crate::widgets::shake_offset(ui.ctx(), ui.max_rect().left() * 0.011);
    crate::widgets::shifted(ui, jolt, |ui| tile_inner(ui, key, label, value, note, gauge, alert));
}

fn tile_inner(ui: &mut egui::Ui, key: &str, label: &str, value: &str, note: &str, gauge: Option<f32>, alert: bool) {
    let (fill, border, value_color) =
        if alert { (theme::blend(theme::SURFACE(), theme::DANGER(), 0.22), theme::DANGER(), theme::DANGER()) } else { (theme::SURFACE(), theme::BORDER(), theme::TEXT()) };
    egui::Frame::new().fill(fill).stroke(Stroke::new(1.0, border)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(16)).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(egui::RichText::new(label).small().color(theme::TEXT_MUTED()));
        ui.label(egui::RichText::new(value).font(theme::title_font(26.0)).color(value_color));
        ui.label(egui::RichText::new(note).color(theme::TEXT_MUTED()));
        ui.add_space(8.0);
        // Hauteur identique pour toutes les tuiles, avec ou sans jauge.
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
        if let Some(v) = gauge {
            bar(ui, key, r, v);
        }
    });
}

/// Ligne de jauge dans `cell` : intitulé à gauche (`label_w`), valeur à droite (`text_w`), et la
/// jauge prend toute la largeur restante.
#[allow(clippy::too_many_arguments)]
fn gauge_row(ui: &egui::Ui, cell: Rect, key: impl std::hash::Hash + std::fmt::Debug, label: &str, label_w: f32, value: f32, text: &str, text_w: f32) {
    let font = egui::FontId::proportional(13.0);
    let l = Rect::from_min_size(cell.min, vec2(label_w, cell.height()));
    // Intitulé trop long (point de montage) : coupé au bord de sa colonne.
    ui.painter().with_clip_rect(l.with_max_x(l.right() - 8.0)).text(l.left_center(), egui::Align2::LEFT_CENTER, label, font.clone(), theme::TEXT());
    ui.painter().text(cell.right_center(), egui::Align2::RIGHT_CENTER, text, font, theme::TEXT_MUTED());
    let g = Rect::from_min_max(egui::pos2(l.right(), cell.center().y - 3.0), egui::pos2((cell.right() - text_w - 12.0).max(l.right() + 20.0), cell.center().y + 3.0));
    bar(ui, key, g, value);
}

/// Jauge arrondie ; la valeur glisse en douceur d'une mesure à l'autre.
fn bar(ui: &egui::Ui, key: impl std::hash::Hash + std::fmt::Debug, r: Rect, value: f32) {
    let v = ui.ctx().animate_value_with_time(egui::Id::unique(("nas_bar", key)), value.clamp(0.0, 100.0), 0.6);
    let p = ui.painter();
    let round = CornerRadius::same((r.height() / 2.0) as u8);
    p.rect_filled(r, round, theme::SURFACE_HI());
    let fill = if v >= HIGH { theme::DANGER() } else { theme::TEXT() };
    let w = (r.width() * v / 100.0).max(if v > 0.0 { r.height() } else { 0.0 });
    p.rect_filled(Rect::from_min_size(r.min, vec2(w, r.height())), round, fill);
}

fn pct(used: u64, total: u64) -> f32 {
    if total == 0 { 0.0 } else { used as f32 * 100.0 / total as f32 }
}

/// Taille lisible : « 3,2 Go » / « 3.2 GB ».
pub fn bytes(n: u64) -> String {
    const UNITS_FR: [&str; 5] = ["o", "Ko", "Mo", "Go", "To"];
    const UNITS_EN: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let units = t!(UNITS_FR, UNITS_EN);
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    let s = if i == 0 || v >= 100.0 { format!("{v:.0}") } else { format!("{v:.1}") };
    let s = if crate::i18n::en() { s } else { s.replace('.', ",") };
    format!("{s} {}", units[i])
}

/// Durée lisible : « 2 j 4 h », « 3 h 12 min », « 5 min ».
fn uptime(s: u64) -> String {
    let (d, h, m) = (s / 86_400, (s % 86_400) / 3600, (s % 3600) / 60);
    if d > 0 {
        tf!("{d} j {h} h", "{d} d {h} h")
    } else if h > 0 {
        format!("{h} h {m} min")
    } else {
        format!("{m} min")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        crate::i18n::set(crate::i18n::Language::French);
        assert_eq!(bytes(8 * 1024 * 1024 * 1024), "8,0 Go");
        assert_eq!(bytes(512), "512 o");
        assert_eq!(uptime(2 * 86_400 + 4 * 3600 + 60), "2 j 4 h");
        assert_eq!(uptime(3 * 3600 + 12 * 60), "3 h 12 min");
        assert_eq!(pct(1, 4), 25.0);
        assert_eq!(pct(1, 0), 0.0);
    }
}
