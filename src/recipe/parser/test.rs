use serde::{Deserialize, Serialize};

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{HasSpan, RenderedMappingNode, RenderedNode, TryConvertNode},
        error::{ErrorKind, PartialParsingError},
    },
};

/// Define tests in your recipe that are executed after successfully building the package.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Test {
    /// Try importing a python module as a sanity check
    imports: Vec<String>,
    /// Run a list of given commands
    commands: Vec<String>,
    /// Extra requirements to be installed at test time
    requires: Vec<String>,
    /// Extra files to be copied to the test environment from the source dir (can be globs)
    source_files: Vec<String>,
    /// Extra files to be copied to the test environment from the build dir (can be globs)
    files: Vec<String>,
    /// <!-- TODO: use a better name: --> All new test section
    package_contents: PackageContent,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// PackageContent
pub struct PackageContent {
    /// file paths, direct and/or globs
    files: Vec<String>,
    /// checks existence of package init in env python site packages dir
    /// eg: mamba.api -> ${SITE_PACKAGES}/mamba/api/__init__.py
    site_packages: Vec<String>,
    /// search for binary in prefix path: eg, %PREFIX%/bin/mamba
    bins: Vec<String>,
    /// check for dynamic or static library file path
    libs: Vec<String>,
    /// check if include path contains the file, direct or glob?
    includes: Vec<String>,
}

impl PackageContent {
    /// Get the package files.
    pub fn files(&self) -> &[String] {
        &self.files
    }

    /// Get the site_packages.
    pub fn site_packages(&self) -> &[String] {
        &self.site_packages
    }

    /// Get the binaries.
    pub fn bins(&self) -> &[String] {
        &self.bins
    }

    /// Get the libraries.
    pub fn libs(&self) -> &[String] {
        &self.libs
    }

    /// Get the includes.
    pub fn includes(&self) -> &[String] {
        &self.includes
    }
}

impl TryConvertNode<PackageContent> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<PackageContent, PartialParsingError> {
        match self {
            RenderedNode::Mapping(map) => map.try_convert(name),
            RenderedNode::Sequence(_) | RenderedNode::Scalar(_) => {
                Err(_partialerror!(*self.span(), ErrorKind::ExpectedMapping,))?
            }
            RenderedNode::Null(_) => Ok(PackageContent::default()),
        }
    }
}

impl TryConvertNode<PackageContent> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<PackageContent, PartialParsingError> {
        let mut files = vec![];
        let mut site_packages = vec![];
        let mut libs = vec![];
        let mut bins = vec![];
        let mut includes = vec![];
        for (key, value) in self.iter() {
            let key_str = key.as_str();
            match key_str {
                "files" => files = value.try_convert(key_str)?,
                "site_packages" => site_packages = value.try_convert(key_str)?,
                "libs" => libs = value.try_convert(key_str)?,
                "bins" => bins = value.try_convert(key_str)?,
                "includes" => includes = value.try_convert(key_str)?,
                invalid => Err(_partialerror!(
                    *key.span(),
                    ErrorKind::InvalidField(invalid.to_string().into()),
                    help = format!("expected fields for {name} is one of `files`, `site_packages`, `libs`, `bins`, `includes`")
                ))?
            }
        }
        Ok(PackageContent {
            files,
            site_packages,
            bins,
            libs,
            includes,
        })
    }
}

impl Test {
    /// Get package content.
    pub fn package_content(&self) -> &PackageContent {
        &self.package_contents
    }

    /// Get the imports.
    pub fn imports(&self) -> &[String] {
        self.imports.as_slice()
    }

    /// Get the commands.
    pub fn commands(&self) -> &[String] {
        self.commands.as_slice()
    }

    /// Get the requires.
    pub fn requires(&self) -> &[String] {
        self.requires.as_slice()
    }

    /// Get the source files.
    pub fn source_files(&self) -> &[String] {
        self.source_files.as_slice()
    }

    /// Get the files.
    pub fn files(&self) -> &[String] {
        self.files.as_slice()
    }

    /// Check if there is not test commands to be run
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

impl TryConvertNode<Test> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Test, PartialParsingError> {
        match self {
            RenderedNode::Mapping(map) => map.try_convert(name),
            RenderedNode::Scalar(_) => {
                Err(_partialerror!(*self.span(), ErrorKind::ExpectedMapping,))?
            }
            RenderedNode::Null(_) => Ok(Test::default()),
            RenderedNode::Sequence(_) => todo!("Not implemented yet: sequence on Test"),
        }
    }
}

impl TryConvertNode<Test> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<Test, PartialParsingError> {
        let mut test = Test::default();

        for (key, value) in self.iter() {
            let key_str = key.as_str();
            match key_str {
                "package_contents" => test.package_contents = value.try_convert(key_str)?,
                "imports" => test.imports = value.try_convert(key_str)?,
                "commands" => test.commands = value.try_convert(key_str)?,
                "requires" => test.requires = value.try_convert(key_str)?,
                "source_files" => test.source_files = value.try_convert(key_str)?,
                "files" => test.files = value.try_convert(key_str)?,
                invalid => Err(_partialerror!(
                    *key.span(),
                    ErrorKind::InvalidField(invalid.to_string().into()),
                    help = format!("expected fields for {name} is one of `imports`, `commands`, `requires`, `source_files`, `files`")
                ))?
            }
        }

        Ok(test)
    }
}
