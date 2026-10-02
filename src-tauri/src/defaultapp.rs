//! *Make tpdf the default PDF app*: the one place tpdf touches which
//! application opens a PDF, and only when the reader chooses the command.
//!
//! **tpdf never asks.** There is no check at start and no reminder: the
//! bundle declares that it can view PDFs, which puts it under "Open With", and
//! whether it is the default is the reader's setting. This command is the
//! shortcut for a reader who wants it, and the answer is read back from the
//! system after the change, as `clitool.rs` reads the filesystem back.
//!
//! On macOS that is `NSWorkspace`, which shows the system's own confirmation.
//! Windows does not let an application make itself the default --- the choice
//! is protected by a per-user hash only Settings writes --- so there the
//! command opens Settings at Default apps and says what to choose.

/// What to tell the reader, from which application the system names for PDFs
/// after the change and which one this is. Both are paths to an application.
///
/// # Errors
///
/// The system still names another application, or none.
pub fn said(handler: Option<&str>, ours: &str, already: bool) -> Result<String, String> {
    let same = |handler: &str| {
        handler
            .trim_end_matches('/')
            .eq_ignore_ascii_case(ours.trim_end_matches('/'))
    };
    match handler {
        Some(handler) if same(handler) && already => Ok("tpdf already opens PDF documents.".into()),
        Some(handler) if same(handler) => Ok(
            "tpdf now opens PDF documents. To change that, select a PDF in Finder, choose \
             Get Info, and pick another application under \"Open with\"."
                .into(),
        ),
        Some(handler) => Err(format!(
            "tpdf is not the default: PDF documents still open with {handler}"
        )),
        None => {
            Err("tpdf is not the default: the system names no application for PDF documents".into())
        }
    }
}

/// The `.app` this executable runs inside, if it runs inside one.
#[cfg(target_os = "macos")]
fn bundle_of(executable: &std::path::Path) -> Option<&std::path::Path> {
    executable
        .ancestors()
        .find(|at| at.extension().is_some_and(|extension| extension == "app"))
}

/// Makes this copy of tpdf the default application for PDF documents.
///
/// `NSWorkspace`, and the system shows its own confirmation. Measured on
/// macOS 27 before choosing: Launch Services' older
/// `LSSetDefaultRoleHandlerForContentType` returns 0 and changes nothing, for
/// an installed application as for one that does not exist, so the answer here
/// is what the system names afterwards and never the call's own result.
///
/// The content type is given as a file, an empty one written for the call:
/// the variant taking a type directly needs a crate this tree does not have.
///
/// # Errors
///
/// tpdf is not running from an application bundle, the reader declined the
/// system's question, or the system names another application afterwards.
#[cfg(target_os = "macos")]
pub fn apply(_identifier: &str) -> Result<String, String> {
    let executable = std::env::current_exe().map_err(|e| format!("where tpdf runs from: {e}"))?;
    let bundle = bundle_of(&executable)
        .ok_or("this copy of tpdf is not an application: open tpdf from the Applications folder")?
        .to_string_lossy()
        .into_owned();
    apply_for(&bundle)
}

/// [`apply`], for the application at `bundle`.
#[cfg(target_os = "macos")]
fn apply_for(bundle: &str) -> Result<String, String> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSError, NSString, NSURL};

    let bundle = bundle.to_string();
    let sample = std::env::temp_dir().join(format!("tpdf-default-{}.pdf", std::process::id()));
    std::fs::write(&sample, b"%PDF-1.4\n").map_err(|e| format!("a sample document: {e}"))?;

    let workspace = NSWorkspace::sharedWorkspace();
    let sample_url = NSURL::fileURLWithPath(&NSString::from_str(&sample.to_string_lossy()));
    let current = || {
        workspace
            .URLForApplicationToOpenURL(&sample_url)
            .and_then(|url| url.path())
            .map(|path| path.to_string())
    };
    let outcome = (|| {
        if let Ok(done) = said(current().as_deref(), &bundle, true) {
            return Ok(done);
        }
        let (tell, told) = std::sync::mpsc::channel::<Option<String>>();
        let handler = block2::RcBlock::new(move |error: *mut NSError| {
            // SAFETY: the system passes nil or a valid `NSError` that lives for
            // the duration of this call, which is all it is read for.
            let why =
                unsafe { error.as_ref() }.map(|error| error.localizedDescription().to_string());
            let _ = tell.send(why);
        });
        let application = NSURL::fileURLWithPath(&NSString::from_str(&bundle));
        workspace.setDefaultApplicationAtURL_toOpenContentTypeOfFileAtURL_completionHandler(
            &application,
            &sample_url,
            Some(&handler),
        );
        // The system's question is open while this waits; a reader may take a while.
        match told.recv_timeout(std::time::Duration::from_secs(120)) {
            Ok(None) => said(current().as_deref(), &bundle, false),
            // Declining arrives as an error too; either way the answer is the
            // application the system names now.
            Ok(Some(_)) | Err(_) => said(current().as_deref(), &bundle, false)
                .map_err(|still| format!("{still}. The system's question was not confirmed.")),
        }
    })();
    let _ = std::fs::remove_file(&sample);
    outcome
}

/// Opens Settings at Default apps, where the reader makes the choice.
///
/// # Errors
///
/// Settings did not open.
#[cfg(windows)]
pub fn apply(_identifier: &str) -> Result<String, String> {
    crate::opener::open_settings("ms-settings:defaultapps").map_err(|_| {
        "Settings did not open. Choose tpdf for .pdf under Apps, Default apps.".to_string()
    })?;
    Ok(
        "Windows lets only you choose the default. Settings is open at Default apps: \
        choose tpdf, then set it for .pdf."
            .into(),
    )
}

/// No default-application setting this build knows how to reach.
///
/// # Errors
///
/// Always.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn apply(_identifier: &str) -> Result<String, String> {
    Err("making tpdf the default is available on macOS and Windows".into())
}

#[cfg(test)]
mod tests {
    use super::said;

    #[test]
    fn the_sentence_is_read_from_what_the_system_names_afterwards() {
        let ours = "/Applications/tpdf.app";
        assert!(said(Some(ours), ours, false)
            .expect("set")
            .starts_with("tpdf now opens"));
        // The system answers with or without the trailing slash of a directory.
        assert!(said(Some("/Applications/tpdf.app/"), ours, false).is_ok());
        assert_eq!(
            said(Some(ours), ours, true).expect("already"),
            "tpdf already opens PDF documents."
        );
        let other =
            said(Some("/System/Applications/Preview.app"), ours, false).expect_err("another");
        assert!(other.contains("Preview.app"), "{other}");
        assert!(said(None, ours, false).is_err());
        // A path that only starts the same is another application.
        assert!(said(Some("/Applications/tpdf.app.old/x.app"), ours, false).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_bundle_is_the_app_folder_above_the_executable() {
        use std::path::Path;
        let inside = Path::new("/Applications/tpdf.app/Contents/MacOS/tpdf");
        assert_eq!(
            super::bundle_of(inside),
            Some(Path::new("/Applications/tpdf.app"))
        );
        assert_eq!(
            super::bundle_of(Path::new("/usr/local/target/debug/tpdf")),
            None
        );
    }

    /// Asks the real system, as the installed application. Changes nothing when
    /// tpdf is already the default; otherwise the system asks the person at the
    /// machine. Run by hand: `cargo test --lib defaultapp -- --ignored --nocapture`.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "asks the system which application opens PDFs, and may change it"]
    fn the_system_answers_for_the_installed_application() {
        println!("{:?}", super::apply_for("/Applications/tpdf.app"));
    }
}
