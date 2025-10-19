//! Recipe generation for Python (PyPI), R (CRAN), Perl (CPAN), and Lua (LuaRocks) packages

#[cfg(feature = "cli")]
use clap::Parser;

mod cpan;
mod cran;
mod luarocks;
mod pypi;
mod serialize;

pub use self::cpan::{CpanOpts, generate_cpan_recipe, generate_cpan_recipe_string};
pub use self::cran::{CranOpts, generate_r_recipe, generate_r_recipe_string};
pub use self::luarocks::{LuarocksOpts, generate_luarocks_recipe, generate_luarocks_recipe_string};
pub use self::pypi::{PyPIOpts, generate_pypi_recipe, generate_pypi_recipe_string};
pub use serialize::write_recipe;

/// The source of the package to generate a recipe for
#[cfg(feature = "cli")]
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
#[cfg(feature = "cli")]
#[derive(Parser)]
pub struct GenerateRecipeOpts {
    /// Type of package to generate a recipe for
    #[clap(subcommand)]
    pub source: Source,
}

/// Generate a recipe for a package
#[cfg(feature = "cli")]
pub async fn generate_recipe(args: GenerateRecipeOpts) -> miette::Result<()> {
    match args.source {
        Source::Pypi(opts) => generate_pypi_recipe(&opts).await?,
        Source::Cran(opts) => generate_r_recipe(&opts).await?,
        Source::Cpan(opts) => generate_cpan_recipe(&opts).await?,
        Source::Luarocks(opts) => generate_luarocks_recipe(&opts).await?,
    }

    Ok(())
}
