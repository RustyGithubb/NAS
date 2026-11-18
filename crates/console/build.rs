//! Icône et nom de l'exécutable Windows (visibles dans l'Explorateur et les raccourcis).
//! Utilise le compilateur de ressources du SDK Windows ; s'il est absent, la console se compile
//! quand même (avec l'icône par défaut) et un avertissement l'indique.

fn main() {
    println!("cargo:rerun-if-changed=assets/logo.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/logo.ico");
    res.set("ProductName", "NAS");
    res.set("FileDescription", "NAS");
    if let Err(e) = res.compile() {
        println!("cargo:warning=Icône de l'exécutable non appliquée : {e}");
    }
}
