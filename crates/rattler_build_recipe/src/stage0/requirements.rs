//! Stage0 requirements - unrendered requirements with templates and conditionals

use serde::{Deserialize, Serialize};

use super::types::ConditionalList;

/// The requirements section of a stage0 recipe (before rendering).
/// All dependency strings can contain Jinja2 templates and conditional statements.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct Requirements {
    /// Build-time requirements (resolved with build platform)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub build: ConditionalList<String>,

    /// Host-time requirements (resolved with target platform)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub host: ConditionalList<String>,

    /// Run-time requirements (resolved with target platform)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub run: ConditionalList<String>,

    /// Runtime constraints (optional requirements that constrain the environment)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub run_constraints: ConditionalList<String>,

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
        let mut vars = Vec::new();

        vars.extend(self.build.used_variables());
        vars.extend(self.host.used_variables());
        vars.extend(self.run.used_variables());
        vars.extend(self.run_constraints.used_variables());
        vars.extend(self.run_exports.used_variables());
        vars.extend(self.ignore_run_exports.used_variables());

        vars.sort();
        vars.dedup();
        vars
    }
}

/// Run exports configuration (before rendering)
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunExports {
    /// Noarch run exports
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub noarch: ConditionalList<String>,

    /// Strong run exports (apply from build and host env to run env)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub strong: ConditionalList<String>,

    /// Strong run constraints
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub strong_constraints: ConditionalList<String>,

    /// Weak run exports (apply from host env to run env)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub weak: ConditionalList<String>,

    /// Weak run constraints
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub weak_constraints: ConditionalList<String>,
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
        let mut vars = Vec::new();

        vars.extend(self.noarch.used_variables());
        vars.extend(self.strong.used_variables());
        vars.extend(self.strong_constraints.used_variables());
        vars.extend(self.weak.used_variables());
        vars.extend(self.weak_constraints.used_variables());

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
    pub by_name: ConditionalList<String>,

    /// Packages whose run_exports to ignore (can contain templates/conditionals)
    #[serde(default, skip_serializing_if = "ConditionalList::is_empty")]
    pub from_package: ConditionalList<String>,
}

impl IgnoreRunExports {
    /// Check if both fields are empty
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty() && self.from_package.is_empty()
    }

    /// Collect all variables used in ignore configuration
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();

        vars.extend(self.by_name.used_variables());
        vars.extend(self.from_package.used_variables());

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
            Item::Value(Value::Concrete("gcc".to_string())),
            Item::Value(Value::Template(
                JinjaTemplate::new("${{ compiler('c') }}".to_string()).unwrap(),
            )),
        ];

        let req = Requirements {
            build: ConditionalList::new(items),
            ..Default::default()
        };

        assert!(!req.is_empty());
        let vars = req.used_variables();
        assert_eq!(vars, vec!["compiler"]);
    }

    #[test]
    fn test_requirements_collect_all_variables() {
        let build_items = vec![Item::Value(Value::Template(
            JinjaTemplate::new("${{ compiler }}".to_string()).unwrap(),
        ))];

        let run_items = vec![Item::Value(Value::Template(
            JinjaTemplate::new("${{ python }}".to_string()).unwrap(),
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
}
