use std::collections::HashMap;

use rattler_build_recipe::stage1::requirements::IgnoreRunExports;
use rattler_conda_types::{
    MatchSpec, PackageName, ParseMatchSpecError, ParseStrictness, package::RunExportsJson,
};

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

/// Filter run exports based on ignore lists
pub fn filter_run_exports(
    ignore_run_exports: &IgnoreRunExports,
    run_export_map: &HashMap<PackageName, RunExportsJson>,
    from_env: &str,
) -> Result<FilteredRunExports, ParseMatchSpecError> {
    let mut run_export_map = run_export_map.clone();
    run_export_map.retain(|name, _| !ignore_run_exports.from_package.contains(name));

    let mut filtered_run_exports = FilteredRunExports::default();

    let to_specs = |strings: &Vec<String>| -> Result<Vec<MatchSpec>, ParseMatchSpecError> {
        strings
            .iter()
            // We have to parse these as lenient as they come from packages
            .map(|s| MatchSpec::from_str(s, ParseStrictness::Lenient))
            .filter_map(|result| match result {
                Ok(spec) => {
                    let should_include = match spec.name.as_exact() {
                        Some(name) => !ignore_run_exports.by_name.contains(name),
                        None => true, // Include non-exact matchers
                    };
                    if should_include { Some(Ok(spec)) } else { None }
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
        if ignore_run_exports.from_package.contains(name) {
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

/// Filter already-resolved run export dependencies (e.g. inherited from a
/// staging cache) using `ignore_run_exports` rules.
pub fn filter_inherited_run_exports(
    ignore_run_exports: &IgnoreRunExports,
    deps: &[DependencyInfo],
) -> Vec<DependencyInfo> {
    deps.iter()
        .filter(|dep| {
            if let DependencyInfo::RunExport(re) = dep {
                // Check from_package: ignore all run exports originating from
                // a listed source package.
                if ignore_run_exports
                    .from_package
                    .iter()
                    .any(|p| p.as_normalized() == re.source_package)
                {
                    return false;
                }

                // Check by_name: ignore run exports whose dependency name
                // matches one of the listed names.
                if let Some(name) = re.spec.name.as_exact()
                    && ignore_run_exports.by_name.contains(name)
                {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use rattler_build_recipe::stage1::requirements::IgnoreRunExports;
    use rattler_conda_types::{MatchSpec, PackageName, ParseStrictness};

    use super::*;
    use crate::render::resolved_dependencies::{
        DependencyInfo, RunExportDependency, SourceDependency,
    };

    fn make_run_export(spec_str: &str, source: &str) -> DependencyInfo {
        DependencyInfo::RunExport(RunExportDependency {
            spec: MatchSpec::from_str(spec_str, ParseStrictness::Lenient).unwrap(),
            from: "host".to_string(),
            source_package: source.to_string(),
        })
    }

    fn make_source_dep(spec_str: &str) -> DependencyInfo {
        DependencyInfo::Source(SourceDependency {
            spec: MatchSpec::from_str(spec_str, ParseStrictness::Lenient).unwrap(),
        })
    }

    #[test]
    fn test_filter_inherited_by_name() {
        let deps = vec![
            make_run_export("numpy >=1.23,<3", "numpy"),
            make_run_export("python_abi 3.13.* *_cp313", "python"),
            make_run_export("libfoo >=1.0", "libfoo"),
            make_source_dep("some-dep >=2.0"),
        ];

        let ignore = IgnoreRunExports {
            by_name: vec![
                "numpy".parse::<PackageName>().unwrap(),
                "python_abi".parse::<PackageName>().unwrap(),
            ],
            from_package: vec![],
        };

        let filtered = filter_inherited_run_exports(&ignore, &deps);
        assert_eq!(filtered.len(), 2);
        // libfoo run export and the source dep should remain
        assert!(
            matches!(&filtered[0], DependencyInfo::RunExport(re) if re.source_package == "libfoo")
        );
        assert!(matches!(&filtered[1], DependencyInfo::Source(_)));
    }

    #[test]
    fn test_filter_inherited_from_package() {
        let deps = vec![
            make_run_export("numpy >=1.23,<3", "numpy"),
            make_run_export("python_abi 3.13.* *_cp313", "python"),
            make_run_export("libfoo >=1.0", "libfoo"),
        ];

        let ignore = IgnoreRunExports {
            by_name: vec![],
            from_package: vec!["python".parse::<PackageName>().unwrap()],
        };

        let filtered = filter_inherited_run_exports(&ignore, &deps);
        assert_eq!(filtered.len(), 2);
        assert!(
            matches!(&filtered[0], DependencyInfo::RunExport(re) if re.source_package == "numpy")
        );
        assert!(
            matches!(&filtered[1], DependencyInfo::RunExport(re) if re.source_package == "libfoo")
        );
    }

    #[test]
    fn test_filter_inherited_combined() {
        let deps = vec![
            make_run_export("numpy >=1.23,<3", "numpy"),
            make_run_export("python_abi 3.13.* *_cp313", "python"),
            make_run_export("libbar >=2.0", "libfoo"),
        ];

        let ignore = IgnoreRunExports {
            by_name: vec!["numpy".parse::<PackageName>().unwrap()],
            from_package: vec!["libfoo".parse::<PackageName>().unwrap()],
        };

        let filtered = filter_inherited_run_exports(&ignore, &deps);
        // numpy filtered by name, libbar filtered by from_package (libfoo)
        assert_eq!(filtered.len(), 1);
        assert!(
            matches!(&filtered[0], DependencyInfo::RunExport(re) if re.source_package == "python")
        );
    }

    #[test]
    fn test_filter_inherited_no_filters() {
        let deps = vec![
            make_run_export("numpy >=1.23", "numpy"),
            make_source_dep("foo >=1.0"),
        ];

        let ignore = IgnoreRunExports::default();
        let filtered = filter_inherited_run_exports(&ignore, &deps);
        assert_eq!(filtered.len(), 2);
    }
}
