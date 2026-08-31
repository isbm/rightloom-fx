pub mod bokeh;
pub mod burn;
mod cli;
pub mod render;
pub mod scratches;
pub mod stain;

use std::error::Error;

use crate::{
    bokeh::{BokehSettings, generate_images as generate_bokeh_images},
    burn::{BurnSettings, generate_images as generate_burn_images},
    cli::{Cli, Command},
    render::RenderError,
    scratches::{ScratchSettings, generate_images},
    stain::{StainSettings, generate_images as generate_stain_images},
};

pub fn run() -> Result<(), Box<dyn Error>> {
    run_cli(Cli::parse()).map_err(Into::into)
}

fn run_cli(cli: Cli) -> Result<(), RenderError> {
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
        Command::Bokeh(args) => generate_bokeh_images(&BokehSettings {
            render: args.render,
            types: args.types,
            placements: args.placements,
            blur: args.blur,
            lightness: args.lightness,
            deform: args.deform,
            size: args.size,
            uniform: args.uniform,
        }),
        Command::Burn(args) => generate_burn_images(&BurnSettings {
            render: args.render,
            size: args.size,
            blur: args.blur,
            lightness: args.lightness,
            saturation: args.saturation,
        }),
    }
}

#[cfg(test)]
#[path = "lib_ut.rs"]
mod lib_ut;
