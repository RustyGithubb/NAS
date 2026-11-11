//! Réseau du serveur : interface de sortie, Wi-Fi, adresses, débit en direct, pings, coupures
//! et test de débit internet. Lu sans rien installer : fichiers du noyau (`/proc`, `/sys`) et
//! commandes du système (`nmcli`, `ip`, `ping`, `journalctl`, `curl`).
//!
//! Deux fils de mesure : le débit chaque seconde, le reste toutes les 10 s.

use std::collections::VecDeque;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nas_proto::{Network, Ping, SpeedState, SpeedTest, Wifi};

/// Historique du débit : 5 minutes, une valeur par seconde.
const HISTORY: usize = 300;
/// Pings gardés : 5 minutes, un toutes les 10 s.
const PINGS: usize = 30;
const SLOW: Duration = Duration::from_secs(10);
/// Hôte joint pour mesurer l'accès à internet.
const INTERNET: &str = "1.1.1.1";
/// Fichier téléchargé par le test de débit (100 Mo, serveur d'OVH en France).
const SPEED_URL: &str = "https://proof.ovh.net/files/100Mb.dat";

pub struct NetMonitor {
    state: Arc<Mutex<Network>>,
    simulated: bool,
}

impl NetMonitor {
    pub fn start(simulated: bool) -> Self {
        let state = Arc::new(Mutex::new(Network { simulated, ..Default::default() }));
        if simulated {
            spawn_mock(state.clone());
        } else {
            spawn_traffic(state.clone());
            spawn_details(state.clone());
        }
        NetMonitor { state, simulated }
    }

    pub fn get(&self) -> Network {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Lance un test de débit (sans attendre la fin : il dure jusqu'à une minute et demie).
    pub fn speedtest(&self) -> Result<(), String> {
        {
            let mut s = self.state.lock().map_err(|_| "état illisible".to_string())?;
            if s.speedtest.state == SpeedState::Running {
                return Err("un test est déjà en cours".into());
            }
            s.speedtest = SpeedTest { state: SpeedState::Running, ..s.speedtest.clone() };
        }
        let (state, simulated) = (self.state.clone(), self.simulated);
        std::thread::spawn(move || {
            let r = if simulated {
                std::thread::sleep(Duration::from_secs(3));
                Ok(8_400_000)
            } else {
                download_speed()
            };
            if let Ok(mut s) = state.lock() {
                s.speedtest = match r {
                    Ok(bps) => SpeedTest { state: SpeedState::Done, bytes_per_s: bps, at_ms: now_ms(), message: String::new() },
                    Err(message) => SpeedTest { state: SpeedState::Failed, at_ms: now_ms(), message, ..s.speedtest.clone() },
                };
            }
        });
        Ok(())
    }
}

/// Télécharge le fichier de test sans le garder : `curl` donne le débit moyen.
fn download_speed() -> Result<u64, String> {
    let out = Command::new("curl").args(["-sS", "-o", "/dev/null", "--max-time", "90", "-w", "%{speed_download}", SPEED_URL]).output().map_err(|e| format!("curl : {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().map(|v| v as u64).map_err(|e| e.to_string())
}

// --- Débit, chaque seconde --------------------------------------------------------------------

fn spawn_traffic(state: Arc<Mutex<Network>>) {
    std::thread::spawn(move || {
        let mut history: VecDeque<(u64, u64)> = VecDeque::new();
        let mut prev: Option<(String, u64, u64, Instant)> = None;
        loop {
            if let Some(iface) = default_route().map(|r| r.0) {
                let rx = counter(&iface, "rx_bytes");
                let tx = counter(&iface, "tx_bytes");
                let now = Instant::now();
                let rate = match &prev {
                    Some((i, prx, ptx, t)) if *i == iface => {
                        let secs = now.duration_since(*t).as_secs_f64().max(0.001);
                        ((rx.saturating_sub(*prx) as f64 / secs) as u64, (tx.saturating_sub(*ptx) as f64 / secs) as u64)
                    }
                    _ => (0, 0),
                };
                prev = Some((iface, rx, tx, now));
                history.push_back(rate);
                while history.len() > HISTORY {
                    history.pop_front();
                }
                if let Ok(mut s) = state.lock() {
                    (s.rx_bps, s.tx_bps, s.rx_total, s.tx_total) = (rate.0, rate.1, rx, tx);
                    s.history = history.iter().copied().collect();
                }
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

/// Compteur de l'interface (octets reçus ou envoyés depuis le démarrage).
fn counter(iface: &str, name: &str) -> u64 {
    std::fs::read_to_string(format!("/sys/class/net/{iface}/statistics/{name}")).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0)
}

/// Route par défaut : (interface, passerelle), lue dans la table de routage du noyau.
fn default_route() -> Option<(String, String)> {
    let table = std::fs::read_to_string("/proc/net/route").ok()?;
    table.lines().skip(1).find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        (f.len() > 2 && f[1] == "00000000").then(|| (f[0].to_string(), hex_ip(f[2])))
    })
}

/// Adresse IPv4 écrite en hexadécimal, octets à l'envers (format de `/proc/net/route`).
fn hex_ip(h: &str) -> String {
    let n = u32::from_str_radix(h, 16).unwrap_or(0);
    let b = n.to_le_bytes();
    format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
}

// --- Détails, pings et coupures, toutes les 10 s ----------------------------------------------

fn spawn_details(state: Arc<Mutex<Network>>) {
    std::thread::spawn(move || {
        let (mut gw_pings, mut net_pings) = (VecDeque::new(), VecDeque::new());
        let mut tick = 0u64;
        loop {
            let route = default_route();
            let iface = route.as_ref().map(|r| r.0.clone()).unwrap_or_default();
            let gateway = route.map(|r| r.1).unwrap_or_default();
            // Les deux pings en même temps (chacun peut attendre 2 s).
            let gw = gateway.clone();
            let h = std::thread::spawn(move || if gw.is_empty() { None } else { ping(&gw) });
            let internet = ping(INTERNET);
            let local = h.join().ok().flatten();
            push(&mut gw_pings, local);
            push(&mut net_pings, internet);

            let wireless = !iface.is_empty() && std::path::Path::new(&format!("/sys/class/net/{iface}/wireless")).exists();
            let wifi = if wireless { wifi_info() } else { None };
            let ipv4 = ipv4(&iface);
            let mac = std::fs::read_to_string(format!("/sys/class/net/{iface}/address")).map(|t| t.trim().to_string()).unwrap_or_default();
            // Journal : toutes les 30 s suffisent.
            let journal = tick.is_multiple_of(3).then(|| disconnects(&iface));
            if let Ok(mut s) = state.lock() {
                s.iface = iface;
                s.wireless = wireless;
                s.gateway = gateway;
                s.wifi = wifi;
                s.ipv4 = ipv4;
                s.mac = mac;
                s.dns = dns();
                s.ping_gateway = summary(&gw_pings);
                s.ping_internet = summary(&net_pings);
                if let Some((n, last, since)) = journal {
                    (s.disconnects, s.last_disconnect, s.connected_since_ms) = (n, last, since);
                }
            }
            tick += 1;
            std::thread::sleep(SLOW);
        }
    });
}

fn push(v: &mut VecDeque<Option<f32>>, x: Option<f32>) {
    v.push_back(x);
    while v.len() > PINGS {
        v.pop_front();
    }
}

fn summary(v: &VecDeque<Option<f32>>) -> Ping {
    let ok: Vec<f32> = v.iter().flatten().copied().collect();
    let sent = v.len() as u32;
    Ping {
        last_ms: v.back().copied().flatten(),
        avg_ms: (!ok.is_empty()).then(|| ok.iter().sum::<f32>() / ok.len() as f32),
        loss_pct: if sent == 0 { 0.0 } else { (sent as usize - ok.len()) as f32 * 100.0 / sent as f32 },
        sent,
    }
}

/// Un ping (2 s d'attente au plus) : temps de réponse en ms.
fn ping(host: &str) -> Option<f32> {
    let out = Command::new("ping").args(["-n", "-c", "1", "-W", "2", host]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let t = text.split("time=").nth(1)?;
    t.split_whitespace().next()?.parse().ok()
}

/// Wi-Fi utilisé, d'après NetworkManager (`nmcli`, sans relancer de recherche de réseaux).
fn wifi_info() -> Option<Wifi> {
    let out = Command::new("nmcli").args(["-t", "-f", "IN-USE,SSID,BSSID,CHAN,FREQ,RATE,SIGNAL", "dev", "wifi", "list", "--rescan", "no"]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().map(nmcli_fields).find(|f| f.first().is_some_and(|x| x == "*")).and_then(|f| parse_wifi(&f))
}

fn parse_wifi(f: &[String]) -> Option<Wifi> {
    let num = |s: &String| s.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
    Some(Wifi {
        ssid: f.get(1)?.clone(),
        bssid: f.get(2)?.clone(),
        channel: num(f.get(3)?),
        freq_mhz: num(f.get(4)?),
        rate_mbps: num(f.get(5)?),
        signal: num(f.get(6)?).min(100) as u8,
    })
}

/// Champs d'une ligne `nmcli -t` : séparés par `:`, où `\:` est un deux-points dans un champ.
fn nmcli_fields(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut esc) = (Vec::new(), String::new(), false);
    for c in line.chars() {
        match (esc, c) {
            (true, _) => {
                cur.push(c);
                esc = false;
            }
            (false, '\\') => esc = true,
            (false, ':') => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// Adresse IPv4 de l'interface, avec son masque.
fn ipv4(iface: &str) -> String {
    if iface.is_empty() {
        return String::new();
    }
    let out = Command::new("ip").args(["-4", "-o", "addr", "show", "dev", iface]).output();
    out.ok().and_then(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().skip_while(|w| *w != "inet").nth(1).map(str::to_string)).unwrap_or_default()
}

/// Serveurs DNS utilisés.
fn dns() -> Vec<String> {
    std::fs::read_to_string("/etc/resolv.conf").map(|t| t.lines().filter_map(|l| l.strip_prefix("nameserver")).map(|s| s.trim().to_string()).collect()).unwrap_or_default()
}

/// Coupures du Wi-Fi depuis le démarrage, d'après le journal de NetworkManager et de
/// wpa_supplicant : (nombre, dernière (date, raison), connecté depuis).
fn disconnects(iface: &str) -> (u32, Option<(i64, String)>, Option<i64>) {
    let out = Command::new("journalctl").args(["-b", "-u", "NetworkManager", "-u", "wpa_supplicant", "-o", "short-unix", "--no-pager", "-q"]).output();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    parse_journal(&text, iface)
}

fn parse_journal(text: &str, iface: &str) -> (u32, Option<(i64, String)>, Option<i64>) {
    let dev = format!("device ({iface}): supplicant interface state:");
    let (mut n, mut last, mut since) = (0u32, None::<(i64, String)>, None);
    // Coupure précédente, remise en place si la dernière s'avère volontaire.
    let mut before: Option<(i64, String)> = None;
    // Raison donnée par wpa_supplicant (juste avant ou juste après la ligne de NetworkManager),
    // et départ volontaire du Mac (changement de réglage, reconnexion demandée) ou non.
    let mut reason_at: Option<(i64, String, bool)> = None;
    let near = |a: i64, b: i64| (a - b).abs() <= 5000;
    for line in text.lines() {
        let ms = line.split_whitespace().next().and_then(|t| t.parse::<f64>().ok()).map(|t| (t * 1000.0) as i64).unwrap_or(0);
        if let Some(rest) = line.split(&dev).nth(1) {
            let rest = rest.trim();
            // Décroché après avoir été connecté (pas un essai de connexion qui échoue).
            if rest.ends_with("-> disconnected") && (rest.starts_with("completed") || rest.starts_with("associated") || rest.contains("handshake")) {
                match &reason_at {
                    // Le Mac est parti de lui-même : pas une coupure.
                    Some((t, _, true)) if near(*t, ms) => {}
                    Some((t, w, false)) if near(*t, ms) => {
                        n += 1;
                        before = last.replace((ms, w.clone()));
                    }
                    _ => {
                        n += 1;
                        before = last.replace((ms, "raison inconnue".into()));
                    }
                }
            } else if rest.ends_with("-> completed") {
                since = Some(ms);
            }
        } else if line.contains(&format!("{iface}: CTRL-EVENT-DISCONNECTED"))
            && let Some(code) = line.split("reason=").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|c| c.parse::<u16>().ok())
        {
            let local = line.contains("locally_generated=1");
            // Code 3 venant du Mac : il a quitté le réseau exprès.
            let voluntary = code == 3 && local;
            let why = reason(code, local);
            if last.as_ref().is_some_and(|(t, _)| near(*t, ms)) {
                if voluntary {
                    // Comptée juste avant par erreur : on l'annule.
                    n = n.saturating_sub(1);
                    last = before.take();
                } else if let Some((_, w)) = last.as_mut() {
                    *w = why.clone();
                }
            }
            reason_at = Some((ms, why, voluntary));
        }
    }
    // Reconnecté depuis la dernière coupure ?
    let since = since.filter(|s| last.as_ref().is_none_or(|(l, _)| s >= l));
    (n, last, since)
}

/// Raison d'une déconnexion Wi-Fi (codes de la norme 802.11).
fn reason(code: u16, local: bool) -> String {
    let what = match code {
        1 => "raison non précisée",
        2 => "authentification expirée",
        3 => "a quitté le réseau",
        4 => "inactivité",
        5 => "point d'accès saturé",
        6 | 7 => "trame refusée par le point d'accès",
        8 => "a quitté le point d'accès",
        14 => "erreur de contrôle de sécurité",
        15 => "négociation de sécurité expirée",
        16 => "renouvellement de clé expiré",
        23 => "échec de l'authentification",
        34 => "trop de pertes (signal faible)",
        _ => "autre raison",
    };
    let side = if local { "décidé par le Mac" } else { "décidé par la box" };
    format!("{what} (code {code}, {side})")
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

// --- Simulé (hors du Mac mini) ----------------------------------------------------------------

fn spawn_mock(state: Arc<Mutex<Network>>) {
    std::thread::spawn(move || {
        let start = now_ms();
        let mut history: VecDeque<(u64, u64)> = VecDeque::new();
        let (mut rx_total, mut tx_total) = (8_000_000_000u64, 600_000_000u64);
        let mut t = 0f64;
        loop {
            t += 1.0;
            let wave = |a: f64, p: f64| ((t / p).sin() * 0.5 + 0.5) * a;
            let rate = ((wave(2_500_000.0, 23.0) + wave(400_000.0, 3.0)) as u64, (wave(300_000.0, 17.0) + 20_000.0) as u64);
            rx_total += rate.0;
            tx_total += rate.1;
            history.push_back(rate);
            while history.len() > HISTORY {
                history.pop_front();
            }
            if let Ok(mut s) = state.lock() {
                s.iface = "wlp2s0".into();
                s.wireless = true;
                s.mac = "a8:66:7f:12:34:56".into();
                s.ipv4 = "192.168.1.169/24".into();
                s.gateway = "192.168.1.254".into();
                s.dns = vec!["192.168.1.254".into()];
                s.wifi = Some(Wifi {
                    ssid: "Bbox-EF3836D8".into(),
                    bssid: "A0:2D:13:E7:63:01".into(),
                    channel: 7,
                    freq_mhz: 2442,
                    rate_mbps: 130,
                    signal: (62.0 + wave(8.0, 40.0)) as u8,
                });
                (s.rx_bps, s.tx_bps, s.rx_total, s.tx_total) = (rate.0, rate.1, rx_total, tx_total);
                s.history = history.iter().copied().collect();
                s.ping_gateway = Ping { last_ms: Some(3.0 + wave(4.0, 7.0) as f32), avg_ms: Some(4.6), loss_pct: 0.0, sent: 30 };
                s.ping_internet = Ping { last_ms: Some(14.0 + wave(6.0, 9.0) as f32), avg_ms: Some(16.2), loss_pct: 3.3, sent: 30 };
                s.disconnects = 2;
                s.last_disconnect = Some((start - 40 * 60_000, reason(4, false)));
                s.connected_since_ms = Some(start - 39 * 60_000);
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nmcli_line() {
        let f = nmcli_fields(r"*:Bbox-EF3836D8:A0\:2D\:13\:E7\:63\:01:7:2442 MHz:130 Mbit/s:64");
        let w = parse_wifi(&f).unwrap();
        assert_eq!((w.ssid.as_str(), w.bssid.as_str(), w.channel, w.freq_mhz, w.rate_mbps, w.signal), ("Bbox-EF3836D8", "A0:2D:13:E7:63:01", 7, 2442, 130, 64));
        assert_eq!(w.band(), "2,4 GHz");
    }

    #[test]
    fn route_hex() {
        assert_eq!(hex_ip("FE01A8C0"), "192.168.1.254");
    }

    #[test]
    fn journal() {
        let text = "\
1794402700.97 nas NetworkManager[1]: <info> device (wlp2s0): supplicant interface state: associating -> disconnected
1794402704.95 nas NetworkManager[1]: <info> device (wlp2s0): supplicant interface state: associating -> completed
1794407336.20 nas wpa_supplicant[2]: wlp2s0: CTRL-EVENT-DISCONNECTED bssid=a0:2d:13:e7:63:01 reason=4 locally_generated=1
1794407336.21 nas NetworkManager[1]: <info> device (wlp2s0): supplicant interface state: completed -> disconnected
";
        let (n, last, since) = parse_journal(text, "wlp2s0");
        assert_eq!(n, 1);
        let (at, why) = last.unwrap();
        assert_eq!(at, 1794407336210);
        assert!(why.contains("code 4"), "{why}");
        assert_eq!(since, None);
        // Départ volontaire du Mac (réglage changé) : pas compté.
        let text = "\
1794407336.20 nas wpa_supplicant[2]: wlp2s0: CTRL-EVENT-DISCONNECTED bssid=a0:2d:13:e7:63:01 reason=3 locally_generated=1
1794407336.21 nas NetworkManager[1]: <info> device (wlp2s0): supplicant interface state: completed -> disconnected
1794407340.00 nas NetworkManager[1]: <info> device (wlp2s0): supplicant interface state: associating -> completed
";
        let (n, last, since) = parse_journal(text, "wlp2s0");
        assert_eq!((n, last, since), (0, None, Some(1794407340000)));
    }

    #[test]
    fn pings() {
        let v: VecDeque<Option<f32>> = [Some(2.0), None, Some(4.0), Some(6.0)].into();
        let p = summary(&v);
        assert_eq!((p.last_ms, p.avg_ms, p.loss_pct, p.sent), (Some(6.0), Some(4.0), 25.0, 4));
    }
}
