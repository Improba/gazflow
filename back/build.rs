//! Script de build — link optionnel de la lib C Ipopt.
//!
//! Uniquement actif quand le feature `nlp-ipopt` est activé. Le build par défaut
//! (hôte, sans Ipopt installé) n'est pas impacté.

fn main() {
    if std::env::var("CARGO_FEATURE_NLP_IPOPT").is_err() {
        return;
    }
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_NLP_IPOPT");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");

    // Prefer pkg-config (récupère ipopt + mumps + lapack + blas + gfortran).
    let pc = std::process::Command::new("pkg-config")
        .args(["--libs", "ipopt"])
        .output();
    match pc {
        Ok(o) if o.status.success() => {
            let libs = String::from_utf8_lossy(&o.stdout);
            for token in libs.split_whitespace() {
                if let Some(l) = token.strip_prefix("-l") {
                    println!("cargo:rustc-link-lib=dylib={l}");
                } else if let Some(p) = token.strip_prefix("-L") {
                    println!("cargo:rustc-link-search={p}");
                }
            }
        }
        _ => {
            // Fallback : link direct libipopt (suppose les deps transitive résolues).
            println!("cargo:rustc-link-lib=dylib=ipopt");
        }
    }
}
