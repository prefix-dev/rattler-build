# Exceptions

Exception classes raised by rattler-build operations.

All exceptions inherit from `RattlerBuildError`, so you can catch all rattler-build
errors with a single except clause:

```python
from rattler_build import RattlerBuildError, RecipeParseError, Stage0Recipe

try:
    recipe = Stage0Recipe.from_yaml(invalid_yaml)
except RecipeParseError as e:
    print(f"Invalid recipe: {e}")
except RattlerBuildError as e:
    print(f"rattler-build error: {e}")
```

You can import all exceptions from `rattler_build`:

```python
from rattler_build import (
    RattlerBuildError,
    AuthError,
    ChannelError,
    ChannelPriorityError,
    IoError,
    JsonError,
    PackageFormatError,
    PlatformParseError,
    RecipeParseError,
    UploadError,
    UrlParseError,
    VariantError,
)
```

## Exception Hierarchy

All exceptions inherit from `RattlerBuildError`:

```
RattlerBuildError (base)
├── AuthError
├── ChannelError
├── ChannelPriorityError
├── IoError
├── JsonError
├── PackageFormatError
├── PlatformParseError
├── RecipeParseError
├── UploadError
├── UrlParseError
└── VariantError
```

## `RattlerBuildError`

Base exception for all rattler-build errors. Catch this to handle any error from the library.

## `AuthError`

Raised when authentication fails, such as when credentials are missing or invalid.

## `ChannelError`

Raised when there's an issue with a conda channel, such as a channel that can't be accessed.

## `ChannelPriorityError`

Raised when channel priority configuration is invalid.

## `IoError`

Raised for I/O operation failures, such as file read/write errors.

## `JsonError`

Raised when JSON parsing or serialization fails.

## `PackageFormatError`

Raised when a package file format is invalid or corrupted.

## `PlatformParseError`

Raised when a platform string (e.g., "linux-64", "osx-arm64") cannot be parsed.

## `RecipeParseError`

Raised when a recipe cannot be parsed. This includes YAML syntax errors and schema validation failures.

## `UploadError`

Raised when uploading a package to a server fails.

## `UrlParseError`

Raised when a URL cannot be parsed.

## `VariantError`

Raised when variant configuration is invalid, such as mismatched zip_keys lengths.
