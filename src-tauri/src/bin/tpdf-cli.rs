//! The command-line tool: `tpdf sign`, `tpdf verify`, `tpdf identities`.
//!
//! A second executable rather than a mode of the application's, for Windows:
//! the application is a GUI-subsystem program there (`main.rs`), which gets no
//! console and so cannot write to the terminal it was started from, and a
//! console-subsystem program flashes a window when double-clicked. So each
//! platform ships both, and the bundler carries this one beside the other ---
//! `Contents/MacOS/tpdf-cli` in the macOS bundle, which the application's
//! *Install Command-Line Tool...* links to as `/usr/local/bin/tpdf`, and
//! `tpdf-cli.exe` beside `tpdf.exe` on Windows.
//!
//! No `windows_subsystem` attribute, on purpose: this one is a console program.
//! Everything it does is `tpdf_lib::cli`, which is where the reasoning is.

fn main() {
    std::process::exit(tpdf_lib::cli::main());
}
