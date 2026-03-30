<h1>
  <a href="https://prefix.dev/tools/rattler-build">
    <img alt="banner" src="https://github.com/user-attachments/assets/456f8ef1-1c7b-463d-ad88-de3496b05db2">
  </a>
</h1>

# rattler_build_allocator

Platform-optimized memory allocator for rattler-build, using jemalloc on Unix and mimalloc on Windows.

This crate is designed to be used as a dependency that, when included, automatically sets the global allocator. Simply add this crate as a dependency and the allocator will be configured.
