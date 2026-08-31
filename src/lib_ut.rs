use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicUsize, Ordering},
};

use image::GenericImageView;

use super::{cli::Cli, run_cli};

static TEMP_DIRECTORY_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TestOutputDir(PathBuf);

impl TestOutputDir {
    fn new() -> Self {
        let number = TEMP_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!("rightloom-fx-test-{}-{number}", process::id()));
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

fn run_from<I, T>(arguments: I) -> Result<(), Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = Cli::try_parse_from(arguments)?;
    run_cli(cli).map_err(Into::into)
}

#[test]
fn scratches_command_writes_numbered_pngs() {
    let output = TestOutputDir::new();
    let output_argument = output.path().to_string_lossy().into_owned();

    run_from([
        "rightloom-fx",
        "scratches",
        "-r",
        "320x200",
        "-d",
        "10",
        "-t",
        "dust",
        "-a",
        "2",
        "-o",
        output_argument.as_str(),
    ])
    .expect("command should generate images");

    for number in 1..=2 {
        let path = output.path().join(format!("scratches-{number:04}.png"));
        assert!(path.is_file(), "{} should exist", path.display());

        let image = image::open(&path).expect("output should be a readable PNG");
        assert_eq!(image.dimensions(), (320, 200));

        let rgba = image.to_rgba8();
        assert!(rgba.pixels().all(|pixel| pixel[3] == 255));
    }
}

#[test]
fn stain_command_writes_requested_number_of_pngs() {
    let output = TestOutputDir::new();
    let output_argument = output.path().to_string_lossy().into_owned();

    run_from([
        "rightloom-fx",
        "stain",
        "-r",
        "320x200",
        "-d",
        "10",
        "-b",
        "75",
        "-l",
        "25",
        "-a",
        "3",
        "-o",
        output_argument.as_str(),
    ])
    .expect("command should generate images");

    for number in 1..=3 {
        let path = output.path().join(format!("stain-{number:04}.png"));
        assert!(path.is_file(), "{} should exist", path.display());

        let image = image::open(&path).expect("output should be a readable PNG");
        assert_eq!(image.dimensions(), (320, 200));
        let rgba = image.to_rgba8();
        assert!(rgba.pixels().all(|pixel| pixel[3] == 255));
    }
}

#[test]
fn stain_command_with_alpha_preserves_transparency() {
    let output = TestOutputDir::new();
    let output_argument = output.path().to_string_lossy().into_owned();

    run_from([
        "rightloom-fx",
        "stain",
        "-r",
        "320x200",
        "-d",
        "10",
        "--alpha",
        "-a",
        "1",
        "-o",
        output_argument.as_str(),
    ])
    .expect("command should generate images");

    let image = image::open(output.path().join("stain-0001.png"))
        .expect("output should be a readable PNG")
        .to_rgba8();
    assert_eq!(image.dimensions(), (320, 200));
    assert!(image.pixels().any(|pixel| pixel[3] == 0));
    assert!(image.pixels().any(|pixel| pixel[3] > 0));
}

#[test]
fn bokeh_command_writes_numbered_pngs_through_shared_output_handling() {
    let output = TestOutputDir::new();
    let output_argument = output.path().to_string_lossy().into_owned();

    run_from([
        "rightloom-fx",
        "bokeh",
        "-r",
        "320x200",
        "-t",
        "twinkle",
        "-t",
        "edge",
        "-d",
        "50",
        "-s",
        "70",
        "-u",
        "25",
        "-a",
        "2",
        "-o",
        output_argument.as_str(),
    ])
    .expect("command should generate images");

    for number in 1..=2 {
        let path = output.path().join(format!("bokeh-{number:04}.png"));
        assert!(path.is_file(), "{} should exist", path.display());
        let image = image::open(&path).expect("output should be a readable PNG");
        assert_eq!(image.dimensions(), (320, 200));
        assert!(image.to_rgba8().pixels().all(|pixel| pixel[3] == 255));
    }
}

#[test]
fn burn_command_writes_numbered_pngs_through_shared_output_handling() {
    let output = TestOutputDir::new();
    let output_argument = output.path().to_string_lossy().into_owned();

    run_from([
        "rightloom-fx",
        "burn",
        "-r",
        "320x200",
        "-d",
        "50",
        "-s",
        "70",
        "-b",
        "70",
        "-l",
        "80",
        "--saturation",
        "85",
        "-a",
        "2",
        "-o",
        output_argument.as_str(),
    ])
    .expect("command should generate images");

    for number in 1..=2 {
        let path = output.path().join(format!("burn-{number:04}.png"));
        assert!(path.is_file(), "{} should exist", path.display());
        let image = image::open(&path).expect("output should be a readable PNG");
        assert_eq!(image.dimensions(), (320, 200));
        assert!(image.to_rgba8().pixels().all(|pixel| pixel[3] == 255));
    }
}

#[test]
fn bokeh_edge_rejects_center_placement_before_writing_output() {
    let output = TestOutputDir::new();
    let output_argument = output.path().to_string_lossy().into_owned();

    let error = run_from([
        "rightloom-fx",
        "bokeh",
        "-r",
        "320x200",
        "-t",
        "edge",
        "-p",
        "center",
        "-d",
        "50",
        "-a",
        "1",
        "-o",
        output_argument.as_str(),
    ])
    .expect_err("edge bokeh center placement should fail");

    assert!(
        error
            .to_string()
            .contains("center placement is not available for edge bokeh")
    );
    assert!(
        fs::read_dir(output.path())
            .expect("test directory should be readable")
            .next()
            .is_none()
    );
}
