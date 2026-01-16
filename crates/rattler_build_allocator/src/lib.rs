//! This crate provides the best memory allocator for different platforms.
//!
//! On Windows, we use mimalloc because it provides good performance.
//! On most Unix platforms (Linux, macOS), we use jemalloc for its excellent
//! performance characteristics with multi-threaded applications.
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
