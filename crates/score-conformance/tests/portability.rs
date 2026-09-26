//! The core crates perform no I/O, so that they build for wasm32 (R-portable).
//! (The wasm build itself runs in CI: .github/workflows/ci.yml.)
#[test]
fn core_crates_do_no_io() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for krate in ["score-core", "score-logic", "score-syntax", "score-chance", "score-engine"] {
        for entry in std::fs::read_dir(root.join(krate).join("src")).unwrap() {
            let p = entry.unwrap().path();
            let src = std::fs::read_to_string(&p).unwrap();
            for bad in ["std::fs", "std::io", "std::net", "std::process", "println!", "eprintln!", "std::env", "std::time"] {
                assert!(!src.contains(bad), "{} uses {bad}", p.display());
            }
        }
    }
}
