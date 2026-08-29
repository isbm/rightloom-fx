use std::{
    fs,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicUsize, Ordering},
};

use image::{Rgba, RgbaImage};

use super::{
    ExportPolicy, RenderError, RenderSettings, Resolution, RgbColor, next_sequence_start,
    output_filename, reserve_next_output_file, write_images_with_progress,
};

static TEMP_DIRECTORY_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TestOutputDir(PathBuf);

impl TestOutputDir {
    fn new() -> Self {
        let number = TEMP_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".tmp")
            .join(format!(
                "rightloom-fx-render-test-{}-{number}",
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

fn add_file(directory: &TestOutputDir, name: &str) {
    fs::write(directory.path().join(name), b"existing output")
        .expect("test output file should be created");
}

fn test_settings(directory: &TestOutputDir, amount: u32) -> RenderSettings {
    RenderSettings {
        resolution: Resolution::new(1, 1).expect("test resolution should be valid"),
        density: 0,
        amount,
        outdir: directory.path().to_path_buf(),
        export_policy: ExportPolicy::PreserveAlpha,
    }
}

#[test]
fn default_export_policy_flattens_onto_black() {
    assert_eq!(
        ExportPolicy::default(),
        ExportPolicy::Flatten(RgbColor::BLACK)
    );

    let mut image = RgbaImage::from_pixel(1, 1, Rgba([160, 160, 160, 64]));

    ExportPolicy::default().apply_to(&mut image);

    assert_eq!(image.dimensions(), (1, 1));
    assert_eq!(image.get_pixel(0, 0).0, [40, 40, 40, 255]);
}

#[test]
fn transparent_pixels_become_the_background_color() {
    let mut image = RgbaImage::from_pixel(1, 1, Rgba([200, 100, 50, 0]));

    ExportPolicy::Flatten(RgbColor::new(12, 34, 56)).apply_to(&mut image);

    assert_eq!(image.get_pixel(0, 0).0, [12, 34, 56, 255]);
}

#[test]
fn opaque_pixels_keep_their_source_color() {
    let mut image = RgbaImage::from_pixel(1, 1, Rgba([17, 34, 51, 255]));

    ExportPolicy::Flatten(RgbColor::new(200, 180, 160)).apply_to(&mut image);

    assert_eq!(image.get_pixel(0, 0).0, [17, 34, 51, 255]);
}

#[test]
fn half_alpha_pixels_use_rounded_source_over_composition() {
    let mut image = RgbaImage::from_pixel(1, 1, Rgba([200, 100, 50, 128]));

    ExportPolicy::Flatten(RgbColor::new(20, 40, 60)).apply_to(&mut image);

    assert_eq!(image.get_pixel(0, 0).0, [110, 70, 55, 255]);
}

#[test]
fn preserve_alpha_leaves_pixels_and_dimensions_unchanged() {
    let mut image = RgbaImage::from_raw(2, 1, vec![12, 34, 56, 0, 160, 160, 160, 64])
        .expect("test image data should be valid");
    let expected = image.clone();

    ExportPolicy::PreserveAlpha.apply_to(&mut image);

    assert_eq!(image.dimensions(), (2, 1));
    assert_eq!(image, expected);
}

#[test]
fn empty_output_directory_starts_at_sequence_one() {
    let output = TestOutputDir::new();

    assert_eq!(
        next_sequence_start(output.path(), "stain").expect("sequence scan should succeed"),
        1
    );
}

#[test]
fn matching_sequences_continue_after_the_highest_number() {
    let output = TestOutputDir::new();
    add_file(&output, "stain-0001.png");
    add_file(&output, "stain-0012.png");
    add_file(&output, "stain-7.png");

    assert_eq!(
        next_sequence_start(output.path(), "stain").expect("sequence scan should succeed"),
        13
    );
}

#[test]
fn unrelated_files_do_not_affect_sequence_selection() {
    let output = TestOutputDir::new();
    for name in [
        "stain-9999.jpg",
        "stain-9999.PNG",
        "Stain-9999.png",
        "stain-nine.png",
        "stain--9999.png",
        "stain-9999.png.bak",
        "notes.txt",
    ] {
        add_file(&output, name);
    }

    assert_eq!(
        next_sequence_start(output.path(), "stain").expect("sequence scan should succeed"),
        1
    );
}

#[test]
fn other_prefixes_do_not_affect_sequence_selection() {
    let output = TestOutputDir::new();
    add_file(&output, "scratches-9999.png");

    assert_eq!(
        next_sequence_start(output.path(), "stain").expect("sequence scan should succeed"),
        1
    );
}

#[test]
fn matching_directories_do_not_affect_sequence_selection() {
    let output = TestOutputDir::new();
    fs::create_dir(output.path().join("stain-9999.png"))
        .expect("matching directory should be created");

    assert_eq!(
        next_sequence_start(output.path(), "stain").expect("sequence scan should succeed"),
        1
    );
}

#[test]
fn sequence_selection_does_not_fill_gaps() {
    let output = TestOutputDir::new();
    add_file(&output, "stain-0001.png");
    add_file(&output, "stain-0003.png");

    assert_eq!(
        next_sequence_start(output.path(), "stain").expect("sequence scan should succeed"),
        4
    );
}

#[test]
fn output_filenames_keep_four_digit_minimum_padding() {
    assert_eq!(output_filename("stain", 1), "stain-0001.png");
    assert_eq!(output_filename("stain", 73), "stain-0073.png");
    assert_eq!(output_filename("stain", 9999), "stain-9999.png");
}

#[test]
fn sequence_continues_naturally_past_four_digits() {
    let output = TestOutputDir::new();
    add_file(&output, "stain-9999.png");

    let sequence =
        next_sequence_start(output.path(), "stain").expect("sequence scan should succeed");

    assert_eq!(sequence, 10_000);
    assert_eq!(output_filename("stain", sequence), "stain-10000.png");
}

#[test]
fn reserving_a_filename_never_reuses_an_existing_file() {
    let output = TestOutputDir::new();
    add_file(&output, "stain-0001.png");
    add_file(&output, "stain-0002.png");

    let (sequence, path, file) =
        reserve_next_output_file(output.path(), "stain", 1).expect("file should be reserved");
    drop(file);

    assert_eq!(sequence, 3);
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("stain-0003.png")
    );
    assert!(path.is_file(), "reserved output should exist");
}

#[test]
fn repeated_writes_append_without_overwriting_existing_outputs() {
    let output = TestOutputDir::new();
    let original = b"do not overwrite";
    fs::write(output.path().join("stain-0001.png"), original)
        .expect("existing output should be created");
    let settings = test_settings(&output, 2);

    write_images_with_progress(
        &settings,
        "stain",
        || Ok(RgbaImage::from_pixel(1, 1, Rgba([12, 34, 56, 255]))),
        |_| {},
    )
    .expect("first write should succeed");
    write_images_with_progress(
        &settings,
        "stain",
        || Ok(RgbaImage::from_pixel(1, 1, Rgba([12, 34, 56, 255]))),
        |_| {},
    )
    .expect("second write should succeed");

    assert_eq!(
        fs::read(output.path().join("stain-0001.png")).expect("existing output should be readable"),
        original
    );
    for number in 2..=5 {
        assert!(
            output
                .path()
                .join(output_filename("stain", number))
                .is_file(),
            "appended output {number} should exist"
        );
    }
}

#[test]
fn fallible_rendering_removes_its_reserved_output() {
    let output = TestOutputDir::new();
    let settings = test_settings(&output, 1);

    let error = write_images_with_progress(
        &settings,
        "bokeh",
        || {
            Err(RenderError::TwinkleCoverageUnreachable {
                target: 0.995,
                coverage: 0.90,
                count: 4_096,
            })
        },
        |_| {},
    )
    .expect_err("unreachable twinkle coverage should fail without writing an image");

    assert!(matches!(
        error,
        RenderError::TwinkleCoverageUnreachable { .. }
    ));
    assert!(
        fs::read_dir(output.path())
            .expect("test output should be readable")
            .next()
            .is_none()
    );
}
