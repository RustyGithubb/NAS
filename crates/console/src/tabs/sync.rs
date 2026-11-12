//! Synchro : synchronisation des apps (WorkTable et les suivantes) avec le serveur.
//! Colonne de gauche : vue d'ensemble, répertoire d'apps, comptes. À droite : le détail.
//!
//! Les données des comptes sont chiffrées par les apps : la console ne voit que les comptes, les
//! postes, l'espace occupé et l'activité, jamais le contenu.

use eframe::egui::{self, RichText};
use nas_proto::sync::{ActivityKind, AppForm, AppInfo, Overview, UserInfo};
use serde_json::json;

use super::{header, result_line, row, separator, unavailable};
use crate::icons::{self, Emoji};
use crate::liquid::Liquid;
use crate::server::{Link, Server};
use crate::tabs::dashboard::{TILE_H, Tile, ago, bytes, tile};
use crate::{theme, widgets};

const GB: u64 = 1024 * 1024 * 1024;

/// Ce qui est affiché à droite.
#[derive(Clone, Default, PartialEq)]
enum Sel {
    #[default]
    Overview,
    App(String),
    NewApp,
    User(u64),
    NewUser,
}

#[derive(Default)]
pub struct State {
    sel: Sel,
    /// Formulaire d'app (inscription, ou modification de l'app choisie).
    form: Option<Form>,
    /// Nouveau compte : identifiant et apps cochées.
    new_login: String,
    new_apps: Vec<String>,
    /// Confirmation d'une action destructrice : ce qu'il faut retaper.
    confirm: Option<Confirm>,
    /// Adresse publique en cours de saisie (`None` : celle du serveur).
    domain: Option<String>,
}

/// Formulaire d'app ; quota en Go pour la saisie.
#[derive(Clone)]
struct Form {
    editing: bool,
    f: AppForm,
    quota_gb: u64,
}

impl Form {
    fn new() -> Self {
        Form { editing: false, f: AppForm { history_days: 30, min_protocol: 1, icon: Emoji::Plugin.code().into(), ..Default::default() }, quota_gb: 20 }
    }

    fn edit(a: &AppInfo) -> Self {
        Form {
            editing: true,
            f: AppForm {
                id: a.id.clone(),
                name: a.name.clone(),
                description: a.description.clone(),
                icon: a.icon.clone(),
                default_quota: a.default_quota,
                history_days: a.history_days,
                min_protocol: a.min_protocol,
            },
            quota_gb: (a.default_quota / GB).max(1),
        }
    }
}

/// Action destructrice en attente de confirmation (et texte retapé).
#[derive(Clone, PartialEq)]
enum Confirm {
    DeleteApp(String, String),
    /// Compte, identifiant, texte retapé.
    DeleteUser(u64, String, String),
    RemoveAccess(u64, String),
}

impl Confirm {
    /// Même action (sans tenir compte du texte retapé).
    fn same(&self, other: &Confirm) -> bool {
        match (self, other) {
            (Confirm::DeleteApp(a, _), Confirm::DeleteApp(b, _)) => a == b,
            (Confirm::DeleteUser(a, ..), Confirm::DeleteUser(b, ..)) => a == b,
            (Confirm::RemoveAccess(a, x), Confirm::RemoveAccess(b, y)) => a == b && x == y,
            _ => false,
        }
    }
}

pub fn show(ui: &mut egui::Ui, st: &mut State, server: &Server, link: &Link, addr: &str) {
    let (overline, title) = (t!("Serveur", "Server"), "Synchro");
    if unavailable(ui, overline, title, link, addr) {
        return;
    }
    server.poll_sync();
    let (data, busy, result, invite) = {
        let d = server.data();
        (d.sync.clone(), d.sync_busy, d.sync_result.clone(), d.sync_invite.clone())
    };
    let ov = match data {
        None => {
            header(ui, overline, title, t!("Chargement…", "Loading…"), false);
            widgets::pane(ui, |ui| {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| ui.spinner());
            });
            return;
        }
        Some(Err(e)) => {
            header(ui, overline, title, t!("Indisponible", "Unavailable"), true);
            widgets::pane(ui, |ui| {
                ui.add_space(40.0);
                let hint = if e.contains("route inconnue") {
                    t!("Agent trop ancien : mets-le à jour (Réglages › Serveur › Rechercher)", "Agent too old: update it (Settings › Server › Check now)").to_string()
                } else {
                    e
                };
                widgets::empty_state(ui, t!("Synchronisation indisponible", "Sync unavailable"), &hint);
            });
            return;
        }
        Some(Ok(o)) => o,
    };

    let mut badge = format!(
        "{} · {} · {}",
        plural(ov.apps.len(), "app", "apps"),
        plural(ov.users.len(), t!("compte", "account"), t!("comptes", "accounts")),
        tf!("{} utilisés", "{} used", bytes(ov.used))
    );
    if !ov.settings.enabled {
        badge = t!("Coupée", "Turned off").to_string();
    } else if !ov.alerts.is_empty() {
        badge.push_str(&format!("  ·  {}", plural(ov.alerts.len(), t!("alerte", "alert"), t!("alertes", "alerts"))));
    }
    if ov.simulated {
        badge.push_str(t!("  ·  dossier de test", "  ·  test folder"));
    }
    header(ui, overline, title, &badge, !ov.alerts.is_empty() || !ov.settings.enabled);

    // Élément choisi disparu (supprimé) : retour à la vue d'ensemble.
    let gone = match &st.sel {
        Sel::App(id) => !ov.apps.iter().any(|a| &a.id == id),
        Sel::User(id) => !ov.users.iter().any(|u| u.id == *id),
        _ => false,
    };
    if gone {
        st.sel = Sel::Overview;
    }
    // Nouveau compte créé : on l'ouvre.
    if let Some(inv) = &invite
        && st.sel == Sel::NewUser
        && ov.users.iter().any(|u| u.id == inv.user_id)
    {
        st.sel = Sel::User(inv.user_id);
    }

    widgets::pane_columns(ui, &[widgets::SIDE_W], |i, ui| {
        if i == 0 {
            sidebar(ui, st, &ov);
            return;
        }
        egui::ScrollArea::vertical().id_salt("sync_detail").auto_shrink([false, false]).show(ui, |ui| {
            if let Some(inv) = &invite {
                invite_card(ui, server, inv, &ov.settings.domain);
                ui.add_space(12.0);
            }
            match st.sel.clone() {
                Sel::Overview => overview(ui, st, server, &ov, busy),
                Sel::App(id) => {
                    if let Some(a) = ov.apps.iter().find(|a| a.id == id) {
                        app_page(ui, st, server, &ov, a, busy);
                    }
                }
                Sel::NewApp => app_form(ui, st, server, busy),
                Sel::User(id) => {
                    if let Some(u) = ov.users.iter().find(|u| u.id == id) {
                        user_page(ui, st, server, &ov, u, busy);
                    }
                }
                Sel::NewUser => new_user(ui, st, server, &ov, busy),
            }
            if let Some((ok, text)) = &result {
                ui.add_space(10.0);
                if result_line(ui, *ok, text) {
                    server.data().sync_result = None;
                }
            }
        });
    });
}

// ───────────────────────────── Colonne de gauche ─────────────────────────────

fn sidebar(ui: &mut egui::Ui, st: &mut State, ov: &Overview) {
    let mut hl = Liquid::begin(ui, "sync_nav");
    widgets::side_label(ui, t!("Vue", "View"));
    let alerts = !ov.alerts.is_empty();
    let tone = if alerts { widgets::Tone::Alert } else { widgets::Tone::Normal };
    let count = alerts.then_some(ov.alerts.len());
    if widgets::nav_row(ui, t!("Vue d'ensemble", "Overview"), count, st.sel == Sel::Overview, tone, &mut hl, None).clicked() {
        st.sel = Sel::Overview;
    }

    ui.add_space(14.0);
    widgets::side_label(ui, t!("Apps", "Apps"));
    for a in &ov.apps {
        let (emoji, active) = (Emoji::from_code(&a.icon), a.active);
        let lead = move |p: &egui::Painter, c: egui::Pos2| {
            let r = egui::Rect::from_center_size(c, egui::vec2(16.0, 16.0));
            icons::paint(p, r, emoji);
            if !active {
                p.rect_filled(r.expand(1.0), 3.0, theme::SURFACE().gamma_multiply(0.7));
            }
        };
        let tone = if a.active { widgets::Tone::Normal } else { widgets::Tone::Off };
        if widgets::nav_row(ui, &a.name, Some(a.users as usize), st.sel == Sel::App(a.id.clone()), tone, &mut hl, Some(&lead)).clicked() {
            st.sel = Sel::App(a.id.clone());
            st.form = None;
            st.confirm = None;
        }
    }
    if widgets::nav_row(ui, t!("+ Inscrire une app", "+ Register an app"), None, st.sel == Sel::NewApp, widgets::Tone::Normal, &mut hl, None).clicked() {
        st.sel = Sel::NewApp;
        st.form = Some(Form::new());
        st.confirm = None;
    }

    ui.add_space(14.0);
    widgets::side_label(ui, t!("Comptes", "Accounts"));
    for u in &ov.users {
        let (pending, active, problem) = (u.pending, u.active, u.failures >= 5);
        let dot = move |p: &egui::Painter, c: egui::Pos2| {
            if problem {
                p.circle_filled(c, 4.0, theme::DANGER());
            } else if !active || pending {
                p.circle_stroke(c, 3.5, egui::Stroke::new(1.3, theme::TEXT_MUTED()));
            } else {
                p.circle_filled(c, 4.0, theme::TEXT());
            }
        };
        let tone = if problem {
            widgets::Tone::Alert
        } else if !u.active {
            widgets::Tone::Off
        } else {
            widgets::Tone::Normal
        };
        if widgets::nav_row(ui, &u.login, Some(u.devices.len()), st.sel == Sel::User(u.id), tone, &mut hl, Some(&dot)).clicked() {
            st.sel = Sel::User(u.id);
            st.confirm = None;
        }
    }
    if widgets::nav_row(ui, t!("+ Nouveau compte", "+ New account"), None, st.sel == Sel::NewUser, widgets::Tone::Normal, &mut hl, None).clicked() {
        st.sel = Sel::NewUser;
        st.new_login.clear();
        st.new_apps = ov.apps.iter().filter(|a| a.builtin).map(|a| a.id.clone()).collect();
        st.confirm = None;
    }
    widgets::nav_end(ui, hl);
}

// ───────────────────────────── Vue d'ensemble ─────────────────────────────

fn overview(ui: &mut egui::Ui, st: &mut State, server: &Server, ov: &Overview, busy: bool) {
    // Tuiles.
    let total = ov.used + ov.disk_free;
    let devices: usize = ov.users.iter().map(|u| u.devices.len()).sum();
    let online = ov.users.iter().flat_map(|u| &u.devices).filter(|d| now_s().saturating_sub(d.last_seen) < 600).count();
    let backup_old = ov.last_backup.is_none_or(|t| now_s().saturating_sub(t) > 2 * 86_400);
    let tiles: [Tile; 4] = [
        (
            "sync_used",
            t!("Espace utilisé", "Space used"),
            bytes(ov.used),
            tf!("{} libres", "{} free", bytes(ov.disk_free)),
            (total > 0).then(|| ov.used as f32 * 100.0 / total as f32),
            false,
        ),
        (
            "sync_apps",
            t!("Apps actives", "Active apps"),
            format!("{} / {}", ov.apps.iter().filter(|a| a.active).count(), ov.apps.len()),
            plural(ov.users.len(), t!("compte", "account"), t!("comptes", "accounts")),
            None,
            false,
        ),
        ("sync_devices", t!("Postes", "Devices"), devices.to_string(), tf!("{} actifs ces 10 min", "{} active in last 10 min", online), None, false),
        (
            "sync_backup",
            t!("Dernière sauvegarde", "Last backup"),
            ov.last_backup.map(|t| ago(t as i64 * 1000)).unwrap_or_else(|| t!("jamais", "never").into()),
            t!("chaque nuit vers 3 h", "every night around 3 am").into(),
            None,
            backup_old && ov.users.iter().any(|u| !u.pending),
        ),
    ];
    let gap = 12.0;
    let w = (ui.available_width() - gap * 3.0) / 4.0;
    let top = ui.cursor().top();
    let left = ui.max_rect().left();
    for (i, (key, label, value, note, gauge, alert)) in tiles.iter().enumerate() {
        let r = egui::Rect::from_min_size(egui::pos2(left + i as f32 * (w + gap), top), egui::vec2(w, TILE_H));
        ui.scope_builder(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min)), |ui| tile(ui, key, label, value, note, *gauge, *alert));
    }
    ui.add_space(gap);

    if !ov.alerts.is_empty() {
        widgets::card(ui, t!("À regarder", "Needs attention"), 0.0, |ui| {
            for a in &ov.alerts {
                ui.label(RichText::new(format!("●  {a}")).color(theme::DANGER()));
            }
        });
        ui.add_space(gap);
    }

    widgets::card(ui, t!("Activité récente", "Recent activity"), 0.0, |ui| {
        if ov.activity.is_empty() {
            widgets::empty_state(ui, t!("Aucune activité depuis le démarrage de l'agent", "No activity since the agent started"), "");
            return;
        }
        egui::Grid::new("sync_activity").num_columns(5).spacing([18.0, 6.0]).striped(false).show(ui, |ui| {
            for a in ov.activity.iter().take(14) {
                let name = ov.apps.iter().find(|x| x.id == a.app).map(|x| x.name.as_str()).unwrap_or(&a.app);
                let (what, alert) = match a.kind {
                    ActivityKind::Push => (tf!("↑ {} envoyé(s)", "↑ {} sent", a.count), false),
                    ActivityKind::Pull => (tf!("↓ {} reçu(s)", "↓ {} received", a.count), false),
                    ActivityKind::Login => (t!("connexion", "sign-in").to_string(), false),
                    ActivityKind::LoginFailed => (t!("échec de connexion", "failed sign-in").to_string(), true),
                    ActivityKind::Conflict => (t!("conflit (fusion par l'app)", "conflict (merged by the app)").to_string(), false),
                };
                ui.label(RichText::new(ago(a.time as i64 * 1000)).color(theme::TEXT_FAINT()));
                ui.label(RichText::new(name).color(theme::TEXT_MUTED()));
                ui.label(&a.login);
                ui.label(RichText::new(&a.device).color(theme::TEXT_MUTED()));
                ui.label(RichText::new(what).color(if alert { theme::DANGER() } else { theme::TEXT() }));
                ui.end_row();
            }
        });
    });
    ui.add_space(gap);

    widgets::card(ui, t!("Réglages de la synchro", "Sync settings"), 0.0, |ui| {
        row(ui, t!("Synchronisation", "Synchronisation"), t!("Coupée : les apps ne peuvent plus se connecter", "Off: apps can no longer connect"), |ui| {
            let mut on = ov.settings.enabled;
            if widgets::segmented(ui, "sync_enabled", &[(false, t!("Coupée", "Off")), (true, t!("Active", "On"))], &mut on) && !busy {
                let msg = if on { t!("Synchronisation activée.", "Sync turned on.") } else { t!("Synchronisation coupée.", "Sync turned off.") };
                server.sync_action("/api/sync/settings", Some(json!({ "enabled": on, "domain": ov.settings.domain })), msg.into());
            }
        });
        separator(ui);
        row(ui, t!("Adresse publique", "Public address"), t!("Donnée aux apps (Caddy fait suivre vers l'agent)", "Given to apps (Caddy forwards to the agent)"), |ui| {
            let domain = st.domain.get_or_insert_with(|| ov.settings.domain.clone());
            let changed = *domain != ov.settings.domain;
            if ui.add_enabled(changed && !busy, egui::Button::new(t!("Enregistrer", "Save"))).clicked() {
                server.sync_action(
                    "/api/sync/settings",
                    Some(json!({ "enabled": ov.settings.enabled, "domain": domain.trim() })),
                    t!("Adresse enregistrée.", "Address saved.").into(),
                );
                st.domain = None;
                return;
            }
            ui.add(egui::TextEdit::singleline(domain).hint_text("sync.exemple.fr").desired_width(220.0).font(egui::TextStyle::Monospace));
        });
        separator(ui);
        row(ui, t!("Données", "Data"), t!("Base des comptes et versions chiffrées", "Accounts database and encrypted versions"), |ui| {
            ui.label(RichText::new(&ov.dir).monospace().color(theme::TEXT_MUTED()));
        });
        separator(ui);
        let last = ov.last_backup.map(date).unwrap_or_else(|| t!("jamais", "never").to_string());
        row(ui, t!("Sauvegarde", "Backup"), &tf!("Dernière : {last} · 7 copies gardées", "Last: {last} · 7 copies kept"), |ui| {
            if ui.add_enabled(!busy, egui::Button::new(t!("Sauvegarder maintenant", "Back up now"))).clicked() {
                server.sync_action("/api/sync/backup", None, t!("Sauvegarde faite.", "Backup done.").into());
            }
        });
    });
}

// ───────────────────────────── Apps ─────────────────────────────

fn app_page(ui: &mut egui::Ui, st: &mut State, server: &Server, ov: &Overview, a: &AppInfo, busy: bool) {
    if st.form.as_ref().is_some_and(|f| f.editing && f.f.id == a.id) {
        app_form(ui, st, server, busy);
        return;
    }
    // Titre : icône, nom, actions.
    let mut edit = false;
    let mut toggle = false;
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::hover());
        icons::paint(ui.painter(), r, Emoji::from_code(&a.icon));
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(&a.name).font(theme::title_font(20.0)).color(theme::TEXT()));
            ui.label(RichText::new(&a.description).color(theme::TEXT_MUTED()));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if a.active { t!("Couper", "Turn off") } else { t!("Activer", "Turn on") };
            toggle = ui.add_enabled(!busy, egui::Button::new(label)).clicked();
            edit = ui.add_enabled(!busy, egui::Button::new(t!("Modifier", "Edit"))).clicked();
        });
    });
    if edit {
        st.form = Some(Form::edit(a));
    }
    if toggle {
        let (verb, msg) = if a.active { ("disable", tf!("{} coupée.", "{} turned off.", a.name)) } else { ("enable", tf!("{} activée.", "{} turned on.", a.name)) };
        server.sync_action(&format!("/api/sync/apps/{}/{verb}", a.id), None, msg);
    }
    ui.add_space(14.0);

    widgets::card(ui, "", 0.0, |ui| {
        row(ui, t!("État", "Status"), t!("Coupée : ses postes reçoivent « app coupée »", "Off: its devices get “app turned off”"), |ui| {
            let (text, color) = if a.active { (t!("Active", "On"), theme::TEXT()) } else { (t!("Coupée", "Off"), theme::DANGER()) };
            ui.label(RichText::new(text).color(color));
        });
        separator(ui);
        row(ui, t!("Identifiant", "Identifier"), t!("Dans l'adresse : /sync/<identifiant>/…", "In the address: /sync/<identifier>/…"), |ui| {
            ui.label(RichText::new(&a.id).monospace().color(theme::TEXT_MUTED()));
        });
        separator(ui);
        row(ui, t!("Espace utilisé", "Space used"), &tf!("{} fichiers, anciennes versions comprises", "{} files, old versions included", a.files), |ui| {
            ui.label(bytes(a.used));
        });
        separator(ui);
        row(ui, t!("Quota par compte", "Quota per account"), t!("Donné aux nouveaux accès", "Given to new accesses"), |ui| {
            ui.label(bytes(a.default_quota));
        });
        separator(ui);
        row(ui, t!("Historique", "History"), t!("Anciennes versions gardées pour revenir en arrière", "Old versions kept to roll back"), |ui| {
            ui.label(tf!("{} jours", "{} days", a.history_days));
        });
        separator(ui);
        row(ui, t!("Version minimale", "Minimum version"), t!("Protocole de synchro exigé des postes", "Sync protocol required from devices"), |ui| {
            ui.label(a.min_protocol.to_string());
        });
        separator(ui);
        row(ui, t!("Inscrite", "Registered"), if a.builtin { t!("D'office, par l'agent", "Built in, by the agent") } else { "" }, |ui| {
            ui.label(date(a.created));
        });
    });
    ui.add_space(12.0);

    widgets::card(ui, t!("Comptes autorisés", "Allowed accounts"), 0.0, |ui| {
        let users: Vec<(&UserInfo, &nas_proto::sync::UserApp)> = ov.users.iter().filter_map(|u| u.apps.iter().find(|x| x.app == a.id).map(|x| (u, x))).collect();
        if users.is_empty() {
            widgets::empty_state(ui, t!("Aucun compte", "No account"), t!("Autorise l'app depuis la fiche d'un compte", "Allow the app from an account page"));
        }
        for (u, x) in users {
            let tone = if !x.active || !u.active { widgets::Tone::Off } else { widgets::Tone::Normal };
            if widgets::list_item(ui, false, tone, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&u.login).color(theme::TEXT()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let last = x.last_sync.map(|t| ago(t as i64 * 1000)).unwrap_or_else(|| t!("jamais synchronisé", "never synced").into());
                        ui.label(RichText::new(last).color(theme::TEXT_FAINT()));
                        ui.label(RichText::new(format!("{} / {}", bytes(x.used), bytes(x.quota))).color(theme::TEXT_MUTED()));
                    });
                });
            })
            .clicked()
            {
                st.sel = Sel::User(u.id);
            }
        }
    });
    ui.add_space(12.0);

    widgets::card(ui, t!("Supprimer", "Delete"), 0.0, |ui| {
        if a.builtin {
            ui.label(
                RichText::new(tf!("{} est inscrite d'office : on peut la couper, pas la supprimer.", "{} is built in: it can be turned off, not deleted.", a.name))
                    .color(theme::TEXT_MUTED()),
            );
            return;
        }
        confirm_zone(
            ui,
            st,
            server,
            busy,
            Confirm::DeleteApp(a.id.clone(), String::new()),
            &tf!("Efface l'app et les données de tous ses comptes. Tape « {} » pour confirmer.", "Deletes the app and the data of all its accounts. Type “{}” to confirm.", a.id),
            t!("Supprimer l'app et ses données", "Delete the app and its data"),
        );
    });
}

fn app_form(ui: &mut egui::Ui, st: &mut State, server: &Server, busy: bool) {
    let Some(form) = st.form.as_mut() else { return };
    let title = if form.editing { tf!("Modifier {}", "Edit {}", form.f.name) } else { t!("Inscrire une app", "Register an app").to_string() };
    widgets::pane_title(ui, &title, |_| {});
    let mut save = false;
    let mut cancel = false;
    widgets::card(ui, "", 0.0, |ui| {
        row(ui, t!("Identifiant", "Identifier"), t!("a-z, 0-9, « - » ; ne change plus ensuite", "a-z, 0-9, “-”; cannot change later"), |ui| {
            if form.editing {
                ui.label(RichText::new(&form.f.id).monospace().color(theme::TEXT_MUTED()));
            } else {
                ui.add(egui::TextEdit::singleline(&mut form.f.id).hint_text("mon-app").desired_width(220.0).font(egui::TextStyle::Monospace));
            }
        });
        separator(ui);
        row(ui, t!("Nom", "Name"), "", |ui| {
            ui.add(egui::TextEdit::singleline(&mut form.f.name).desired_width(220.0));
        });
        separator(ui);
        row(ui, t!("Description", "Description"), "", |ui| {
            ui.add(egui::TextEdit::singleline(&mut form.f.description).desired_width(320.0));
        });
        separator(ui);
        ui.label(t!("Icône", "Icon"));
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for e in icons::ALL {
                let selected = form.f.icon == e.code();
                let (r, resp) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::click());
                if selected {
                    ui.painter().rect_filled(r, 8.0, theme::gray(0x30));
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 8.0, theme::overlay(8));
                }
                icons::paint(ui.painter(), r.shrink(7.0), *e);
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    form.f.icon = e.code().into();
                }
            }
        });
        separator(ui);
        row(ui, t!("Quota par compte", "Quota per account"), t!("Pour les nouveaux accès", "For new accesses"), |ui| {
            ui.add(egui::DragValue::new(&mut form.quota_gb).range(1..=2000).suffix(" Go"));
        });
        separator(ui);
        row(ui, t!("Historique", "History"), t!("Jours de conservation des anciennes versions", "Days old versions are kept"), |ui| {
            ui.add(egui::DragValue::new(&mut form.f.history_days).range(0..=3650).suffix(t!(" j", " d")));
        });
        separator(ui);
        row(ui, t!("Version minimale", "Minimum version"), t!("Monte-la pour obliger les postes à se mettre à jour", "Raise it to force devices to update"), |ui| {
            ui.add(egui::DragValue::new(&mut form.f.min_protocol).range(1..=nas_proto::sync::SYNC_PROTOCOL_VERSION));
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let label = if form.editing { t!("Enregistrer", "Save") } else { t!("Inscrire", "Register") };
            let valid = nas_proto::sync::valid_app_id(&form.f.id) && !form.f.name.trim().is_empty();
            save = ui.add_enabled(valid && !busy, egui::Button::new(label)).clicked();
            cancel = ui.button(t!("Annuler", "Cancel")).clicked();
            if !valid && !form.f.id.is_empty() && !nas_proto::sync::valid_app_id(&form.f.id) {
                ui.label(RichText::new(t!("identifiant invalide", "invalid identifier")).color(theme::DANGER()));
            }
        });
    });
    if save {
        let mut f = form.f.clone();
        f.default_quota = form.quota_gb * GB;
        let body = serde_json::to_value(&f).ok();
        if form.editing {
            server.sync_action(&format!("/api/sync/apps/{}/update", f.id), body, tf!("{} modifiée.", "{} updated.", f.name));
        } else {
            server.sync_action("/api/sync/apps", body, tf!("{} inscrite au répertoire.", "{} added to the directory.", f.name));
            st.sel = Sel::App(f.id.clone());
        }
        st.form = None;
    }
    if cancel {
        if !st.form.as_ref().is_some_and(|f| f.editing) {
            st.sel = Sel::Overview;
        }
        st.form = None;
    }
}

// ───────────────────────────── Comptes ─────────────────────────────

fn user_page(ui: &mut egui::Ui, st: &mut State, server: &Server, ov: &Overview, u: &UserInfo, busy: bool) {
    let mut toggle = false;
    widgets::pane_title(ui, &u.login, |ui| {
        let label = if u.active { t!("Désactiver", "Disable") } else { t!("Réactiver", "Enable") };
        toggle = ui.add_enabled(!busy, egui::Button::new(label)).clicked();
    });
    if toggle {
        let (verb, msg) = if u.active { ("disable", tf!("{} désactivé.", "{} disabled.", u.login)) } else { ("enable", tf!("{} réactivé.", "{} enabled.", u.login)) };
        server.sync_action(&format!("/api/sync/users/{}/{verb}", u.id), None, msg);
    }

    widgets::card(ui, "", 0.0, |ui| {
        row(ui, t!("État", "Status"), "", |ui| {
            let (text, color) = if !u.active {
                (t!("Désactivé", "Disabled").to_string(), theme::DANGER())
            } else if u.pending {
                let until = u.invite_expires.map(date).unwrap_or_default();
                (tf!("En attente du premier login (code valable jusqu'au {until})", "Waiting for first sign-in (code valid until {until})"), theme::TODAY())
            } else {
                (t!("Actif", "Active").to_string(), theme::TEXT())
            };
            ui.label(RichText::new(text).color(color));
        });
        separator(ui);
        row(ui, t!("Dernière connexion", "Last sign-in"), &tf!("Compte créé le {}", "Account created {}", date(u.created)), |ui| {
            ui.label(u.last_seen.map(|t| ago(t as i64 * 1000)).unwrap_or_else(|| t!("jamais", "never").into()));
        });
        separator(ui);
        row(ui, t!("Échecs de connexion", "Failed sign-ins"), t!("Depuis la dernière connexion réussie", "Since the last successful sign-in"), |ui| {
            ui.label(RichText::new(u.failures.to_string()).color(if u.failures >= 5 { theme::DANGER() } else { theme::TEXT() }));
        });
    });
    ui.add_space(12.0);

    // Apps du répertoire : autorisées ou non, quota.
    widgets::card(ui, t!("Apps", "Apps"), 0.0, |ui| {
        for a in &ov.apps {
            let access = u.apps.iter().find(|x| x.app == a.id);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                icons::paint(ui.painter(), r, Emoji::from_code(&a.icon));
                let mut on = access.is_some_and(|x| x.active);
                if ui.add_enabled(!busy, egui::Checkbox::new(&mut on, &a.name)).changed() {
                    if on {
                        let quota = access.map(|x| x.quota);
                        server.sync_action(
                            &format!("/api/sync/users/{}/apps/{}", u.id, a.id),
                            Some(json!({ "active": true, "quota": quota })),
                            tf!("{} autorisée.", "{} allowed.", a.name),
                        );
                    } else {
                        // Suspendu sans effacer : les données restent (retrait complet plus bas).
                        server.sync_action(
                            &format!("/api/sync/users/{}/apps/{}", u.id, a.id),
                            Some(json!({ "active": false, "quota": access.map(|x| x.quota) })),
                            tf!("{} suspendue pour {} (données gardées).", "{} suspended for {} (data kept).", a.name, u.login),
                        );
                    }
                }
                if !a.active {
                    ui.label(RichText::new(t!("(app coupée)", "(app off)")).color(theme::TEXT_FAINT()));
                }
                if let Some(x) = access {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let id = egui::Id::unique(("sync_quota", u.id, &a.id));
                        let mut gb = ui.data(|d| d.get_temp::<u64>(id)).unwrap_or((x.quota / GB).max(1));
                        let changed = gb != (x.quota / GB).max(1);
                        if changed && ui.add_enabled(!busy, egui::Button::new(t!("Appliquer", "Apply"))).clicked() {
                            server.sync_action(
                                &format!("/api/sync/users/{}/apps/{}", u.id, a.id),
                                Some(json!({ "active": x.active, "quota": gb * GB })),
                                tf!("Quota de {} : {} Go.", "{} quota: {} GB.", a.name, gb),
                            );
                            ui.data_mut(|d| d.remove::<u64>(id));
                        } else {
                            if ui.add(egui::DragValue::new(&mut gb).range(1..=2000).suffix(" Go")).changed() {
                                ui.data_mut(|d| d.insert_temp(id, gb));
                            }
                        }
                        let alert = x.quota > 0 && x.used * 10 >= x.quota * 9;
                        ui.label(RichText::new(tf!("{} utilisés · {} fichiers  ·  quota", "{} used · {} files  ·  quota", bytes(x.used), x.files)).color(if alert {
                            theme::DANGER()
                        } else {
                            theme::TEXT_MUTED()
                        }));
                    });
                }
            });
            ui.add_space(4.0);
        }
    });
    ui.add_space(12.0);

    widgets::card(ui, t!("Postes", "Devices"), 0.0, |ui| {
        if u.devices.is_empty() {
            let hint = if u.pending { t!("Le premier login se fait avec le code d'invitation", "The first sign-in uses the invitation code") } else { "" };
            widgets::empty_state(ui, t!("Aucun poste connecté", "No device signed in"), hint);
        }
        for d in &u.devices {
            let app = ov.apps.iter().find(|a| a.id == d.app);
            let mut revoke = false;
            widgets::list_item(ui, false, widgets::Tone::Normal, |ui| {
                ui.horizontal(|ui| {
                    if let Some(a) = app {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                        icons::paint(ui.painter(), r, Emoji::from_code(&a.icon));
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&d.name).color(theme::TEXT()));
                        let detail = tf!("{} · {} · connecté le {}", "{} · {} · signed in {}", app.map(|a| a.name.as_str()).unwrap_or(&d.app), d.ip, date(d.created));
                        ui.label(RichText::new(detail).small().color(theme::TEXT_FAINT()));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        revoke = ui.add_enabled(!busy, egui::Button::new(t!("Révoquer", "Revoke"))).clicked();
                        ui.label(RichText::new(ago(d.last_seen as i64 * 1000)).color(theme::TEXT_MUTED()));
                    });
                });
            });
            if revoke {
                server.sync_action(&format!("/api/sync/devices/{}/revoke", d.id), None, tf!("{} révoqué : il devra se reconnecter.", "{} revoked: it must sign in again.", d.name));
            }
        }
    });
    ui.add_space(12.0);

    widgets::card(ui, t!("Accès", "Access"), 0.0, |ui| {
        row(
            ui,
            t!("Nouveau code d'invitation", "New invitation code"),
            t!(
                "Mot de passe oublié : déconnecte tous les postes. Les données restent, lisibles avec le code de récupération",
                "Forgotten password: signs out every device. Data stays, readable with the recovery code"
            ),
            |ui| {
                if ui.add_enabled(!busy, egui::Button::new(t!("Réinitialiser", "Reset"))).clicked() {
                    server.sync_invite(&format!("/api/sync/users/{}/reset", u.id), None);
                }
            },
        );
        for x in &u.apps {
            separator(ui);
            let name = ov.apps.iter().find(|a| a.id == x.app).map(|a| a.name.clone()).unwrap_or(x.app.clone());
            confirm_zone(
                ui,
                st,
                server,
                busy,
                Confirm::RemoveAccess(u.id, x.app.clone()),
                &tf!("Retirer {name} : efface ses {} sur le serveur (les copies des PC restent).", "Remove {name}: erases its {} on the server (PC copies stay).", bytes(x.used)),
                &tf!("Retirer {name}", "Remove {name}"),
            );
        }
        separator(ui);
        confirm_zone(
            ui,
            st,
            server,
            busy,
            Confirm::DeleteUser(u.id, u.login.clone(), String::new()),
            &tf!("Supprime le compte et toutes ses données. Tape « {} » pour confirmer.", "Deletes the account and all its data. Type “{}” to confirm.", u.login),
            t!("Supprimer le compte", "Delete account"),
        );
    });
}

fn new_user(ui: &mut egui::Ui, st: &mut State, server: &Server, ov: &Overview, busy: bool) {
    widgets::pane_title(ui, t!("Nouveau compte", "New account"), |_| {});
    widgets::card(ui, "", 0.0, |ui| {
        row(ui, t!("Identifiant", "Login"), t!("2 à 32 caractères : a-z, 0-9, « . », « _ », « - »", "2 to 32 characters: a-z, 0-9, “.”, “_”, “-”"), |ui| {
            ui.add(egui::TextEdit::singleline(&mut st.new_login).hint_text("dorian").desired_width(220.0).font(egui::TextStyle::Monospace));
        });
        separator(ui);
        ui.label(t!("Apps autorisées", "Allowed apps"));
        ui.add_space(4.0);
        for a in &ov.apps {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                icons::paint(ui.painter(), r, Emoji::from_code(&a.icon));
                let mut on = st.new_apps.contains(&a.id);
                if ui.checkbox(&mut on, &a.name).changed() {
                    if on {
                        st.new_apps.push(a.id.clone());
                    } else {
                        st.new_apps.retain(|x| x != &a.id);
                    }
                }
                ui.label(RichText::new(tf!("quota {}", "quota {}", bytes(a.default_quota))).color(theme::TEXT_FAINT()));
            });
        }
        ui.add_space(12.0);
        ui.label(
            RichText::new(t!(
                "La console donne un code d'invitation (valable 48 h). Au premier login dans l'app, la personne choisit son mot de passe : il ne passe jamais par le serveur.",
                "The console gives an invitation code (valid 48 h). On first sign-in in the app, the person picks a password: it never goes through the server."
            ))
            .color(theme::TEXT_MUTED()),
        );
        ui.add_space(8.0);
        let login = st.new_login.trim().to_lowercase();
        let valid = nas_proto::sync::valid_login(&login);
        if ui.add_enabled(valid && !busy, egui::Button::new(t!("Créer et obtenir le code", "Create and get the code"))).clicked() {
            server.sync_invite("/api/sync/users", Some(json!({ "login": login, "apps": st.new_apps })));
        }
    });
}

/// Code d'invitation à transmettre (affiché jusqu'à ce qu'on le ferme).
fn invite_card(ui: &mut egui::Ui, server: &Server, inv: &nas_proto::sync::Invitation, domain: &str) {
    widgets::card(ui, &tf!("Code d'invitation pour {}", "Invitation code for {}", inv.login), 0.0, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&inv.code).font(egui::FontId::monospace(26.0)).color(theme::TEXT()));
            ui.add_space(12.0);
            if ui.button(t!("Copier", "Copy")).clicked() {
                ui.ctx().copy_text(inv.code.clone());
            }
        });
        ui.add_space(6.0);
        let server_text = if domain.is_empty() {
            t!("adresse publique à régler dans la Vue d'ensemble", "public address to set in the Overview").to_string()
        } else {
            format!("https://{domain}")
        };
        ui.label(
            RichText::new(tf!(
                "Valable jusqu'au {}. Dans WorkTable : Réglages › Compte NAS, serveur {server_text}, identifiant « {} », puis ce code.",
                "Valid until {}. In WorkTable: Settings › NAS account, server {server_text}, login “{}”, then this code.",
                date(inv.expires),
                inv.login
            ))
            .color(theme::TEXT_MUTED()),
        );
        ui.label(RichText::new(t!("Il ne sera plus affiché ensuite.", "It won't be shown again.")).color(theme::TODAY()));
        ui.add_space(8.0);
        if ui.button(t!("C'est noté", "Got it")).clicked() {
            server.data().sync_invite = None;
        }
    });
}

/// Action destructrice : bouton, puis champ où retaper l'identifiant pour confirmer.
fn confirm_zone(ui: &mut egui::Ui, st: &mut State, server: &Server, busy: bool, what: Confirm, text: &str, button: &str) {
    let open = st.confirm.as_ref().is_some_and(|c| c.same(&what));
    if !open {
        ui.horizontal(|ui| {
            ui.label(RichText::new(text).color(theme::TEXT_MUTED()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(!busy, egui::Button::new(RichText::new(button).color(theme::DANGER()))).clicked() {
                    st.confirm = Some(what.clone());
                }
            });
        });
        return;
    }
    let mut go = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        match st.confirm.as_mut() {
            Some(Confirm::DeleteApp(id, typed)) => {
                ui.add(egui::TextEdit::singleline(typed).hint_text(id.as_str()).desired_width(160.0).font(egui::TextStyle::Monospace));
                go = ui.add_enabled(typed == id && !busy, egui::Button::new(RichText::new(button).color(theme::DANGER()))).clicked();
            }
            Some(Confirm::DeleteUser(_, login, typed)) => {
                ui.add(egui::TextEdit::singleline(typed).hint_text(login.as_str()).desired_width(160.0).font(egui::TextStyle::Monospace));
                go = ui.add_enabled(typed == login && !busy, egui::Button::new(RichText::new(button).color(theme::DANGER()))).clicked();
            }
            Some(Confirm::RemoveAccess(..)) => {
                ui.label(
                    RichText::new(t!("Sûr ? Les données de cette app seront effacées du serveur.", "Sure? This app's data will be erased from the server.")).color(theme::DANGER()),
                );
                go = ui.add_enabled(!busy, egui::Button::new(RichText::new(t!("Oui, retirer", "Yes, remove")).color(theme::DANGER()))).clicked();
            }
            None => {}
        }
        cancel = ui.button(t!("Annuler", "Cancel")).clicked();
    });
    if go {
        match st.confirm.take() {
            Some(Confirm::DeleteApp(id, _)) => server.sync_action(&format!("/api/sync/apps/{id}/delete"), None, tf!("App {id} supprimée.", "App {id} deleted.")),
            Some(Confirm::DeleteUser(id, login, _)) => {
                server.sync_action(&format!("/api/sync/users/{id}/delete"), None, tf!("Compte {login} supprimé.", "Account {login} deleted."))
            }
            Some(Confirm::RemoveAccess(id, app)) => {
                server.sync_action(&format!("/api/sync/users/{id}/apps/{app}/remove"), None, tf!("Accès à {app} retiré.", "Access to {app} removed."))
            }
            None => {}
        }
    }
    if cancel {
        st.confirm = None;
    }
}

/// « 1 compte », « 3 comptes ».
fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n > 1 { many } else { one })
}

fn now_s() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// « 06/11/2026 21:40 », à l'heure locale.
fn date(s: u64) -> String {
    chrono::DateTime::from_timestamp(s as i64, 0).map(|d| d.with_timezone(&chrono::Local).format("%d/%m/%Y %H:%M").to_string()).unwrap_or_default()
}
