fn main() {
    // These no_main executables use libFuzzer's main from a static archive.
    // MSVC does not discover it automatically; cargo-fuzz normally adds this
    // flag, but CI also builds the targets with plain cargo build.
    // Keep it on this package's bins: global RUSTFLAGS would also demand main
    // from the application's cdylib and break that dependency's link.
    // https://rust-fuzz.github.io/book/cargo-fuzz/windows/dll-fuzzing.html
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/INCLUDE:main");
    }
}
