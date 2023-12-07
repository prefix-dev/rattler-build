use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    _partialerror,
    recipe::{
        custom_yaml::{HasSpan, RenderedMappingNode, RenderedNode, TryConvertNode},
        error::{ErrorKind, PartialParsingError},
    },
};

/// Format for requirements in form of build and run
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Requirement {
    /// Build time requirement for tests
    build: Vec<String>,
    /// Runtime requirement for tests
    run: Vec<String>,
}
impl Requirement {
    pub const fn empty() -> Self {
        Requirement {
            build: Vec::new(),
            run: Vec::new(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.build.is_empty() && self.run.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Files {
    source: Vec<String>,
    recipe: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Python {
    imports: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AsScript {
    String(String),
    Strings(Vec<String>),
    Script(Script),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Script {
    interpreter: String,
    env: BTreeMap<String, String>,
    secrets: Vec<String>,
    file: Option<String>,
    content: Content,
}
impl Default for Script {
    fn default() -> Self {
        Self {
            interpreter: if cfg!(not(target_os = "windows")) {
                "sh".to_string()
            } else {
                "cmd.exe".to_string()
            },
            env: Default::default(),
            secrets: Default::default(),
            file: Some("default_test".to_string()),
            content: Content::Src(String::new()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Src(String),
    Page(Vec<String>),
}

/// Define tests in your recipe that are executed after successfully building the package.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Test {
    script: Option<AsScript>,
    /// Try importing a python module as a sanity check
    python: Option<Python>,
    /// Run a list of given commands
    commands: Vec<String>,
    /// Extra requirements to be installed at test time
    requirements: Option<Requirement>,
    /// Extra files to be copied to the test environment from the build dir (can be globs)
    files: Option<Files>,
    /// Match against items in built package.
    package_contents: Option<PackageContent>,
    /// Packages to be tested against this package
    downstream: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// PackageContent provides skeleton for testing for file and artifact match against
/// files and artifacts inside built the package
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
    pub fn package_content(&self) -> Option<&PackageContent> {
        self.package_contents.as_ref()
    }

    /// Get the imports.
    pub fn python_imports(&self) -> &[String] {
        static EMPTY: &[String] = &[];
        match &self.python {
            Some(py) => py.imports.as_slice(),
            None => &EMPTY,
        }
    }

    /// Get the commands.
    pub fn commands(&self) -> &[String] {
        self.commands.as_slice()
    }

    /// Get the requires.
    pub fn requirements<'a>(&'a self) -> &'a Requirement {
        static EMPTY: Requirement = Requirement::empty();
        match &self.requirements {
            Some(reqs) => reqs,
            None => &EMPTY,
        }
    }

    /// Get the source files.
    pub fn source_files(&self) -> &[String] {
        static EMPTY: &[String] = &[];
        match &self.files {
            Some(files) => files.source.as_slice(),
            None => &EMPTY,
        }
    }

    /// Get the files.
    pub fn recipe_files(&self) -> &[String] {
        static EMPTY: &[String] = &[];
        match &self.files {
            Some(f) => f.recipe.as_slice(),
            None => &EMPTY,
        }
    }

    /// Check if there is not test commands to be run
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Get the Downstream package MatchSpec
    pub fn downstream(&self) -> &[String] {
        &self.downstream
    }
}

impl TryConvertNode<Test> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Test, PartialParsingError> {
        match self {
            RenderedNode::Mapping(map) => map.try_convert(name),
            RenderedNode::Scalar(_) => Err(_partialerror!(
                *self.span(),
                ErrorKind::ExpectedMapping,
                help = format!("expected mapping for {name}")
            ))?,
            RenderedNode::Null(_) => Ok(Test::default()),
            RenderedNode::Sequence(_) => Err(_partialerror!(
                *self.span(),
                ErrorKind::ExpectedMapping,
                help = format!("expected mapping for {name}")
            ))?,
        }
    }
}

impl TryConvertNode<Requirement> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<Requirement, PartialParsingError> {
        let mut req = Requirement::default();
        for (k, v) in self.iter() {
            let ks = k.as_str();
            match ks {
                "build" => req.build = v.try_convert(ks)?,
                "run" => req.run = v.try_convert(ks)?,
                invalid => Err(_partialerror!(
                    *k.span(),
                    ErrorKind::InvalidField(invalid.to_string().into()),
                    help = format!("expected fields for {name} is `build` and `run`")
                ))?,
            }
        }
        Ok(req)
    }
}
impl TryConvertNode<Files> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<Files, PartialParsingError> {
        let mut files = Files::default();
        for (k, v) in self.iter() {
            let ks = k.as_str();
            match ks {
                "source" => files.source = v.try_convert(ks)?,
                "recipe" => files.recipe = v.try_convert(ks)?,
                invalid => Err(_partialerror!(
                    *k.span(),
                    ErrorKind::InvalidField(invalid.to_string().into()),
                    help = format!("expected fields for {name} is `source` and `recipe`")
                ))?,
            }
        }
        Ok(files)
    }
}
impl TryConvertNode<Python> for RenderedMappingNode {
    fn try_convert(&self, name: &str) -> Result<Python, PartialParsingError> {
        let mut python = Python::default();
        for (k, v) in self.iter() {
            let ks = k.as_str();
            match ks {
                "imports" => python.imports = v.try_convert(ks)?,
                invalid => Err(_partialerror!(
                    *k.span(),
                    ErrorKind::InvalidField(invalid.to_string().into()),
                    help = format!("expected fields for {name} is `imports`")
                ))?,
            }
        }
        Ok(python)
    }
}

impl TryConvertNode<Requirement> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Requirement, PartialParsingError> {
        match self {
            RenderedNode::Scalar(_) => Err(_partialerror!(
                *self.span(),
                ErrorKind::ExpectedMapping,
                label = "python accepts only `build` and `run` field(s)"
            ))?,
            RenderedNode::Mapping(map) => map.try_convert(name),
            RenderedNode::Sequence(_) => Err(_partialerror!(
                *self.span(),
                ErrorKind::ExpectedMapping,
                label = "python accepts only `build` and `run` field(s)"
            ))?,
            RenderedNode::Null(_) => Err(_partialerror!(
                *self.span(),
                ErrorKind::ExpectedMapping,
                label = "python accepts only `build` and `run` field(s)"
            ))?,
        }
    }
}
impl TryConvertNode<Files> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Files, PartialParsingError> {
        match self {
            RenderedNode::Scalar(_) => Err(_partialerror!(
                *self.span(),
                ErrorKind::ExpectedMapping,
                label = "python accepts only `source` and `recipe` field(s)"
            ))?,
            RenderedNode::Mapping(map) => map.try_convert(name),
            RenderedNode::Sequence(_) => Err(_partialerror!(
                *self.span(),
                ErrorKind::ExpectedMapping,
                label = "python accepts only `source` and `recipe` field(s)"
            ))?,
            RenderedNode::Null(_) => Err(_partialerror!(
                *self.span(),
                ErrorKind::ExpectedMapping,
                label = "python accepts only `source` and `recipe` field(s)"
            ))?,
        }
    }
}
impl TryConvertNode<Python> for RenderedNode {
    fn try_convert(&self, name: &str) -> Result<Python, PartialParsingError> {
        match self {
            RenderedNode::Scalar(_) => Err(_partialerror!(
                *self.span(),
                ErrorKind::ExpectedMapping,
                label = "python accepts only `imports` field(s)"
            ))?,
            RenderedNode::Mapping(map) => map.try_convert(name),
            RenderedNode::Sequence(_) => Err(_partialerror!(
                *self.span(),
                ErrorKind::ExpectedMapping,
                label = "python accepts only `imports` field(s)"
            ))?,
            RenderedNode::Null(_) => Err(_partialerror!(
                *self.span(),
                ErrorKind::ExpectedMapping,
                label = "python accepts only `imports` field(s)"
            ))?,
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
                "python" => test.python = value.try_convert(key_str)?,
                "commands" => test.commands = value.try_convert(key_str)?,
                "requirements" => test.requirements = value.try_convert(key_str)?,
                "files" => test.files = value.try_convert(key_str)?,
                "downstream" => test.downstream = value.try_convert(key_str)?,
                invalid => Err(_partialerror!(
                    *key.span(),
                    ErrorKind::InvalidField(invalid.to_string().into()),
                    help = format!("expected fields for {name} is one of `imports`, `commands`, `requires`, `downstream`, `source_files`, `files`, `package_contents`")
                ))?
            }
        }
        Ok(test)
    }
}
