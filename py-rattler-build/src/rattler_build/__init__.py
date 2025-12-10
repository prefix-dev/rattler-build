from rattler_build._rattler_build import (
    AuthError,
    ChannelError,
    ChannelPriorityError,
    IoError,
    JsonError,
    PackageFormatError,
    PlatformParseError,
    RattlerBuildError,
    RecipeParseError,
    UploadError,
    UrlParseError,
    VariantError,
    get_rattler_build_version_py,
)
from rattler_build.build_result import BuildResult
from rattler_build.cli_api import (
    build_recipes,
    test_package,
)
from rattler_build.package_assembler import (
    ArchiveType,
    FileEntry,
    PackageOutput,
    assemble_package,
    collect_files,
)
from rattler_build.jinja_config import JinjaConfig
from rattler_build.package import (
    CommandsTest,
    DownstreamTest,
    FileChecks,
    Package,
    PackageContentsTest,
    PackageTest,
    PathEntry,
    PerlTest,
    PythonTest,
    PythonVersion,
    RTest,
    RubyTest,
    TestResult,
)
from rattler_build.recipe_generation import (
    generate_cpan_recipe,
    generate_cran_recipe,
    generate_luarocks_recipe,
    generate_pypi_recipe,
)
from rattler_build.render import RenderConfig, RenderedVariant
from rattler_build.stage0 import MultiOutputRecipe, SingleOutputRecipe, Stage0Recipe
from rattler_build.stage1 import Stage1Recipe
from rattler_build.tool_config import PlatformConfig, ToolConfiguration
from rattler_build.upload import (
    upload_package_to_anaconda,
    upload_package_to_artifactory,
    upload_package_to_prefix,
    upload_package_to_quetz,
    upload_packages_to_conda_forge,
)
from rattler_build.variant_config import VariantConfig

__all__ = [
    # Core API
    "rattler_build_version",
    "build_recipes",
    "test_package",
    # Package assembly (low-level)
    "assemble_package",
    "collect_files",
    "ArchiveType",
    "FileEntry",
    "PackageOutput",
    # Upload
    "upload_package_to_quetz",
    "upload_package_to_artifactory",
    "upload_package_to_prefix",
    "upload_package_to_anaconda",
    "upload_packages_to_conda_forge",
    # Recipe generation
    "generate_pypi_recipe",
    "generate_cran_recipe",
    "generate_cpan_recipe",
    "generate_luarocks_recipe",
    # Configuration
    "BuildResult",
    "JinjaConfig",
    "VariantConfig",
    "ToolConfiguration",
    "PlatformConfig",
    "RenderConfig",
    "RenderedVariant",
    # Recipe types
    "Stage0Recipe",
    "SingleOutputRecipe",
    "MultiOutputRecipe",
    "Stage1Recipe",
    # Package inspection and testing
    "Package",
    "PackageTest",
    "PythonTest",
    "PythonVersion",
    "CommandsTest",
    "PerlTest",
    "RTest",
    "RubyTest",
    "DownstreamTest",
    "PackageContentsTest",
    "FileChecks",
    "TestResult",
    "PathEntry",
    # Exceptions
    "RattlerBuildError",
    "AuthError",
    "ChannelError",
    "ChannelPriorityError",
    "IoError",
    "JsonError",
    "PackageFormatError",
    "PlatformParseError",
    "RecipeParseError",
    "UploadError",
    "UrlParseError",
    "VariantError",
]


def rattler_build_version() -> str:
    """Get the version of the rattler-build package"""
    return get_rattler_build_version_py()
