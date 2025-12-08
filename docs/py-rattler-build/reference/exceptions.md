# Exceptions

Exception types raised by rattler-build operations.

```python
from rattler_build import RattlerBuildError, RecipeParseError, UploadError
```

All exceptions inherit from `RattlerBuildError`.

| Exception | Description |
|-----------|-------------|
| `RattlerBuildError` | Base exception for all rattler-build errors |
| `AuthError` | Authentication failed |
| `ChannelError` | Channel configuration or access error |
| `ChannelPriorityError` | Invalid channel priority setting |
| `IoError` | File or network I/O error |
| `JsonError` | JSON parsing or serialization error |
| `PackageFormatError` | Invalid package format |
| `PlatformParseError` | Invalid platform string |
| `RecipeParseError` | Recipe YAML parsing error |
| `UploadError` | Package upload failed |
| `UrlParseError` | Invalid URL |
| `VariantError` | Variant configuration error |
