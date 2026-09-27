fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    // These no_main executables use libFuzzer's main from a static archive.
    // MSVC does not discover it automatically; cargo-fuzz normally adds this
    // flag, but CI also builds the targets with plain cargo build.
    // Keep it on this package's bins: global RUSTFLAGS would also demand main
    // from the application's cdylib and break that dependency's link.
    // https://rust-fuzz.github.io/book/cargo-fuzz/windows/dll-fuzzing.html
    println!("cargo:rustc-link-arg-bins=/INCLUDE:main");

    // The static Visual C++ runtime, as the application is linked, and needed
    // since Tauri 2.12 (`tauri-build` 2.7). That links the runtime statically by
    // default in any build of the `tpdf` crate, not only under `tauri build`: it
    // writes a nearly empty `msvcrt.lib` into its OUT_DIR, adds that directory
    // to the library search path, and gives the linker the flags below. The
    // search path reaches every package depending on `tpdf`; the flags reach
    // only `tpdf`'s own binaries. So each target here found the empty
    // `msvcrt.lib` first, the linker ignored it as an invalid library (LNK4003),
    // and nothing supplied the C runtime: 128 unresolved externals, `memcpy` and
    // `__CxxFrameHandler3` among them. Found by CI's windows-2025 leg on the
    // 26.9.21 release commit, and reproduced on a Windows desktop, where these
    // flags made every target link and one run 200 inputs.
    //
    // The list is `tauri-build`'s own (`static_vcruntime.rs`), kept in step by
    // hand: if a Tauri update changes that file, change this one.
    for flag in [
        "/NODEFAULTLIB:libvcruntimed.lib",
        "/NODEFAULTLIB:vcruntime.lib",
        "/NODEFAULTLIB:vcruntimed.lib",
        "/NODEFAULTLIB:libcmtd.lib",
        "/NODEFAULTLIB:msvcrt.lib",
        "/NODEFAULTLIB:msvcrtd.lib",
        "/NODEFAULTLIB:libucrt.lib",
        "/NODEFAULTLIB:libucrtd.lib",
        "/DEFAULTLIB:libcmt.lib",
        "/DEFAULTLIB:libvcruntime.lib",
        "/DEFAULTLIB:ucrt.lib",
    ] {
        println!("cargo:rustc-link-arg-bins={flag}");
    }
}
