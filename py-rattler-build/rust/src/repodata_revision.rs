use pyo3::prelude::*;
use rattler_conda_types::RepodataRevision;

/// Repodata revision controlling which recipe fields and MatchSpec syntax are
/// accepted by the parser and renderer.
///
/// - ``LEGACY`` (default): Legacy repodata layout with ``packages`` and
///   ``packages.conda`` maps.
/// - ``V3``: Repodata records stored under the top-level ``v3`` map. Enables
///   v3 recipe fields and MatchSpec syntax.
#[pyclass(name = "RepodataRevision", eq, eq_int, from_py_object)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PyRepodataRevision {
    #[default]
    #[pyo3(name = "LEGACY")]
    Legacy,
    #[pyo3(name = "V3")]
    V3,
}

impl From<PyRepodataRevision> for RepodataRevision {
    fn from(value: PyRepodataRevision) -> Self {
        match value {
            PyRepodataRevision::Legacy => RepodataRevision::Legacy,
            PyRepodataRevision::V3 => RepodataRevision::V3,
        }
    }
}

impl From<RepodataRevision> for PyRepodataRevision {
    fn from(value: RepodataRevision) -> Self {
        match value {
            RepodataRevision::Legacy => PyRepodataRevision::Legacy,
            RepodataRevision::V3 => PyRepodataRevision::V3,
            RepodataRevision::Unknown(_) => PyRepodataRevision::Legacy,
        }
    }
}
