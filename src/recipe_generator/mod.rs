//! Module for generating recipes for Python (PyPI) or R (CRAN) packages
use clap::Parser;

mod cran;

mod pypi;
mod serialize;

use cran::generate_r_recipe;
pub use serialize::write_recipe;

use self::pypi::generate_pypi_recipe;

/// The source of the package to generate a recipe for
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum Source {
    /// Generate a recipe for a Python package from PyPI
    Pypi,
    /// Generate a recipe for an R package from CRAN
    Cran,
}

/// Options for generating a recipe
#[derive(Parser)]
pub struct GenerateRecipeOpts {
    /// Type of package to generate a recipe for
    #[arg(value_enum)]
    pub source: Source,
    /// Name of the package to generate
    pub package: String,

    /// Whether to write the recipe to a folder
    #[arg(short, long)]
    pub write: bool,

    /// Whether to generate the whole dependency tree
    #[arg(short, long)]
    pub tree: bool,
}

/// Generate a recipe for a package
pub async fn generate_recipe(args: GenerateRecipeOpts) -> miette::Result<()> {
    match args.source {
        Source::Pypi => generate_pypi_recipe(&args.package, args.write).await?,
        Source::Cran => generate_r_recipe(&args.package, args.write, args.tree).await?,
    }

    Ok(())
}
