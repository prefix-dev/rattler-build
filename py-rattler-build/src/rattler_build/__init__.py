from rattler_build import (
    package,
    progress,
    recipe_generation,
    render,
    stage0,
    stage1,
    tool_config,
)
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
    upload_package_to_anaconda,
    upload_package_to_artifactory,
    upload_package_to_prefix,
    upload_package_to_quetz,
    upload_packages_to_conda_forge,
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
from rattler_build.render import RenderConfig
from rattler_build.stage0 import Stage0Recipe
from rattler_build.stage1 import Stage1Recipe
from rattler_build.tool_config import ToolConfiguration
from rattler_build.variant_config import VariantConfig

__all__ = [
    # Core API
    "rattler_build_version",
    "build_recipes",
    "test_package",
    "upload_package_to_quetz",
    "upload_package_to_artifactory",
    "upload_package_to_prefix",
    "upload_package_to_anaconda",
    "upload_packages_to_conda_forge",
    "recipe_generation",
    # Configuration
    "BuildResult",
    "JinjaConfig",
    "VariantConfig",
    "ToolConfiguration",
    "RenderConfig",
    # Recipe types
    "Stage0Recipe",
    "Stage1Recipe",
    # Recipe modules
    "stage0",
    "stage1",
    "render",
    "tool_config",
    "progress",
    # Package inspection and testing
    "package",
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
