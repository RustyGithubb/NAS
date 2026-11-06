//! Mesures du système, relevées une fois par seconde dans un fil à part.
//!
//! L'utilisation du processeur se calcule entre deux relevés : c'est pour ça qu'on mesure en
//! continu plutôt qu'au moment où la console demande.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use nas_proto::{Cpu, Disk, Memory, Status};
use sysinfo::{Components, Disks, System};

const PERIOD: Duration = Duration::from_secs(1);

const VIRTUAL_FS: [&str; 6] = ["tmpfs", "devtmpfs", "overlay", "squashfs", "efivarfs", "ramfs"];

pub fn spawn(out: Arc<Mutex<Status>>) {
    std::thread::spawn(move || {
        let mut sys = System::new();
        let mut disks = Disks::new_with_refreshed_list();
        let mut components = Components::new_with_refreshed_list();
        let mut tick = 0u64;
        loop {
            sys.refresh_cpu_all();
            sys.refresh_memory();
            components.refresh(false);
            // Disques : toutes les 10 s suffisent (leur taille change lentement).
            if tick.is_multiple_of(10) {
                disks.refresh(true);
            }
            let status = read(&sys, &disks, &components);
            if let Ok(mut s) = out.lock() {
                *s = status;
            }
            tick += 1;
            std::thread::sleep(PERIOD);
        }
    });
}

fn read(sys: &System, disks: &Disks, components: &Components) -> Status {
    let cpus = sys.cpus();
    Status {
        protocol: nas_proto::PROTOCOL_VERSION,
        agent_version: env!("CARGO_PKG_VERSION").to_string(),
        hostname: System::host_name().unwrap_or_default(),
        os: System::long_os_version().unwrap_or_default(),
        uptime_s: System::uptime(),
        cpu: Cpu {
            model: cpus.first().map(|c| c.brand().trim().to_string()).unwrap_or_default(),
            usage: sys.global_cpu_usage(),
            per_core: cpus.iter().map(|c| c.cpu_usage()).collect(),
        },
        memory: Memory { used: sys.used_memory(), total: sys.total_memory(), swap_used: sys.used_swap(), swap_total: sys.total_swap() },
        disks: disks
            .list()
            .iter()
            // Systèmes de fichiers virtuels (mémoire, conteneurs, paquets) : pas de vrais disques.
            .filter(|d| d.total_space() > 0 && !VIRTUAL_FS.contains(&d.file_system().to_string_lossy().as_ref()))
            .map(|d| Disk {
                mount: d.mount_point().to_string_lossy().into_owned(),
                fs: d.file_system().to_string_lossy().into_owned(),
                used: d.total_space().saturating_sub(d.available_space()),
                total: d.total_space(),
            })
            .collect(),
        temperature_c: components.list().iter().filter_map(|c| c.temperature()).filter(|t| t.is_finite() && *t > 0.0).reduce(f32::max),
    }
}
