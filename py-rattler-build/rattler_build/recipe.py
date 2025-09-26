"""Object-oriented interface for Recipe parsing and manipulation."""

from enum import Enum
from pathlib import Path
from typing import Any, Dict, List, Optional, Union
from .rattler_build import parse_recipe_py, PySelectorConfig


class TestTypeEnum(Enum):
    """Enumeration of test types."""

    COMMAND = "command"
    PYTHON = "python"
    PACKAGE_CONTENTS = "package_contents"
    DOWNSTREAM = "downstream"
    PERL = "perl"
    R = "r"
    RUBY = "ruby"
    UNKNOWN = "unknown"


class Build:
    """Build configuration for a recipe."""

    _data: Dict[str, Any]

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

    @property
    def is_noarch(self) -> bool:
        """Check if this is a noarch build."""
        return self.noarch is not None

    @property
    def script(self) -> Optional[str]:
        """Get the build script if defined."""
        return self._data.get("script")

    @property
    def has_script(self) -> bool:
        """Check if this build has a script defined."""
        return self.script is not None

    def __repr__(self) -> str:
        return f"Build(number={self.number}, noarch={self.is_noarch})"


class Package:
    """Package information for a recipe."""

    _data: Dict[str, Any]

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

    _data: Dict[str, Any]

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

    _data: Dict[str, Any]

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

    _data: Dict[str, Any]

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

    _data: Dict[str, Any]

    def __init__(self, data: Dict[str, Any]):
        self._data = data

    @property
    def test_type(self) -> TestTypeEnum:
        """Get the test type name."""
        if "Command" in self._data:
            return TestTypeEnum.COMMAND
        elif "Python" in self._data:
            return TestTypeEnum.PYTHON
        elif "PackageContents" in self._data:
            return TestTypeEnum.PACKAGE_CONTENTS
        elif "Downstream" in self._data:
            return TestTypeEnum.DOWNSTREAM
        elif "Perl" in self._data:
            return TestTypeEnum.PERL
        elif "R" in self._data:
            return TestTypeEnum.R
        elif "Ruby" in self._data:
            return TestTypeEnum.RUBY
        return TestTypeEnum.UNKNOWN

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


class SelectorConfig:
    """Python wrapper for PySelectorConfig to provide a cleaner interface."""

    _config: PySelectorConfig

    def __init__(
        self,
        target_platform: Optional[str] = None,
        host_platform: Optional[str] = None,
        build_platform: Optional[str] = None,
        experimental: Optional[bool] = None,
        allow_undefined: Optional[bool] = None,
        variant: Optional[Dict[str, Any]] = None,
    ):
        self._config = PySelectorConfig(
            target_platform=target_platform,
            host_platform=host_platform,
            build_platform=build_platform,
            experimental=experimental,
            allow_undefined=allow_undefined,
            variant=variant,
        )

    @property
    def target_platform(self) -> Optional[str]:
        """Get the target platform."""
        return self._config.target_platform

    @target_platform.setter
    def target_platform(self, value: Optional[str]) -> None:
        """Set the target platform."""
        self._config.target_platform = value

    @property
    def host_platform(self) -> Optional[str]:
        """Get the host platform."""
        return self._config.host_platform

    @host_platform.setter
    def host_platform(self, value: Optional[str]) -> None:
        """Set the host platform."""
        self._config.host_platform = value

    @property
    def build_platform(self) -> Optional[str]:
        """Get the build platform."""
        return self._config.build_platform

    @build_platform.setter
    def build_platform(self, value: Optional[str]) -> None:
        """Set the build platform."""
        self._config.build_platform = value

    @property
    def experimental(self) -> Optional[bool]:
        """Get whether experimental features are enabled."""
        return self._config.experimental

    @experimental.setter
    def experimental(self, value: Optional[bool]) -> None:
        """Set whether experimental features are enabled."""
        self._config.experimental = value

    @property
    def allow_undefined(self) -> Optional[bool]:
        """Get whether undefined variables are allowed."""
        return self._config.allow_undefined

    @allow_undefined.setter
    def allow_undefined(self, value: Optional[bool]) -> None:
        """Set whether undefined variables are allowed."""
        self._config.allow_undefined = value

    @property
    def variant(self) -> Dict[str, Any]:
        """Get the variant configuration."""
        return self._config.variant

    @variant.setter
    def variant(self, value: Dict[str, Any]) -> None:
        """Set the variant configuration."""
        self._config.variant = value

    def __repr__(self) -> str:
        return f"SelectorConfig(target_platform={self.target_platform!r}, variant={self.variant!r})"

    @property
    def config(self) -> PySelectorConfig:
        """Get the underlying PySelectorConfig object."""
        return self._config


class Recipe:
    """A parsed conda recipe with object-oriented access to all fields."""

    _data: Dict[str, Any]

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
        variant: Optional[Dict[str, Any]] = None,
    ) -> "Recipe":
        """Create a Recipe from a YAML string.

        Args:
            yaml_content: The YAML content to parse
            target_platform: The target platform (e.g., 'linux-64', 'win-64'). Defaults to current platform.
            host_platform: The host platform (relevant for noarch packages). Defaults to current platform.
            build_platform: The build platform. Defaults to current platform.
            experimental: Enable experimental features. Defaults to False.
            allow_undefined: Allow undefined variables in Jinja templates. Defaults to False.
            variant: Variant configuration as a dictionary. Defaults to empty.
        """
        selector_config = SelectorConfig(
            target_platform=target_platform,
            host_platform=host_platform,
            build_platform=build_platform,
            experimental=experimental,
            allow_undefined=allow_undefined,
            variant=variant,
        )
        data = parse_recipe_py(yaml_content, selector_config.config)
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
        variant: Optional[Dict[str, Any]] = None,
    ) -> "Recipe":
        """Create a Recipe from a YAML file path.

        Args:
            path: Path to the YAML file
            target_platform: The target platform (e.g., 'linux-64', 'win-64'). Defaults to current platform.
            host_platform: The host platform (relevant for noarch packages). Defaults to current platform.
            build_platform: The build platform. Defaults to current platform.
            experimental: Enable experimental features. Defaults to False.
            allow_undefined: Allow undefined variables in Jinja templates. Defaults to False.
            variant: Variant configuration as a dictionary. Defaults to empty.
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
            variant=variant,
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

    @property
    def has_tests(self) -> bool:
        """Check if this recipe has any tests defined."""
        return len(self.tests) > 0

    @property
    def is_noarch(self) -> bool:
        """Check if this recipe builds a noarch package."""
        return self.build.is_noarch

    def __repr__(self) -> str:
        return f"Recipe(package={self.package.name}, schema_version={self.schema_version})"
