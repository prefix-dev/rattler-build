//! Platform-optimized memory allocator for rattler-build, using jemalloc on Unix
//! and mimalloc on Windows.
//!
//! This crate is designed to be used as a dependency that, when included,
//! automatically sets the global allocator. Simply add this crate as a
//! dependency and the allocator will be configured.

// Use mimalloc on Windows
#[cfg(windows)]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// Use jemalloc on supported unix platforms
#[cfg(all(
    not(windows),
    not(target_os = "openbsd"),
    not(target_os = "freebsd"),
    any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "powerpc64"
    )
))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;
