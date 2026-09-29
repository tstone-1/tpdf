//! External, per-process loader observations for the built macOS CLI.
use super::{pdfium_in, root, Command, Path, PathBuf};
use std::collections::BTreeMap;
use std::process::Stdio;

pub(super) fn compile(dir: &Path) -> PathBuf {
    let library = dir.join("image_observer.dylib");
    let output = Command::new("clang")
        .args(["-dynamiclib", "-Wall", "-Wextra", "-Werror"])
        .arg(root().join("src-tauri/tests/cli/image_observer.c"))
        .arg("-o")
        .arg(&library)
        .output()
        .expect("clang is required for the macOS loader check");
    assert!(
        output.status.success(),
        "observer build: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    library
}

pub(super) struct Observed {
    pub code: i32,
    pub stderr: String,
    pid: u32,
    logs: BTreeMap<u32, Vec<String>>,
}

impl Observed {
    pub fn boundary(&self, minimum_workers: usize) -> Result<(), String> {
        let parent = self
            .logs
            .get(&self.pid)
            .ok_or("no log for the launched PID")?;
        // A negative observation needs a complete stream, including shutdown;
        // an unreadable, partial or uninitialized observer can never pass it.
        if !parent.iter().any(|s| s == "READY")
            || parent.last().map(String::as_str) != Some("END")
            || !parent.iter().any(|s| s.contains("libSystem"))
            || !parent.iter().any(|s| s.ends_with("/tpdf-cli"))
            || !parent.iter().any(|s| s.ends_with("/image_observer.dylib"))
        {
            return Err("incomplete loader observation for the launched PID".into());
        }
        if pdfium_in(parent) {
            return Err("the launched PID loaded PDFium".into());
        }
        let workers = self
            .logs
            .iter()
            .filter(|(pid, images)| {
                **pid != self.pid
                    && pdfium_in(images)
                    && images.iter().any(|s| s.ends_with("/tpdf-cli"))
                    && images.iter().any(|s| s == "READY")
            })
            .count();
        if workers < minimum_workers {
            return Err(format!(
                "only {workers} workers loaded PDFium; expected at least {minimum_workers}"
            ));
        }
        Ok(())
    }

    pub fn without_parent_log(&self) -> Self {
        let mut logs = self.logs.clone();
        logs.remove(&self.pid);
        Self {
            code: self.code,
            stderr: self.stderr.clone(),
            pid: self.pid,
            logs,
        }
    }
}

pub(super) fn run(
    library: &Path,
    dir: &Path,
    args: &[&str],
    extra_image: Option<&Path>,
) -> Observed {
    std::fs::create_dir(dir).expect("fresh loader log directory");
    let mut injected = library.as_os_str().to_os_string();
    if let Some(extra) = extra_image {
        injected.push(":");
        injected.push(extra);
    }
    let child = Command::new(env!("CARGO_BIN_EXE_tpdf-cli"))
        .args(args)
        .env("DYLD_INSERT_LIBRARIES", injected)
        .env("TPDF_IMAGE_LOG_DIR", dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the built tool runs with its loader observed");
    let pid = child.id();
    let output = child.wait_with_output().expect("observed tool finishes");
    let logs = std::fs::read_dir(dir)
        .expect("loader logs")
        .map(|entry| {
            let path = entry.expect("log entry").path();
            let pid = path
                .file_stem()
                .and_then(|p| p.to_str())
                .and_then(|p| p.parse().ok())
                .expect("PID log filename");
            let lines = std::fs::read_to_string(path)
                .expect("loader log readable")
                .lines()
                .map(str::to_string)
                .collect();
            (pid, lines)
        })
        .collect();
    Observed {
        code: output.status.code().unwrap_or(-1),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        pid,
        logs,
    }
}
