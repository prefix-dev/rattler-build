//! Stage0 requirements - unrendered requirements with templates and conditionals

use rattler_conda_types::PackageName;
use serde::{Deserialize, Serialize};

use crate::stage0::SerializableMatchSpec;

use super::types::ConditionalList;

/// The requirements section of a stage0 recipe (before rendering).
/// All dependency strings can contain Jinja2 templates and conditional statements.
/// Non-template strings are validated as MatchSpec during parsing.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct Requirements {
    /// Build-time requirements (resolved with build platform)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub build: ConditionalList<SerializableMatchSpec>,

    /// Host-time requirements (resolved with target platform)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub host: ConditionalList<SerializableMatchSpec>,

    /// Run-time requirements (resolved with target platform)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub run: ConditionalList<SerializableMatchSpec>,

    /// Runtime constraints (optional requirements that constrain the environment)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub run_constraints: ConditionalList<SerializableMatchSpec>,

    /// Run exports configuration
    #[serde(default, skip_serializing_if = "RunExports::is_empty")]
    pub run_exports: RunExports,

    /// Ignore run-exports configuration
    #[serde(default, skip_serializing_if = "IgnoreRunExports::is_empty")]
    pub ignore_run_exports: IgnoreRunExports,
}

impl Requirements {
    /// Check if all requirements are empty
    pub fn is_empty(&self) -> bool {
        self.build.is_empty()
            && self.host.is_empty()
            && self.run.is_empty()
            && self.run_constraints.is_empty()
            && self.run_exports.is_empty()
            && self.ignore_run_exports.is_empty()
    }

    /// Collect all variables used in requirements
    pub fn used_variables(&self) -> Vec<String> {
        let Requirements {
            build,
            host,
            run,
            run_constraints,
            run_exports,
            ignore_run_exports,
        } = self;

        let mut vars = Vec::new();

        vars.extend(build.used_variables());
        vars.extend(host.used_variables());
        vars.extend(run.used_variables());
        vars.extend(run_constraints.used_variables());
        vars.extend(run_exports.used_variables());
        vars.extend(ignore_run_exports.used_variables());

        vars.sort();
        vars.dedup();
        vars
    }

    /// Find all matchspecs that are free in `build` and `host` (i.e. do not have a version or build constraint)
    /// These are also used as "variants" in the build system.
    /// Note: since this is before rendering, we consider both branches of conditionals (then and else)
    pub fn free_specs(&self) -> Vec<PackageName> {
        use rattler_conda_types::PackageNameMatcher;

        let mut specs = Vec::new();

        // Helper to extract PackageName from PackageNameMatcher
        let extract_name = |matcher: &PackageNameMatcher| -> Option<PackageName> {
            match matcher {
                PackageNameMatcher::Exact(name) => Some(name.clone()),
                _ => None,
            }
        };

        // Recursive helper to process items (supports nested conditionals)
        fn process_item(
            item: &super::types::Item<SerializableMatchSpec>,
            specs: &mut Vec<PackageName>,
            extract_name: impl Fn(&rattler_conda_types::PackageNameMatcher) -> Option<PackageName>
            + Copy,
        ) {
            match item {
                super::types::Item::Value(value) => {
                    // Only process concrete (non-template) values
                    if let Some(val) = value.as_concrete() {
                        let matchspec = &val.0;

                        // A spec is "free" if it has no version and no build constraints
                        if matchspec.version.is_none()
                            && matchspec.build.is_none()
                            && let Some(name) = &matchspec.name
                            && let Some(pkg_name) = extract_name(name)
                        {
                            specs.push(pkg_name);
                        }
                    }
                }
                super::types::Item::Conditional(conditional) => {
                    // Recursively process both then and else branches
                    for nested_item in conditional.then.iter() {
                        process_item(nested_item, specs, extract_name);
                    }
                    if let Some(else_branch) = &conditional.else_value {
                        for nested_item in else_branch.iter() {
                            process_item(nested_item, specs, extract_name);
                        }
                    }
                }
            }
        }

        for item in self.build.iter().chain(self.host.iter()) {
            process_item(item, &mut specs, extract_name);
        }

        specs.sort();
        specs.dedup();
        specs
    }
}

/// Run exports configuration (before rendering)
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunExports {
    /// Noarch run exports
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub noarch: ConditionalList<SerializableMatchSpec>,

    /// Strong run exports (apply from build and host env to run env)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub strong: ConditionalList<SerializableMatchSpec>,

    /// Strong run constraints
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub strong_constraints: ConditionalList<SerializableMatchSpec>,

    /// Weak run exports (apply from host env to run env)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub weak: ConditionalList<SerializableMatchSpec>,

    /// Weak run constraints
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub weak_constraints: ConditionalList<SerializableMatchSpec>,
}

impl RunExports {
    /// Check if all fields are empty
    pub fn is_empty(&self) -> bool {
        self.noarch.is_empty()
            && self.strong.is_empty()
            && self.strong_constraints.is_empty()
            && self.weak.is_empty()
            && self.weak_constraints.is_empty()
    }

    /// Collect all variables used in run exports
    pub fn used_variables(&self) -> Vec<String> {
        let RunExports {
            noarch,
            strong,
            strong_constraints,
            weak,
            weak_constraints,
        } = self;

        let mut vars = Vec::new();

        vars.extend(noarch.used_variables());
        vars.extend(strong.used_variables());
        vars.extend(strong_constraints.used_variables());
        vars.extend(weak.used_variables());
        vars.extend(weak_constraints.used_variables());

        vars.sort();
        vars.dedup();
        vars
    }
}

/// Configuration for ignoring run-exports (before rendering)
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct IgnoreRunExports {
    /// Package names to ignore (can contain templates/conditionals)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub by_name: ConditionalList<PackageName>,

    /// Packages whose run_exports to ignore (can contain templates/conditionals)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub from_package: ConditionalList<PackageName>,
}

impl IgnoreRunExports {
    /// Check if both fields are empty
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty() && self.from_package.is_empty()
    }

    /// Collect all variables used in ignore configuration
    pub fn used_variables(&self) -> Vec<String> {
        let IgnoreRunExports {
            by_name,
            from_package,
        } = self;

        let mut vars = Vec::new();

        vars.extend(by_name.used_variables());
        vars.extend(from_package.used_variables());

        vars.sort();
        vars.dedup();
        vars
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage0::types::{Item, JinjaTemplate, Value};

    #[test]
    fn test_requirements_empty() {
        let req = Requirements::default();
        assert!(req.is_empty());
        assert_eq!(req.used_variables(), Vec::<String>::new());
    }

    #[test]
    fn test_requirements_with_build_deps() {
        let items = vec![
            Item::Value(Value::new_concrete(
                SerializableMatchSpec::from("gcc"),
                None,
            )),
            Item::Value(Value::new_template(
                JinjaTemplate::new("${{ compiler('c') }}".to_string()).unwrap(),
                None,
            )),
        ];

        let req = Requirements {
            build: ConditionalList::new(items),
            ..Default::default()
        };

        assert!(!req.is_empty());
        let vars = req.used_variables();
        assert_eq!(vars, vec!["c_compiler", "c_compiler_version"]);
    }

    #[test]
    fn test_requirements_collect_all_variables() {
        let build_items = vec![Item::Value(Value::new_template(
            JinjaTemplate::new("${{ compiler }}".to_string()).unwrap(),
            None,
        ))];

        let run_items = vec![Item::Value(Value::new_template(
            JinjaTemplate::new("${{ python }}".to_string()).unwrap(),
            None,
        ))];

        let req = Requirements {
            build: ConditionalList::new(build_items),
            run: ConditionalList::new(run_items),
            ..Default::default()
        };

        let mut vars = req.used_variables();
        vars.sort();
        assert_eq!(vars, vec!["compiler", "python"]);
    }

    #[test]
    fn test_run_exports_empty() {
        let exports = RunExports::default();
        assert!(exports.is_empty());
        assert_eq!(exports.used_variables(), Vec::<String>::new());
    }

    #[test]
    fn test_ignore_run_exports_empty() {
        let ignore = IgnoreRunExports::default();
        assert!(ignore.is_empty());
        assert_eq!(ignore.used_variables(), Vec::<String>::new());
    }

    #[test]
    fn test_requirements_serialize_deserialize() {
        // Create requirements with parsed matchspecs and templates
        let items = vec![
            Item::Value(Value::new_concrete(
                SerializableMatchSpec::from("python >=3.8"),
                None,
            )),
            Item::Value(Value::new_template(
                JinjaTemplate::new("cuda-toolkit ${{ cuda_version }}".to_string()).unwrap(),
                None,
            )),
            Item::Value(Value::new_template(
                JinjaTemplate::new("${{ compiler('c') }}".to_string()).unwrap(),
                None,
            )),
        ];

        let req = Requirements {
            build: ConditionalList::new(items),
            ..Default::default()
        };

        // Serialize to YAML
        let yaml = serde_yaml::to_string(&req).unwrap();

        // Verify serialized format is clean and readable
        assert!(yaml.contains("python >=3.8"));
        assert!(yaml.contains("cuda-toolkit ${{ cuda_version }}"));
        assert!(yaml.contains("${{ compiler('c') }}"));

        // Deserialize back
        let deserialized: Requirements = serde_yaml::from_str(&yaml).unwrap();

        // Verify we can access the fields correctly
        assert_eq!(deserialized.build.len(), 3);

        // Verify serialized format is preserved on re-serialization
        let yaml2 = serde_yaml::to_string(&deserialized).unwrap();
        assert!(yaml2.contains("python >=3.8"));
        assert!(yaml2.contains("cuda-toolkit ${{ cuda_version }}"));
        assert!(yaml2.contains("${{ compiler('c') }}"));
    }

    #[test]
    fn test_requirements_serialize_with_deferred() {
        // Test that Deferred matchspecs serialize correctly (even though they deserialize as Templates)
        let yaml = r#"
build:
  - python >=3.8
  - cuda-toolkit ${{ cuda_version }}
  - gcc
"#;

        let req: Requirements = serde_yaml::from_str(yaml).unwrap();

        // Should parse successfully - templates and concrete matchspecs
        assert_eq!(req.build.len(), 3);

        // Re-serialize and verify it's clean
        let reserialized = serde_yaml::to_string(&req).unwrap();
        assert!(reserialized.contains("python >=3.8"));
        assert!(reserialized.contains("cuda-toolkit ${{ cuda_version }}"));
        assert!(reserialized.contains("gcc"));
    }
}
