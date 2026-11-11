//! Glisser des éléments de la console vers l'Explorateur ou le Bureau de Windows.
//!
//! Windows échange ce qui est glissé entre programmes par « OLE » : le programme de départ
//! fournit un objet de données (ici, la liste des fichiers au format `CF_HDROP`) et un objet qui
//! suit la souris ; Windows s'occupe du reste. La liste n'est donnée qu'au moment où
//! l'Explorateur la demande : on attend alors la fin des téléchargements (fichiers du serveur
//! récupérés dans un dossier temporaire dès que le pointeur sort de la fenêtre).
//!
//! Les objets COM sont écrits à la main (table de fonctions + compteur de références), pour
//! rester sur `windows-sys`, déjà utilisé par la console.

use std::cell::Cell;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC, E_NOINTERFACE, E_NOTIMPL, E_OUTOFMEMORY, E_UNEXPECTED, OLE_E_ADVISENOTSUPPORTED, S_OK,
};
use windows_sys::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, STGMEDIUM, TYMED_HGLOBAL};
use windows_sys::Win32::System::Memory::{GHND, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Ole::{CF_HDROP, DROPEFFECT_COPY, DoDragDrop};
use windows_sys::Win32::System::SystemServices::MK_LBUTTON;
use windows_sys::Win32::UI::Shell::{DROPFILES, SHCreateStdEnumFmtEtc};
use windows_sys::core::{GUID, HRESULT};

const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
const IID_IDATAOBJECT: GUID = GUID::from_u128(0x0000010e_0000_0000_c000_000000000046);
const IID_IDROPSOURCE: GUID = GUID::from_u128(0x00000121_0000_0000_c000_000000000046);
/// Seul sens accepté par `EnumFormatEtc` : lire les données.
const DATADIR_GET: u32 = 1;

/// Glisse `paths` hors de la console jusqu'au relâchement de la souris. `ready` est appelé
/// quand l'Explorateur demande les fichiers : il attend qu'ils soient prêts et renvoie `false`
/// s'ils ne le seront pas (téléchargement en échec). Renvoie `true` si les fichiers ont été déposés.
pub fn drag_out(paths: Vec<PathBuf>, ready: impl Fn() -> bool + 'static) -> bool {
    let data = Box::into_raw(Box::new(DataObject { vtbl: &DATA_VTBL, refs: AtomicU32::new(1), paths, ready: Box::new(ready), state: Cell::new(None) }));
    let source = Box::into_raw(Box::new(DropSource { vtbl: &SOURCE_VTBL, refs: AtomicU32::new(1) }));
    let mut effect = 0;
    // SAFETY : les deux objets restent valides jusqu'à leur dernier `Release` (le nôtre, ci-dessous).
    let hr = unsafe { DoDragDrop(data.cast(), source.cast(), DROPEFFECT_COPY, &mut effect) };
    unsafe {
        data_release(data.cast());
        source_release(source.cast());
    }
    hr == DRAGDROP_S_DROP && effect != 0
}

// --- Objet de données ----------------------------------------------------------------------

#[repr(C)]
struct DataObject {
    vtbl: *const DataVtbl,
    refs: AtomicU32,
    paths: Vec<PathBuf>,
    ready: Box<dyn Fn() -> bool>,
    /// Réponse de `ready`, gardée (l'Explorateur peut redemander les données).
    state: Cell<Option<bool>>,
}

#[repr(C)]
struct DataVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_data: unsafe extern "system" fn(*mut c_void, *const FORMATETC, *mut STGMEDIUM) -> HRESULT,
    get_data_here: unsafe extern "system" fn(*mut c_void, *const FORMATETC, *mut STGMEDIUM) -> HRESULT,
    query_get_data: unsafe extern "system" fn(*mut c_void, *const FORMATETC) -> HRESULT,
    get_canonical_format_etc: unsafe extern "system" fn(*mut c_void, *const FORMATETC, *mut FORMATETC) -> HRESULT,
    set_data: unsafe extern "system" fn(*mut c_void, *const FORMATETC, *const STGMEDIUM, i32) -> HRESULT,
    enum_format_etc: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> HRESULT,
    d_advise: unsafe extern "system" fn(*mut c_void, *const FORMATETC, u32, *mut c_void, *mut u32) -> HRESULT,
    d_unadvise: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    enum_d_advise: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}

static DATA_VTBL: DataVtbl = DataVtbl {
    query_interface: data_query_interface,
    add_ref: data_add_ref,
    release: data_release,
    get_data,
    get_data_here,
    query_get_data,
    get_canonical_format_etc,
    set_data,
    enum_format_etc,
    d_advise,
    d_unadvise,
    enum_d_advise,
};

/// Le seul format proposé : une liste de fichiers en mémoire globale.
fn hdrop_format() -> FORMATETC {
    FORMATETC { cfFormat: CF_HDROP, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT, lindex: -1, tymed: TYMED_HGLOBAL as u32 }
}

unsafe extern "system" fn data_query_interface(this: *mut c_void, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    unsafe { query(this, riid, out, IID_IDATAOBJECT, data_add_ref) }
}

unsafe extern "system" fn data_add_ref(this: *mut c_void) -> u32 {
    unsafe { (*this.cast::<DataObject>()).refs.fetch_add(1, Ordering::AcqRel) + 1 }
}

unsafe extern "system" fn data_release(this: *mut c_void) -> u32 {
    let left = unsafe { (*this.cast::<DataObject>()).refs.fetch_sub(1, Ordering::AcqRel) - 1 };
    if left == 0 {
        drop(unsafe { Box::from_raw(this.cast::<DataObject>()) });
    }
    left
}

unsafe extern "system" fn get_data(this: *mut c_void, fmt: *const FORMATETC, medium: *mut STGMEDIUM) -> HRESULT {
    unsafe {
        if query_get_data(this, fmt) != S_OK {
            return DV_E_FORMATETC;
        }
        let obj = &*this.cast::<DataObject>();
        let ok = match obj.state.get() {
            Some(ok) => ok,
            None => {
                let ok = (obj.ready)();
                obj.state.set(Some(ok));
                ok
            }
        };
        if !ok {
            return E_UNEXPECTED;
        }
        let h = hdrop(&obj.paths);
        if h.is_null() {
            return E_OUTOFMEMORY;
        }
        (*medium).tymed = TYMED_HGLOBAL as u32;
        (*medium).u.hGlobal = h;
        (*medium).pUnkForRelease = std::ptr::null_mut();
        S_OK
    }
}

/// Bloc mémoire `DROPFILES` suivi des chemins en UTF-16, chacun terminé par 0, puis un 0 final.
unsafe fn hdrop(paths: &[PathBuf]) -> *mut c_void {
    let mut wide: Vec<u16> = Vec::new();
    for p in paths {
        wide.extend(p.to_string_lossy().encode_utf16());
        wide.push(0);
    }
    wide.push(0);
    let head = std::mem::size_of::<DROPFILES>();
    unsafe {
        let h = GlobalAlloc(GHND, head + wide.len() * 2);
        if h.is_null() {
            return h;
        }
        let ptr = GlobalLock(h).cast::<u8>();
        if ptr.is_null() {
            return std::ptr::null_mut();
        }
        let df = ptr.cast::<DROPFILES>();
        (*df).pFiles = head as u32;
        (*df).fWide = 1;
        std::ptr::copy_nonoverlapping(wide.as_ptr().cast::<u8>(), ptr.add(head), wide.len() * 2);
        GlobalUnlock(h);
        h
    }
}

unsafe extern "system" fn get_data_here(_: *mut c_void, _: *const FORMATETC, _: *mut STGMEDIUM) -> HRESULT {
    E_NOTIMPL
}

unsafe extern "system" fn query_get_data(_: *mut c_void, fmt: *const FORMATETC) -> HRESULT {
    let Some(f) = (unsafe { fmt.as_ref() }) else { return DV_E_FORMATETC };
    if f.cfFormat == CF_HDROP && f.tymed & TYMED_HGLOBAL as u32 != 0 && f.dwAspect == DVASPECT_CONTENT { S_OK } else { DV_E_FORMATETC }
}

unsafe extern "system" fn get_canonical_format_etc(_: *mut c_void, _: *const FORMATETC, out: *mut FORMATETC) -> HRESULT {
    if let Some(o) = unsafe { out.as_mut() } {
        o.ptd = std::ptr::null_mut();
    }
    E_NOTIMPL
}

unsafe extern "system" fn set_data(_: *mut c_void, _: *const FORMATETC, _: *const STGMEDIUM, _: i32) -> HRESULT {
    E_NOTIMPL
}

unsafe extern "system" fn enum_format_etc(_: *mut c_void, direction: u32, out: *mut *mut c_void) -> HRESULT {
    if direction != DATADIR_GET {
        return E_NOTIMPL;
    }
    let f = hdrop_format();
    unsafe { SHCreateStdEnumFmtEtc(1, &f, out) }
}

unsafe extern "system" fn d_advise(_: *mut c_void, _: *const FORMATETC, _: u32, _: *mut c_void, _: *mut u32) -> HRESULT {
    OLE_E_ADVISENOTSUPPORTED
}

unsafe extern "system" fn d_unadvise(_: *mut c_void, _: u32) -> HRESULT {
    OLE_E_ADVISENOTSUPPORTED
}

unsafe extern "system" fn enum_d_advise(_: *mut c_void, _: *mut *mut c_void) -> HRESULT {
    OLE_E_ADVISENOTSUPPORTED
}

// --- Source du glisser (suit la souris) -------------------------------------------------------

#[repr(C)]
struct DropSource {
    vtbl: *const SourceVtbl,
    refs: AtomicU32,
}

#[repr(C)]
struct SourceVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    query_continue_drag: unsafe extern "system" fn(*mut c_void, i32, u32) -> HRESULT,
    give_feedback: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
}

static SOURCE_VTBL: SourceVtbl = SourceVtbl { query_interface: source_query_interface, add_ref: source_add_ref, release: source_release, query_continue_drag, give_feedback };

unsafe extern "system" fn source_query_interface(this: *mut c_void, riid: *const GUID, out: *mut *mut c_void) -> HRESULT {
    unsafe { query(this, riid, out, IID_IDROPSOURCE, source_add_ref) }
}

unsafe extern "system" fn source_add_ref(this: *mut c_void) -> u32 {
    unsafe { (*this.cast::<DropSource>()).refs.fetch_add(1, Ordering::AcqRel) + 1 }
}

unsafe extern "system" fn source_release(this: *mut c_void) -> u32 {
    let left = unsafe { (*this.cast::<DropSource>()).refs.fetch_sub(1, Ordering::AcqRel) - 1 };
    if left == 0 {
        drop(unsafe { Box::from_raw(this.cast::<DropSource>()) });
    }
    left
}

/// Échap : annulé ; bouton gauche relâché : déposé ; sinon on continue.
unsafe extern "system" fn query_continue_drag(_: *mut c_void, escape: i32, keys: u32) -> HRESULT {
    if escape != 0 {
        DRAGDROP_S_CANCEL
    } else if keys & MK_LBUTTON == 0 {
        DRAGDROP_S_DROP
    } else {
        S_OK
    }
}

/// Curseurs habituels de Windows (copier, interdit…).
unsafe extern "system" fn give_feedback(_: *mut c_void, _: u32) -> HRESULT {
    DRAGDROP_S_USEDEFAULTCURSORS
}

/// `QueryInterface` commun : l'objet ne répond qu'à IUnknown et à son interface.
unsafe fn query(this: *mut c_void, riid: *const GUID, out: *mut *mut c_void, iid: GUID, add_ref: unsafe extern "system" fn(*mut c_void) -> u32) -> HRESULT {
    unsafe {
        if out.is_null() {
            return E_NOINTERFACE;
        }
        let r = &*riid;
        if same(r, &IID_IUNKNOWN) || same(r, &iid) {
            *out = this;
            add_ref(this);
            S_OK
        } else {
            *out = std::ptr::null_mut();
            E_NOINTERFACE
        }
    }
}

fn same(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}
