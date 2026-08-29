mod cli;
pub mod render;
pub mod scratches;
pub mod stain;

use std::error::Error;

use crate::{
    cli::{Cli, Command},
    scratches::{ScratchSettings, generate_images},
    stain::{StainSettings, generate_images as generate_stain_images},
};

pub fn run() -> Result<(), Box<dyn Error>> {
    run_cli(Cli::parse()).map_err(Into::into)
}

fn run_cli(cli: Cli) -> Result<(), scratches::RenderError> {
    match cli.command {
        Command::Scratches(args) => generate_images(&ScratchSettings {
            render: args.render,
            effects: args.effects,
        }),
        Command::Stain(args) => generate_stain_images(&StainSettings {
            render: args.render,
            blur: args.blur,
            lightness: args.lightness,
            contrast: args.contrast,
        }),
    }
}

#[cfg(test)]
#[path = "lib_ut.rs"]
mod lib_ut;
