//! What a test does when a generated fixture is absent.
//!
//! A runner cannot generate every fixture (`scripts/ci_fixtures.py` says which
//! and why), so a test that needs one prints a `[SKIP]` and returns. That is
//! the documented contract: skipped on a runner, covered locally. The second
//! half had nothing behind it, because test output is captured: a machine
//! missing a fixture passed those tests without running them, and nothing said
//! so. With [`REQUIRE`] set, as `scripts/gates.py` sets it off CI, an absent
//! fixture fails the test instead.

/// The variable that turns a missing fixture into a failure.
pub(crate) const REQUIRE: &str = "TPDF_REQUIRE_FIXTURES";

/// Prints `line`, the test's own `[SKIP]` sentence, and fails the test when
/// [`REQUIRE`] is set. The caller returns or continues after it, as before.
pub(crate) fn missing(line: &str) {
    println!("{line}");
    assert!(
        std::env::var_os(REQUIRE).is_none(),
        "{line}; {REQUIRE} is set, so a missing fixture fails: generate it (BUILD.md, Test fixtures)"
    );
}
