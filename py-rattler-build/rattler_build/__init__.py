from .rattler_build import get_rattler_build_version_py as _get_rattler_build_version_py


def rattler_build_version() -> str:
    return _get_rattler_build_version_py()
