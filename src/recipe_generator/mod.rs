//! Module for generating recipes for Python (PyPI), R (CRAN), Perl (CPAN), or Lua (LuaRocks) packages
use clap::Parser;

mod cpan;
mod cran;
mod luarocks;
mod pypi;
mod serialize;

use cpan::{CpanOpts, generate_cpan_recipe};
use cran::{CranOpts, generate_r_recipe};
use luarocks::{LuarocksOpts, generate_luarocks_recipe};
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

    /// Generate a recipe for a Perl package from CPAN
    Cpan(CpanOpts),

    /// Generate a recipe for a Lua package from LuaRocks
    Luarocks(LuarocksOpts),
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
        Source::Cpan(opts) => generate_cpan_recipe(&opts).await?,
        Source::Luarocks(opts) => generate_luarocks_recipe(&opts).await?,
    }

    Ok(())
}
