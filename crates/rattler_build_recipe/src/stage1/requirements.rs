//! Stage 1 Requirements - evaluated dependencies with concrete values

/// Run exports configuration
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunExports {
    /// Noarch run exports
    pub noarch: Vec<String>,

    /// Strong run exports (apply from build and host env to run env)
    pub strong: Vec<String>,

    /// Strong run constraints
    pub strong_constraints: Vec<String>,

    /// Weak run exports (apply from host env to run env)
    pub weak: Vec<String>,

    /// Weak run constraints
    pub weak_constraints: Vec<String>,
}

impl RunExports {
    /// Create a new empty RunExports
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if all fields are empty
    pub fn is_empty(&self) -> bool {
        self.noarch.is_empty()
            && self.strong.is_empty()
            && self.strong_constraints.is_empty()
            && self.weak.is_empty()
            && self.weak_constraints.is_empty()
    }
}

/// Ignore run exports configuration
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IgnoreRunExports {
    /// Packages to ignore run exports from by name
    pub by_name: Vec<String>,

    /// Packages whose run_exports to ignore
    pub from_package: Vec<String>,
}

impl IgnoreRunExports {
    /// Create a new empty IgnoreRunExports
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty() && self.from_package.is_empty()
    }
}

/// Evaluated requirements with all templates and conditionals resolved
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Requirements {
    /// Build-time dependencies (available during build)
    pub build: Vec<String>,

    /// Host dependencies (available during build and runtime)
    pub host: Vec<String>,

    /// Runtime dependencies
    pub run: Vec<String>,

    /// Runtime constraints (optional requirements that constrain the environment)
    pub run_constraints: Vec<String>,

    /// Run exports configuration
    pub run_exports: RunExports,

    /// Ignore run exports from specific packages
    pub ignore_run_exports: IgnoreRunExports,
}

impl Requirements {
    /// Create a new empty Requirements
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if the Requirements section is empty
    pub fn is_empty(&self) -> bool {
        self.build.is_empty()
            && self.host.is_empty()
            && self.run.is_empty()
            && self.run_constraints.is_empty()
            && self.run_exports.is_empty()
            && self.ignore_run_exports.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_requirements_creation() {
        let reqs = Requirements::new();
        assert!(reqs.is_empty());
    }

    #[test]
    fn test_requirements_with_deps() {
        let reqs = Requirements {
            build: vec!["gcc".to_string(), "make".to_string()],
            host: vec!["python".to_string()],
            run: vec!["python".to_string(), "numpy".to_string()],
            ..Default::default()
        };

        assert!(!reqs.is_empty());
        assert_eq!(reqs.build.len(), 2);
        assert_eq!(reqs.host.len(), 1);
        assert_eq!(reqs.run.len(), 2);
    }

    #[test]
    fn test_run_exports_empty() {
        let re = RunExports::new();
        assert!(re.is_empty());

        let re = RunExports {
            weak: vec!["foo".to_string()],
            ..Default::default()
        };
        assert!(!re.is_empty());
    }

    #[test]
    fn test_ignore_run_exports() {
        let ire = IgnoreRunExports::new();
        assert!(ire.is_empty());

        let ire = IgnoreRunExports {
            by_name: vec!["gcc".to_string()],
            ..Default::default()
        };
        assert!(!ire.is_empty());
    }
}
