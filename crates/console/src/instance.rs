//! Instance unique (Windows, version installée) : relancer la console ramène au premier plan la
//! fenêtre déjà ouverte au lieu d'en ouvrir une deuxième. Après une mise à jour, la nouvelle
//! version attend que l'ancienne se ferme. En développement (`cargo run`), pas de contrôle.

/// Argument passé à la console relancée après une mise à jour : elle attend que l'ancienne se ferme.
pub const AFTER_UPDATE: &str = "--after-update";

/// Au lancement : `false` si une autre instance tourne déjà (elle vient d'être ramenée au premier
/// plan ; celle-ci doit s'arrêter).
pub fn single_instance() -> bool {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        win::single_instance(std::env::args().any(|a| a == AFTER_UPDATE))
    }
    #[cfg(not(all(windows, not(debug_assertions))))]
    true
}

/// Fenêtre principale créée : écoute les demandes de réouverture des instances suivantes.
pub fn start(cc: &eframe::CreationContext<'_>) {
    #[cfg(all(windows, not(debug_assertions)))]
    if let Ok(h) = raw_window_handle::HasWindowHandle::window_handle(cc)
        && let raw_window_handle::RawWindowHandle::Win32(w) = h.as_raw()
    {
        win::listen(cc.egui_ctx.clone(), w.hwnd.get());
    }
    #[cfg(not(all(windows, not(debug_assertions))))]
    let _ = cc;
}

#[cfg(all(windows, not(debug_assertions)))]
mod win {
    use eframe::egui;
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HWND};
    use windows_sys::Win32::System::Threading::{CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent, WaitForSingleObject};
    use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow};

    const MUTEX: &str = "Local\\NAS.Console.Instance";
    const SHOW: &str = "Local\\NAS.Console.Show";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// `false` si une autre instance tourne : on lui demande de se montrer. `after_update` : on
    /// attend plutôt qu'elle se ferme (10 s au plus).
    pub fn single_instance(after_update: bool) -> bool {
        let name = wide(MUTEX);
        for _ in 0..100 {
            // SAFETY : nom terminé par un zéro, valide le temps de l'appel ; le verrou est gardé
            // ouvert jusqu'à la fin du processus.
            unsafe {
                let m = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
                if m.is_null() || GetLastError() != ERROR_ALREADY_EXISTS {
                    return true;
                }
                CloseHandle(m);
            }
            if !after_update {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        // SAFETY : autorisation sans pointeur ; nom valide le temps de l'appel, poignée fermée aussitôt.
        unsafe {
            AllowSetForegroundWindow(ASFW_ANY);
            let e = OpenEventW(EVENT_MODIFY_STATE, 0, wide(SHOW).as_ptr());
            if !e.is_null() {
                SetEvent(e);
                CloseHandle(e);
            }
        }
        false
    }

    /// Fil d'écoute : à chaque demande d'une autre instance, la fenêtre revient au premier plan.
    pub fn listen(ctx: egui::Context, hwnd: isize) {
        let _ = std::thread::Builder::new().name("nas-instance".into()).spawn(move || {
            let name = wide(SHOW);
            // SAFETY : l'événement vit jusqu'à la fin du processus ; `hwnd` est la fenêtre
            // principale de ce processus, qui vit aussi longtemps que lui.
            unsafe {
                let ev = CreateEventW(std::ptr::null(), 0, 0, name.as_ptr());
                if ev.is_null() {
                    return;
                }
                let hwnd = hwnd as HWND;
                while WaitForSingleObject(ev, INFINITE) == 0 {
                    if IsIconic(hwnd) != 0 {
                        ShowWindow(hwnd, SW_RESTORE);
                    }
                    SetForegroundWindow(hwnd);
                    ctx.request_repaint();
                }
            }
        });
    }
}
