use rattler_conda_types::{RepoDataRecord, MatchSpec};

/// An enum for the different sources of dependencies
enum DependencySource {
    /// This dependency is a direct dependency of the recipe
    Recipe(String),
    /// The dependency comes from the recipe + applied variant
    Variant(String),
    /// This dependency is a run export from another package
    RunExport(String),
    /// This dependency is a pin-subpackage from the same recipe
    PinSubpackage(String),
    /// This dependency is a pin compatible from the same recipe
    PinCompatible(String),
}

/// A struct for a given dependency, containing the final resolved version
struct Dependency {
    source: DependencySource,
    matchspec: MatchSpec,
    selected: Option<RepoDataRecord>,
}

/// A struct for the different dependency environments
struct RenderedDependencies {
    build: Vec<Dependency>,
    host: Vec<Dependency>,
    run: Vec<Dependency>,
    run_constrains: Vec<Dependency>,
    test: Vec<Dependency>,
}
