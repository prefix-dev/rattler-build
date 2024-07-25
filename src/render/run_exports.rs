use std::collections::HashMap;

use rattler_conda_types::{
    package::RunExportsJson, MatchSpec, PackageName, ParseMatchSpecError, ParseStrictness,
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

impl IgnoreRunExports {
    pub fn filter(
        &self,
        run_export_map: &HashMap<PackageName, RunExportsJson>,
        from_env: &str,
    ) -> Result<FilteredRunExports, ParseMatchSpecError> {
        let mut run_export_map = run_export_map.clone();
        println!("Self: {:?}", self);
        println!("run_export_map: {:?}", run_export_map);
        println!("Retaining self.from_package(): {:?}", self.from_package());
        run_export_map.retain(|name, _| !self.from_package().contains(name));

        let mut filtered_run_exports = FilteredRunExports::default();

        let to_specs = |strings: &Vec<String>| -> Result<Vec<MatchSpec>, ParseMatchSpecError> {
            strings
                .iter()
                .map(|s| MatchSpec::from_str(s, ParseStrictness::Strict))
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
