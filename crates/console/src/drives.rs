//! Disques de ce PC (« Ce PC » des Fichiers) : nom affiché et dossier racine.

/// Disques présents : (« Disque local (C:) », « C:\ »). Peut prendre un moment (lecteur réseau
/// endormi) : à appeler hors de l'interface.
#[cfg(windows)]
pub fn list() -> Vec<(String, String)> {
    (b'C'..=b'Z')
        .map(|c| (c as char, format!("{}:\\", c as char)))
        .filter(|(_, root)| std::path::Path::new(root).exists())
        .map(|(c, root)| (format!("{} ({c}:)", label(&root)), root))
        .collect()
}

#[cfg(not(windows))]
pub fn list() -> Vec<(String, String)> {
    vec![("/".to_string(), "/".to_string())]
}

/// Nom du disque donné dans Windows, sinon son genre (disque local, clé USB…).
#[cfg(windows)]
fn label(root: &str) -> String {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumeInformationW};
    let wide: Vec<u16> = root.encode_utf16().chain([0]).collect();
    let mut name = [0u16; 261];
    // SAFETY : chaînes terminées par 0, tampon de la taille annoncée ; les autres sorties sont facultatives.
    let ok = unsafe {
        GetVolumeInformationW(wide.as_ptr(), name.as_mut_ptr(), name.len() as u32, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), 0)
    };
    if ok != 0 {
        let len = name.iter().position(|&c| c == 0).unwrap_or(0);
        let label = String::from_utf16_lossy(&name[..len]);
        if !label.trim().is_empty() {
            return label;
        }
    }
    // Genres de lecteurs de Windows : 2 amovible, 4 réseau, 5 CD/DVD.
    match unsafe { GetDriveTypeW(wide.as_ptr()) } {
        2 => t!("Clé USB", "USB drive").into(),
        4 => t!("Lecteur réseau", "Network drive").into(),
        5 => t!("Lecteur CD", "CD drive").into(),
        _ => t!("Disque local", "Local disk").into(),
    }
}
