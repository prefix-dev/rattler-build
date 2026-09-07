from rattler_build._rattler_build import (
    AuthError,
    BuildError,
    ChannelError,
    ChannelPriorityError,
    EnvironmentIsolation,
    IoError,
    JsonError,
    PackageFormatError,
    PlatformParseError,
    RattlerBuildError,
    RecipeParseError,
    RepodataRevision,
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
from rattler_build.debug import DebugPaths, DebugSession, ScriptResult
from rattler_build.jinja_config import JinjaConfig
from rattler_build.package import (
    CommandsTest,
    DownstreamTest,
    FileChecks,
    Package,
    PackageContentsTest,
    PackageTestType,
    PathEntry,
    PerlTest,
    PythonTest,
    PythonVersion,
    RebuildResult,
    RTest,
    RubyTest,
    TestResult,
)
from rattler_build.package_assembler import (
    ArchiveType,
    FileEntry,
    PackageOutput,
    assemble_package,
    collect_files,
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
    upload_package_to_cloudsmith,
    upload_package_to_prefix,
    upload_package_to_quetz,
    upload_packages_to_conda_forge,
)
from rattler_build.variant_config import VariantConfig

__all__ = [
    "ArchiveType",
    "AuthError",
    "BuildError",
    "BuildResult",
    "ChannelError",
    "ChannelPriorityError",
    "CommandsTest",
    "DebugPaths",
    # Debug
    "DebugSession",
    "DownstreamTest",
    # Build configuration
    "EnvironmentIsolation",
    "FileChecks",
    "FileEntry",
    "IoError",
    "JinjaConfig",
    "JsonError",
    "MultiOutputRecipe",
    # Package inspection and testing
    "Package",
    "PackageContentsTest",
    "PackageFormatError",
    "PackageOutput",
    "PackageTestType",
    "PathEntry",
    "PerlTest",
    "PlatformConfig",
    "PlatformParseError",
    "PythonTest",
    "PythonVersion",
    "RTest",
    # Exceptions
    "RattlerBuildError",
    "RebuildResult",
    "RecipeParseError",
    "RenderConfig",
    "RenderedVariant",
    "RepodataRevision",
    "RubyTest",
    "ScriptResult",
    "SingleOutputRecipe",
    # Recipe types
    "Stage0Recipe",
    "Stage1Recipe",
    "TestResult",
    "ToolConfiguration",
    "UploadError",
    "UrlParseError",
    "VariantConfig",
    "VariantError",
    # Package assembly (low-level)
    "assemble_package",
    "build_recipes",
    "collect_files",
    "generate_cpan_recipe",
    "generate_cran_recipe",
    "generate_luarocks_recipe",
    # Recipe generation
    "generate_pypi_recipe",
    # Core API
    "rattler_build_version",
    "test_package",
    "upload_package_to_anaconda",
    "upload_package_to_artifactory",
    "upload_package_to_cloudsmith",
    "upload_package_to_prefix",
    # Upload
    "upload_package_to_quetz",
    "upload_packages_to_conda_forge",
]


def rattler_build_version() -> str:
    """Get the version of the Rattler-Build package"""
    return get_rattler_build_version_py()
