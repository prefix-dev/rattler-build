//! Module for generating recipes for Python (PyPI) or R (CRAN) packages
use clap::Parser;

mod cran;

mod pypi;
mod serialize;

use cran::{generate_r_recipe, CranOpts};
use pypi::PyPIOpts;
pub use serialize::write_recipe;

use self::pypi::generate_pypi_recipe;

/// The source of the package to generate a recipe for
#[derive(Debug, Clone, Parser)]
pub enum Source {
    /// Generate a recipe for a Python package from PyPI
    Pypi(PyPIOpts),

    /// Generate a recipe for an R package from CRAN
    Cran(CranOpts),
}

/// Options for generating a recipe
#[derive(Parser)]
pub struct GenerateRecipeOpts {
    /// Type of package to generate a recipe for
    #[clap(subcommand)]
    pub source: Source,
}

/// Generate a recipe for a package
pub async fn generate_recipe(args: GenerateRecipeOpts) -> miette::Result<()> {
    match args.source {
        Source::Pypi(opts) => generate_pypi_recipe(&opts).await?,
        Source::Cran(opts) => generate_r_recipe(&opts).await?,
    }

    Ok(())
}
