from . import platform_types, progress, recipe_generation, render, stage0, stage1, tool_config
from .cli_api import (
    build_recipes,
    test_package,
    upload_package_to_anaconda,
    upload_package_to_artifactory,
    upload_package_to_prefix,
    upload_package_to_quetz,
    upload_packages_to_conda_forge,
)
from .jinja_config import JinjaConfig
from .platform_types import Platform, PlatformWithVirtualPackages
from .rattler_build import RattlerBuildError, get_rattler_build_version_py
from .recipe import (
    About,
    Build,
    Package,
    Recipe,
    Requirements,
    Source,
    TestType,
    TestTypeEnum,
)
from .render import RenderConfig
from .tool_config import ToolConfiguration
from .variant_config import VariantConfig


def rattler_build_version() -> str:
    """Get the version of the rattler-build package"""
    return get_rattler_build_version_py()


__all__ = [
    "rattler_build_version",
    "build_recipes",
    "test_package",
    "upload_package_to_quetz",
    "upload_package_to_artifactory",
    "upload_package_to_prefix",
    "upload_package_to_anaconda",
    "upload_packages_to_conda_forge",
    "recipe_generation",
    "Recipe",
    "Package",
    "Build",
    "Requirements",
    "RattlerBuildError",
    "About",
    "Source",
    "TestType",
    "TestTypeEnum",
    "JinjaConfig",
    "stage0",
    "stage1",
    "render",
    "tool_config",
    "platform_types",
    "progress",
    "VariantConfig",
    "ToolConfiguration",
    "Platform",
    "PlatformWithVirtualPackages",
    "RenderConfig",
]
