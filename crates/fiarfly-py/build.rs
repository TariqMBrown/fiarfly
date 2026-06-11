// PyO3 extension modules don't link against libpython directly — the
// interpreter provides Python's symbols at import time. `maturin` injects
// the right linker args; this build script lets plain `cargo build` /
// `cargo check` succeed on macOS too.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-undefined");
        println!("cargo:rustc-link-arg=dynamic_lookup");
    }
}
