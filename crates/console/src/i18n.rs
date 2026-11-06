//! Langue de l'interface : français ou anglais (Réglages). Repris de WorkTable.
//!
//! Chaque texte est écrit dans les deux langues là où il est affiché :
//! - `t!("Réglages", "Settings")` donne un `&'static str` ;
//! - `tf!("{n} non lus", "{n} unread")` donne une `String` (mêmes règles que `format!`).
//!
//! Le changement de langue s'applique à l'image suivante (interface immédiate).

use serde::{Deserialize, Serialize};
#[cfg_attr(test, allow(unused_imports))]
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

/// Langue choisie dans les réglages.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum Language {
    /// Celle du système (français si Windows est en français, anglais sinon).
    #[default]
    System,
    French,
    English,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::System, Language::French, Language::English];

    /// Nom affiché dans le sélecteur (chaque langue dans sa propre langue).
    pub fn label(self) -> &'static str {
        match self {
            Language::System => crate::t!("Langue du système", "System language"),
            Language::French => "Français",
            Language::English => "English",
        }
    }
}

#[cfg_attr(test, allow(dead_code))]
static ENGLISH: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
thread_local! {
    /// Tests : langue propre à chaque fil (les tests tournent en parallèle, en français par défaut).
    static TEST_ENGLISH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Applique la langue choisie.
pub fn set(lang: Language) {
    let en = match lang {
        Language::System => !system_is_french(),
        Language::French => false,
        Language::English => true,
    };
    #[cfg(test)]
    TEST_ENGLISH.with(|c| c.set(en));
    #[cfg(not(test))]
    ENGLISH.store(en, Relaxed);
}

/// L'interface est-elle en anglais ?
#[inline]
pub fn en() -> bool {
    #[cfg(test)]
    return TEST_ENGLISH.with(|c| c.get());
    #[cfg(not(test))]
    ENGLISH.load(Relaxed)
}

/// Langue d'affichage de Windows (ou variables `LANG`/`LC_ALL` ailleurs).
#[cfg(windows)]
fn system_is_french() -> bool {
    // SAFETY : simple lecture d'un réglage du système, sans paramètre.
    let id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
    // Langue principale (bits 0 à 9) : 0x0C = français (France, Belgique, Canada, Suisse…).
    id & 0x3ff == 0x0c
}

#[cfg(not(windows))]
fn system_is_french() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())).is_some_and(|v| v.starts_with("fr"))
}

/// Texte fixe dans la langue de l'interface : `t!("français", "english")`.
#[macro_export]
macro_rules! t {
    ($fr:expr, $en:expr $(,)?) => {
        if $crate::i18n::en() { $en } else { $fr }
    };
}

/// Texte formaté dans la langue de l'interface : `tf!("{n} non lus", "{n} unread")`.
#[macro_export]
macro_rules! tf {
    ($fr:literal, $en:literal $(, $($arg:tt)*)?) => {
        if $crate::i18n::en() { format!($en $(, $($arg)*)?) } else { format!($fr $(, $($arg)*)?) }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switches_language() {
        let n = 3;
        set(Language::English);
        assert_eq!(t!("Réglages", "Settings"), "Settings");
        assert_eq!(tf!("{n} non lus", "{n} unread"), "3 unread");
        set(Language::French);
        assert_eq!(t!("Réglages", "Settings"), "Réglages");
        assert_eq!(tf!("{} non lus", "{} unread", n), "3 non lus");
    }
}
