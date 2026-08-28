use std::{
    fs,
    path::{Path, PathBuf},
    process::{self, Command},
    sync::atomic::{AtomicUsize, Ordering},
};

static TEMP_DIRECTORY_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TestOutputDir(PathBuf);

impl TestOutputDir {
    fn new() -> Self {
        let number = TEMP_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "rightloom-fx-progress-test-{}-{number}",
                process::id()
            ));
        fs::create_dir_all(&path).expect("temporary output directory should be created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestOutputDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cli_reports_progress_to_stdout() {
    let output_directory = TestOutputDir::new();
    fs::write(
        output_directory.path().join("stain-0002.png"),
        b"existing output",
    )
    .expect("existing output should be created");
    let directory = output_directory.path().to_string_lossy().into_owned();

    let output = Command::new(env!("CARGO_BIN_EXE_rightloom-fx"))
        .args([
            "stain",
            "-r",
            "1x1",
            "-d",
            "0",
            "-a",
            "2",
            "-o",
            directory.as_str(),
        ])
        .output()
        .expect("CLI process should run");

    assert!(
        output.status.success(),
        "CLI process failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("CLI stdout should be UTF-8");
    assert!(stdout.contains(&format!(
        "starting stain: 2 image(s), 1x1, output {directory}, sequence 0003"
    )));
    assert!(stdout.contains("generating stain-0003.png ..."));
    assert!(stdout.contains("done stain-0003.png"));
    assert!(stdout.contains("generating stain-0004.png ..."));
    assert!(stdout.contains("done stain-0004.png"));
    assert!(stdout.contains(&format!(
        "completed stain: 2 image(s) in {directory}; files stain-0003.png through stain-0004.png"
    )));

    assert!(output_directory.path().join("stain-0003.png").is_file());
    assert!(output_directory.path().join("stain-0004.png").is_file());
}
