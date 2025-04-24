use std::collections::HashMap;

use rattler_conda_types::{
    MatchSpec, PackageName, ParseMatchSpecError, ParseStrictness, package::RunExportsJson,
};

use crate::recipe::parser::IgnoreRunExports;

use super::resolved_dependencies::{DependencyInfo, RunExportDependency};

/// Filtered run export result
#[derive(Debug, Default, Clone)]
pub struct FilteredRunExports {
    pub noarch: Vec<DependencyInfo>,
    pub strong: Vec<DependencyInfo>,
    pub strong_constraints: Vec<DependencyInfo>,
    pub weak: Vec<DependencyInfo>,
    pub weak_constraints: Vec<DependencyInfo>,
}

impl FilteredRunExports {
    /// Extend the current filtered run exports with another set of filtered run exports
    pub fn extend(&mut self, other: &FilteredRunExports) {
        self.noarch.extend(other.noarch.iter().cloned());
        self.strong.extend(other.strong.iter().cloned());
        self.strong_constraints
            .extend(other.strong_constraints.iter().cloned());
        self.weak.extend(other.weak.iter().cloned());
        self.weak_constraints
            .extend(other.weak_constraints.iter().cloned());
    }
}

impl IgnoreRunExports {
    pub fn filter(
        &self,
        run_export_map: &HashMap<PackageName, RunExportsJson>,
        from_env: &str,
    ) -> Result<FilteredRunExports, ParseMatchSpecError> {
        let mut run_export_map = run_export_map.clone();
        run_export_map.retain(|name, _| !self.from_package().contains(name));

        let mut filtered_run_exports = FilteredRunExports::default();

        let to_specs = |strings: &Vec<String>| -> Result<Vec<MatchSpec>, ParseMatchSpecError> {
            strings
                .iter()
                // We have to parse these as lenient as they come from packages
                .map(|s| MatchSpec::from_str(s, ParseStrictness::Lenient))
                .filter_map(|result| match result {
                    Ok(spec) => {
                        if spec
                            .name
                            .as_ref()
                            .map(|n| !self.by_name().contains(n))
                            .unwrap_or(false)
                        {
                            Some(Ok(spec))
                        } else {
                            None
                        }
                    }
                    Err(e) => Some(Err(e)),
                })
                .collect()
        };

        let as_dependency = |spec: MatchSpec, name: &PackageName| -> DependencyInfo {
            DependencyInfo::RunExport(RunExportDependency {
                spec,
                from: from_env.to_string(),
                source_package: name.as_normalized().to_string(),
            })
        };

        for (name, run_export) in run_export_map.iter() {
            if self.from_package().contains(name) {
                continue;
            }

            filtered_run_exports.noarch.extend(
                to_specs(&run_export.noarch)?
                    .into_iter()
                    .map(|d| as_dependency(d, name)),
            );
            filtered_run_exports.strong.extend(
                to_specs(&run_export.strong)?
                    .into_iter()
                    .map(|d| as_dependency(d, name)),
            );
            filtered_run_exports.strong_constraints.extend(
                to_specs(&run_export.strong_constrains)?
                    .into_iter()
                    .map(|d| as_dependency(d, name)),
            );
            filtered_run_exports.weak.extend(
                to_specs(&run_export.weak)?
                    .into_iter()
                    .map(|d| as_dependency(d, name)),
            );
            filtered_run_exports.weak_constraints.extend(
                to_specs(&run_export.weak_constrains)?
                    .into_iter()
                    .map(|d| as_dependency(d, name)),
            );
        }

        Ok(filtered_run_exports)
    }
}
