mod cli;
pub mod render;
pub mod scratches;
pub mod stain;

use std::error::Error;

#[cfg(test)]
use std::ffi::OsString;

use crate::{
    cli::{Cli, Command},
    scratches::{ScratchSettings, generate_images},
    stain::{StainSettings, generate_images as generate_stain_images},
};

pub fn run() -> Result<(), Box<dyn Error>> {
    run_cli(Cli::parse()).map_err(Into::into)
}

#[cfg(test)]
fn run_from<I, T>(arguments: I) -> Result<(), Box<dyn Error>>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = Cli::try_parse_from(arguments)?;
    run_cli(cli).map_err(Into::into)
}

fn run_cli(cli: Cli) -> Result<(), scratches::RenderError> {
    match cli.command {
        Command::Scratches(args) => generate_images(&ScratchSettings {
            render: args.render,
            effects: args.effects,
        }),
        Command::Stain(args) => generate_stain_images(&StainSettings {
            render: args.render,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        process,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use image::GenericImageView;

    use super::run_from;

    static TEMP_DIRECTORY_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TestOutputDir(PathBuf);

    impl TestOutputDir {
        fn new() -> Self {
            let number = TEMP_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("rightloom-fx-test-{}-{number}", process::id()));
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
            assert!(rgba.pixels().any(|pixel| pixel[3] > 0));
            assert!(rgba.pixels().any(|pixel| pixel[3] == 0));
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
            assert!(image.to_rgba8().pixels().any(|pixel| pixel[3] > 0));
        }
    }
}
