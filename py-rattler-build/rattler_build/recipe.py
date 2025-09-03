"""Object-oriented interface for Recipe parsing and manipulation."""

from pathlib import Path
from typing import Any, Dict, List, Optional, Union
from .rattler_build import parse_recipe_py


class Build:
    """Build configuration for a recipe."""

    def __init__(self, data: Dict[str, Any]):
        self._data = data

    @property
    def number(self) -> int:
        """Get the build number."""
        return self._data.get("number", 0)

    @property
    def string(self) -> Optional[str]:
        """Get the build string."""
        return self._data.get("string")

    @property
    def script(self) -> Optional[str]:
        """Get the build script."""
        script_data = self._data.get("script")
        if script_data:
            return str(script_data)
        return None

    @property
    def noarch(self) -> Optional[str]:
        """Get the noarch type if any."""
        noarch_data = self._data.get("noarch")
        if noarch_data is None:
            return None
        if isinstance(noarch_data, str):
            return noarch_data
        if isinstance(noarch_data, dict):
            return noarch_data.get("type", "generic")
        return "generic"

    @property
    def noarch_type(self) -> Optional[str]:
        """Get the noarch type if any (alias for noarch property)."""
        return self.noarch

    def is_noarch(self) -> bool:
        """Check if this is a noarch build."""
        return self.noarch is not None

    def has_script(self) -> bool:
        """Check if this build has a script defined."""
        return self.script is not None

    def __repr__(self) -> str:
        return f"Build(number={self.number}, noarch={self.is_noarch()})"


class Package:
    """Package information for a recipe."""

    def __init__(self, data: Dict[str, Any]):
        self._data = data

    @property
    def name(self) -> str:
        """Get the package name."""
        return self._data.get("name", "")

    @property
    def version(self) -> str:
        """Get the package version."""
        return self._data.get("version", "")

    def __repr__(self) -> str:
        return f"Package(name='{self.name}', version='{self.version}')"

    def __str__(self) -> str:
        return f"{self.name}-{self.version}"


class Requirements:
    """Requirements for a recipe."""

    def __init__(self, data: Dict[str, Any]):
        self._data = data

    @property
    def build(self) -> List[str]:
        """Get build requirements."""
        return self._data.get("build", [])

    @property
    def host(self) -> List[str]:
        """Get host requirements."""
        return self._data.get("host", [])

    @property
    def run(self) -> List[str]:
        """Get run requirements."""
        return self._data.get("run", [])

    @property
    def run_constrained(self) -> List[str]:
        """Get run constraints."""
        return self._data.get("run_constrained", [])

    @property
    def run_exports(self) -> Optional[List[str]]:
        """Get run exports."""
        exports = self._data.get("run_exports")
        if exports:
            # Flatten weak and strong exports
            result = []
            if isinstance(exports, dict):
                result.extend(exports.get("weak", []))
                result.extend(exports.get("strong", []))
            elif isinstance(exports, list):
                result.extend(exports)
            return result
        return None

    def __repr__(self) -> str:
        return f"Requirements(build={len(self.build)}, host={len(self.host)}, run={len(self.run)})"


class About:
    """About information for a recipe."""

    def __init__(self, data: Dict[str, Any]):
        self._data = data or {}

    @property
    def homepage(self) -> Optional[str]:
        """Get the homepage URL."""
        return self._data.get("homepage")

    @property
    def repository(self) -> Optional[str]:
        """Get the repository URL."""
        return self._data.get("repository")

    @property
    def documentation(self) -> Optional[str]:
        """Get the documentation URL."""
        return self._data.get("documentation")

    @property
    def license(self) -> Optional[str]:
        """Get the license string."""
        return self._data.get("license")

    @property
    def license_file(self) -> Optional[str]:
        """Get the license file."""
        return self._data.get("license_file")

    @property
    def summary(self) -> Optional[str]:
        """Get the summary."""
        return self._data.get("summary")

    @property
    def description(self) -> Optional[str]:
        """Get the description."""
        return self._data.get("description")

    def __repr__(self) -> str:
        return f"About(homepage={self.homepage!r}, license={self.license!r}, summary={self.summary!r})"


class Source:
    """Source information for a recipe."""

    def __init__(self, data: Dict[str, Any]):
        self._data = data

    @property
    def url(self) -> Optional[str]:
        """Get source URL if available."""
        url = self._data.get("url")
        if isinstance(url, list) and url:
            return str(url[0])
        return str(url) if url is not None else None

    @property
    def source_type(self) -> str:
        """Get source type as string."""
        if "url" in self._data:
            return "url"
        elif "git" in self._data:
            return "git"
        elif "path" in self._data:
            return "path"
        return "unknown"

    @property
    def sha256(self) -> Optional[str]:
        """Get SHA256 hash if available."""
        return self._data.get("sha256")

    @property
    def md5(self) -> Optional[str]:
        """Get MD5 hash if available."""
        return self._data.get("md5")

    @property
    def git_rev(self) -> Optional[str]:
        """Get git revision if it's a git source."""
        return self._data.get("rev")

    @property
    def path(self) -> Optional[str]:
        """Get path if it's a path source."""
        return self._data.get("path")

    @property
    def filename(self) -> Optional[str]:
        """Get filename for URL source."""
        return self._data.get("file_name")

    @property
    def patches(self) -> List[str]:
        """Get patches list."""
        return self._data.get("patches", [])

    def __repr__(self) -> str:
        if self.source_type == "url":
            return f"Source(type='url', url='{self.url}')"
        elif self.source_type == "git":
            return f"Source(type='git', url='{self.url}', rev='{self.git_rev}')"
        elif self.source_type == "path":
            return f"Source(type='path', path='{self.path}')"
        return f"Source(type='{self.source_type}')"


class TestType:
    """Test type for a recipe."""

    def __init__(self, data: Dict[str, Any]):
        self._data = data

    @property
    def test_type(self) -> str:
        """Get the test type name."""
        if "Command" in self._data:
            return "command"
        elif "Python" in self._data:
            return "python"
        elif "PackageContents" in self._data:
            return "package_contents"
        elif "Downstream" in self._data:
            return "downstream"
        elif "Perl" in self._data:
            return "perl"
        elif "R" in self._data:
            return "r"
        elif "Ruby" in self._data:
            return "ruby"
        return "unknown"

    @property
    def commands(self) -> Optional[List[str]]:
        """Get test commands if this is a commands test."""
        if "Command" in self._data:
            return [f"Command test: {self._data['Command']}"]
        return None

    @property
    def python_imports(self) -> Optional[List[str]]:
        """Get Python imports if this is a Python test."""
        if "Python" in self._data:
            python_data = self._data["Python"]
            if isinstance(python_data, dict):
                return python_data.get("imports", [])
        return None

    @property
    def files(self) -> Optional[List[str]]:
        """Get files list if this is a package contents test."""
        if "PackageContents" in self._data:
            return [f"Package contents test: {self._data['PackageContents']}"]
        return None

    @property
    def downstream_packages(self) -> Optional[List[str]]:
        """Get downstream packages if this is a downstream test."""
        if "Downstream" in self._data:
            return [self._data["Downstream"]]
        return None

    def __repr__(self) -> str:
        return f"TestType(type='{self.test_type}')"


class Recipe:
    """A parsed conda recipe with object-oriented access to all fields."""

    def __init__(self, data: Dict[str, Any]):
        self._data = data

    @classmethod
    def from_yaml(
        cls,
        yaml_content: str,
        target_platform: Optional[str] = None,
        host_platform: Optional[str] = None,
        build_platform: Optional[str] = None,
        experimental: Optional[bool] = None,
        allow_undefined: Optional[bool] = None,
    ) -> "Recipe":
        """Create a Recipe from a YAML string.

        Args:
            yaml_content: The YAML content to parse
            target_platform: The target platform (e.g., 'linux-64', 'win-64'). Defaults to current platform.
            host_platform: The host platform (relevant for noarch packages). Defaults to current platform.
            build_platform: The build platform. Defaults to current platform.
            experimental: Enable experimental features. Defaults to False.
            allow_undefined: Allow undefined variables in Jinja templates. Defaults to False.
        """
        data = parse_recipe_py(
            yaml_content,
            target_platform=target_platform,
            host_platform=host_platform,
            build_platform=build_platform,
            experimental=experimental,
            allow_undefined=allow_undefined,
        )
        return cls(data)

    @classmethod
    def from_file(
        cls,
        path: Union[str, Path],
        target_platform: Optional[str] = None,
        host_platform: Optional[str] = None,
        build_platform: Optional[str] = None,
        experimental: Optional[bool] = None,
        allow_undefined: Optional[bool] = None,
    ) -> "Recipe":
        """Create a Recipe from a YAML file path.

        Args:
            path: Path to the YAML file
            target_platform: The target platform (e.g., 'linux-64', 'win-64'). Defaults to current platform.
            host_platform: The host platform (relevant for noarch packages). Defaults to current platform.
            build_platform: The build platform. Defaults to current platform.
            experimental: Enable experimental features. Defaults to False.
            allow_undefined: Allow undefined variables in Jinja templates. Defaults to False.
        """
        with open(path, "r", encoding="utf-8") as f:
            content = f.read()
        return cls.from_yaml(
            content,
            target_platform=target_platform,
            host_platform=host_platform,
            build_platform=build_platform,
            experimental=experimental,
            allow_undefined=allow_undefined,
        )

    @property
    def schema_version(self) -> int:
        """Get the schema version of this recipe."""
        return self._data.get("schema_version", 1)

    @property
    def context(self) -> Dict[str, Any]:
        """Get the context values as a Python dictionary."""
        return self._data.get("context", {})

    @property
    def package(self) -> Package:
        """Get the package information."""
        return Package(self._data.get("package", {}))

    @property
    def source(self) -> List[Source]:
        """Get the source information."""
        sources = self._data.get("source", [])
        if isinstance(sources, dict):
            sources = [sources]
        return [Source(s) for s in sources]

    @property
    def build(self) -> Build:
        """Get the build information."""
        return Build(self._data.get("build", {}))

    @property
    def requirements(self) -> Requirements:
        """Get the requirements information."""
        return Requirements(self._data.get("requirements", {}))

    @property
    def tests(self) -> List[TestType]:
        """Get the tests information."""
        tests = self._data.get("tests", [])
        return [TestType(t) for t in tests]

    @property
    def about(self) -> About:
        """Get the about information."""
        return About(self._data.get("about", {}))

    @property
    def extra(self) -> Dict[str, Any]:
        """Get extra information as a Python dictionary."""
        return self._data.get("extra", {})

    def has_tests(self) -> bool:
        """Check if this recipe has any tests defined."""
        return len(self.tests) > 0

    def is_noarch(self) -> bool:
        """Check if this recipe builds a noarch package."""
        return self.build.is_noarch()

    def __repr__(self) -> str:
        return f"Recipe(package={self.package.name}, schema_version={self.schema_version})"
