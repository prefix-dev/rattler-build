//! The ordered mount list of a session.

use std::path::PathBuf;

use super::{Mount, RunnerError};

/// The bind mounts a session must provide, in the order they were added.
///
/// The builder methods append and keep the insertion order, so runners that
/// turn the mounts into arguments produce the same command for the same
/// input. Paths are neither compared nor normalised: a guest path added
/// twice is passed through twice, as given, and a runner that cannot accept
/// that reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mounts(Vec<Mount>);

impl Mounts {
    /// An empty mount list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `path` writable, exposed at the same path in the guest.
    #[must_use]
    pub fn writable(self, path: impl Into<PathBuf>) -> Self {
        self.mount(Mount::mirrored(path, true))
    }

    /// Appends `path` read-only, exposed at the same path in the guest.
    #[must_use]
    pub fn read_only(self, path: impl Into<PathBuf>) -> Self {
        self.mount(Mount::mirrored(path, false))
    }

    /// Appends `mount` as given.
    #[must_use]
    pub fn mount(mut self, mount: Mount) -> Self {
        self.0.push(mount);
        self
    }

    /// Iterates over the mounts in insertion order.
    pub fn iter(&self) -> std::slice::Iter<'_, Mount> {
        self.0.iter()
    }

    /// Checks that every mount is mirrored (host path equal to guest path),
    /// which runners that expose host paths unchanged require. Fails with
    /// [`RunnerError::UnsupportedMount`] naming `runner_name` for the first
    /// mount that is not.
    pub fn require_mirrored(&self, runner_name: &'static str) -> Result<(), RunnerError> {
        match self.iter().find(|mount| mount.host.0 != mount.guest.0) {
            Some(mount) => Err(RunnerError::UnsupportedMount {
                runner: runner_name,
                host: mount.host.clone(),
                guest: mount.guest.clone(),
            }),
            None => Ok(()),
        }
    }
}

impl<'a> IntoIterator for &'a Mounts {
    type Item = &'a Mount;
    type IntoIter = std::slice::Iter<'a, Mount>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{GuestPath, HostPath};

    /// Mounts come out in the order they went in, including a guest path
    /// that was added twice.
    #[test]
    fn mounts_keep_insertion_order() {
        let mounts = Mounts::new()
            .writable("/builds/pkg")
            .read_only("/recipes/pkg")
            .mount(Mount {
                host: HostPath(PathBuf::from("/host/source")),
                guest: GuestPath(PathBuf::from("/guest/source")),
                writable: false,
            })
            .read_only("/builds/pkg");

        let expected = vec![
            Mount::mirrored("/builds/pkg", true),
            Mount::mirrored("/recipes/pkg", false),
            Mount {
                host: HostPath(PathBuf::from("/host/source")),
                guest: GuestPath(PathBuf::from("/guest/source")),
                writable: false,
            },
            Mount::mirrored("/builds/pkg", false),
        ];
        assert_eq!(mounts.iter().cloned().collect::<Vec<_>>(), expected);
        assert_eq!((&mounts).into_iter().cloned().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn require_mirrored_names_the_first_unmirrored_mount() {
        let mirrored = Mounts::new()
            .writable("/builds/pkg")
            .read_only("/recipes/pkg");
        assert!(mirrored.require_mirrored("sandbox").is_ok());
        assert!(Mounts::new().require_mirrored("sandbox").is_ok());

        let unmirrored = mirrored
            .mount(Mount {
                host: HostPath(PathBuf::from("/host/first")),
                guest: GuestPath(PathBuf::from("/guest/first")),
                writable: true,
            })
            .mount(Mount {
                host: HostPath(PathBuf::from("/host/second")),
                guest: GuestPath(PathBuf::from("/guest/second")),
                writable: true,
            });

        match unmirrored.require_mirrored("sandbox") {
            Err(RunnerError::UnsupportedMount {
                runner,
                host,
                guest,
            }) => {
                assert_eq!(runner, "sandbox");
                assert_eq!(host, HostPath(PathBuf::from("/host/first")));
                assert_eq!(guest, GuestPath(PathBuf::from("/guest/first")));
            }
            Err(
                error @ (RunnerError::Io(_)
                | RunnerError::UnsupportedPlatform { .. }
                | RunnerError::MissingExecutable { .. }),
            ) => panic!("unexpected error: {error:?}"),
            Ok(()) => panic!("an unmirrored mount must be rejected"),
        }
    }
}
