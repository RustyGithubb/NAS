//! Vrais services, via les commandes `systemctl` (gestion des services) et `journalctl`
//! (lecture des journaux) de systemd.
//!
//! Démarrer ou arrêter un service demande les droits administrateur : l'agent devra tourner
//! en root, ou avoir une autorisation dédiée (à mettre en place à l'installation).

use std::process::Command;

use nas_proto::{Action, LogEntry, Logs, Service, ServiceState};
use serde_json::Value;

pub struct Systemd;

/// Lance une commande et renvoie sa sortie, ou son message d'erreur.
fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd).args(args).output().map_err(|e| format!("{cmd} : {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { format!("{cmd} : code {}", out.status.code().unwrap_or(-1)) } else { err })
    }
}

/// Services qui démarrent avec le système (`systemctl list-unit-files --state=enabled`).
/// En cas d'erreur : liste vide (aucun service arrêté ne sera signalé, plutôt que tous).
fn enabled_units() -> Vec<String> {
    let out = run("systemctl", &["list-unit-files", "--type=service", "--state=enabled", "--no-legend", "--no-pager"]).unwrap_or_default();
    out.lines().filter_map(|l| l.split_whitespace().next()).map(str::to_string).collect()
}

/// Un service est activé si son nom l'est, ou son modèle pour les instances (« getty@tty1.service »
/// → « getty@.service »).
fn is_enabled(enabled: &[String], unit: &str) -> bool {
    let template = unit.split_once('@').map(|(base, rest)| format!("{base}@{}", &rest[rest.find('.').unwrap_or(rest.len())..]));
    enabled.iter().any(|e| e == unit || Some(e) == template.as_ref())
}

impl super::Backend for Systemd {
    fn simulated(&self) -> bool {
        false
    }

    fn services(&self) -> Result<Vec<Service>, String> {
        let out = run("systemctl", &["list-units", "--type=service", "--all", "--output=json", "--no-pager"])?;
        let list: Vec<Value> = serde_json::from_str(&out).map_err(|e| e.to_string())?;
        let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let enabled = enabled_units();
        let mut services: Vec<Service> = list
            .iter()
            .filter(|v| s(v, "load") != "not-found")
            .map(|v| Service {
                name: s(v, "unit"),
                description: s(v, "description"),
                state: match s(v, "active").as_str() {
                    "active" => ServiceState::Active,
                    "failed" => ServiceState::Failed,
                    "activating" | "deactivating" | "reloading" => ServiceState::Changing,
                    _ => ServiceState::Inactive,
                },
                sub: s(v, "sub"),
                enabled: is_enabled(&enabled, &s(v, "unit")),
            })
            .collect();
        services.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(services)
    }

    fn action(&self, unit: &str, action: Action) -> Result<(), String> {
        run("systemctl", &[action.verb(), "--", unit]).map(|_| ())
    }

    fn logs(&self, unit: Option<&str>, after: Option<&str>) -> Result<Logs, String> {
        let limit = nas_proto::LOG_LIMIT.to_string();
        let mut args = vec!["-o", "json", "--no-pager", "-n", &limit];
        // Sans service : tout le journal du système, comme `journalctl -f`.
        if let Some(unit) = unit {
            args.extend(["-u", unit]);
        }
        let cursor_arg;
        if let Some(c) = after {
            cursor_arg = format!("--after-cursor={c}");
            args.push(&cursor_arg);
        }
        let out = run("journalctl", &args)?;
        let mut logs = Logs { entries: Vec::new(), cursor: after.map(str::to_string) };
        for line in out.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            let text = |k: &str| v.get(k).and_then(Value::as_str);
            logs.entries.push(LogEntry {
                time_ms: text("__REALTIME_TIMESTAMP").and_then(|t| t.parse::<i64>().ok()).unwrap_or(0) / 1000,
                priority: text("PRIORITY").and_then(|p| p.parse().ok()).unwrap_or(6),
                message: message(v.get("MESSAGE")),
                source: source(&v),
            });
            if let Some(c) = text("__CURSOR") {
                logs.cursor = Some(c.to_string());
            }
        }
        Ok(logs)
    }
}

/// Le message est du texte, ou une liste d'octets s'il contient des caractères non imprimables.
fn message(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(bytes)) => String::from_utf8_lossy(&bytes.iter().filter_map(|b| b.as_u64().map(|b| b as u8)).collect::<Vec<_>>()).into_owned(),
        _ => String::new(),
    }
}

/// Auteur d'une ligne, comme l'affiche `journalctl` : « sshd[812] », « kernel ».
fn source(v: &Value) -> String {
    let text = |k: &str| v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
    let name = text("SYSLOG_IDENTIFIER").or_else(|| text("_COMM")).or_else(|| text("_SYSTEMD_UNIT")).unwrap_or("?");
    match text("_PID").or_else(|| text("SYSLOG_PID")) {
        Some(pid) => format!("{name}[{pid}]"),
        None => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_bytes() {
        let v: Value = serde_json::from_str("[104, 105]").unwrap();
        assert_eq!(message(Some(&v)), "hi");
        assert_eq!(message(Some(&Value::String("ok".into()))), "ok");
        assert_eq!(message(None), "");
    }

    #[test]
    fn sources() {
        let v: Value = serde_json::from_str(r#"{"SYSLOG_IDENTIFIER":"sshd","_PID":"812"}"#).unwrap();
        assert_eq!(source(&v), "sshd[812]");
        let v: Value = serde_json::from_str(r#"{"SYSLOG_IDENTIFIER":"kernel"}"#).unwrap();
        assert_eq!(source(&v), "kernel");
    }

    #[test]
    fn enabled_and_templates() {
        let list = vec!["ssh.service".to_string(), "getty@.service".to_string()];
        assert!(is_enabled(&list, "ssh.service"));
        assert!(is_enabled(&list, "getty@tty1.service"));
        assert!(!is_enabled(&list, "e2scrub_all.service"));
        assert!(!is_enabled(&list, "user@1000.service"));
    }
}
