use std::{collections::BTreeMap, str::FromStr};

use minijinja::Value;
use rattler_conda_types::{package::EntryPoint, NoArchKind, NoArchType, PackageName};
use serde::Serialize;

use crate::{
    _partialerror,
    recipe::{
        error::{ErrorKind, PartialParsingError},
        jinja::Jinja,
        stage1::{
            self,
            node::{MappingNode, SequenceNodeInternal},
            Node,
        },
    },
};

use super::Dependency;

/// The build options contain information about how to build the package and some additional
/// metadata about the package.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Build {
    /// The build number is a number that should be incremented every time the recipe is built.
    pub(super) number: u64,
    /// The build string is usually set automatically as the hash of the variant configuration.
    /// It's possible to override this by setting it manually, but not recommended.
    pub(super) string: Option<String>,
    /// List of conditions under which to skip the build of the package.
    pub(super) skip: Vec<Value>,
    /// The build script can be either a list of commands or a path to a script. By
    /// default, the build script is set to `build.sh` or `build.bat` on Unix and Windows respectively.
    pub(super) script: Vec<String>,
    /// Environment variables to pass through or set in the script
    pub(super) script_env: ScriptEnv,
    /// A recipe can choose to ignore certain run exports of its dependencies
    pub(super) ignore_run_exports: Vec<PackageName>,
    /// A recipe can choose to ignore all run exports of coming from some packages
    pub(super) ignore_run_exports_from: Vec<PackageName>,
    /// The recipe can specify a list of run exports that it provides
    pub(super) run_exports: RunExports,
    /// A noarch package runs on any platform. It can be either a python package or a generic package.
    pub(super) noarch: NoArchType,
    /// For a Python noarch package to have executables it is necessary to specify the python entry points.
    /// These contain the name of the executable and the module + function that should be executed.
    pub(super) entry_points: Vec<EntryPoint>,
    // TODO: Add and parse the rest of the fields
}

impl Build {
    pub(super) fn from_stage1(
        build: &stage1::Build,
        jinja: &Jinja,
    ) -> Result<Self, PartialParsingError> {
        Ok(build
            .node
            .as_ref()
            .map(|node| Self::from_node(node, jinja))
            .transpose()?
            .unwrap_or_default())
    }

    fn from_node(node: &MappingNode, jinja: &Jinja) -> Result<Self, PartialParsingError> {
        let mut build = Self::default();

        for (key, value) in node.iter() {
            match key.as_str() {
                "number" => {
                    let number = value.as_scalar().ok_or_else(|| {
                        _partialerror!(*value.span(), ErrorKind::Other, label = "expected scalar")
                    })?;
                    let number = jinja.render_str(number.as_str()).map_err(|err| {
                        _partialerror!(
                            *number.span(),
                            ErrorKind::JinjaRendering(err),
                            label = "error rendering number"
                        )
                    })?;
                    let number = number.parse::<u64>().map_err(|_err| {
                        _partialerror!(
                            *value.span(),
                            ErrorKind::Other,
                            label = "error parsing number"
                        )
                    })?;
                    build.number = number;
                }
                "string" => {
                    let string = value.as_scalar().ok_or_else(|| {
                        _partialerror!(*value.span(), ErrorKind::Other, label = "expected scalar")
                    })?;
                    let string = jinja.render_str(string.as_str()).map_err(|err| {
                        _partialerror!(
                            *string.span(),
                            ErrorKind::JinjaRendering(err),
                            label = "error rendering string"
                        )
                    })?;
                    build.string = Some(string);
                }
                "skip" => build.skip = parse_skip(value, jinja)?,
                "script" => build.script = parse_script(value, jinja)?,
                "script_env" => build.script_env = ScriptEnv::from_node(value, jinja)?,
                "ignore_run_exports" => {
                    build.ignore_run_exports = parse_ignore_run_exports(value, jinja)?;
                }
                "ignore_run_exports_from" => {
                    // Abuse parse_ignore_run_exports since in structure they are the same
                    // We may want to change this in the future for better error messages.
                    build.ignore_run_exports_from = parse_ignore_run_exports(value, jinja)?;
                }
                "noarch" => {
                    let noarch = value.as_scalar().ok_or_else(|| {
                        _partialerror!(*value.span(), ErrorKind::Other, label = "expected scalar")
                    })?;
                    let noarch = jinja.render_str(noarch.as_str()).map_err(|err| {
                        _partialerror!(
                            *noarch.span(),
                            ErrorKind::JinjaRendering(err),
                            label = "error rendering noarch"
                        )
                    })?;
                    let noarch = match noarch.as_str() {
                        "python" => NoArchType::python(),
                        "generic" => NoArchType::generic(),
                        _ => {
                            return Err(_partialerror!(
                                *value.span(),
                                ErrorKind::Other,
                                label = "expected `python` or `generic`"
                            ))
                        }
                    };
                    build.noarch = noarch;
                }
                "run_exports" => {
                    build.run_exports = RunExports::from_node(value, jinja)?;
                }
                "entry_points" => {
                    if let Some(NoArchKind::Generic) = build.noarch.kind() {
                        return Err(_partialerror!(
                            *key.span(),
                            ErrorKind::Other,
                            label = "entry_points are only allowed for python noarch packages"
                        ));
                    }

                    build.entry_points = parse_entry_points(value, jinja)?;
                }
                _ => unimplemented!("unimplemented field: {}", key.as_str()),
            }
        }

        Ok(build)
    }

    /// Get the build number.
    pub const fn number(&self) -> u64 {
        self.number
    }

    /// Get the build string.
    pub fn string(&self) -> Option<&str> {
        self.string.as_deref()
    }

    /// Get the skip conditions.
    pub fn skip(&self) -> &[Value] {
        self.skip.as_slice()
    }

    /// Get the build script.
    pub fn scripts(&self) -> &[String] {
        self.script.as_slice()
    }

    /// Get the build script environment.
    pub const fn script_env(&self) -> &ScriptEnv {
        &self.script_env
    }

    /// Get run exports.
    pub const fn run_exports(&self) -> &RunExports {
        &self.run_exports
    }

    /// Get the ignore run exports.
    ///
    /// A recipe can choose to ignore certain run exports of its dependencies
    pub fn ignore_run_exports(&self) -> &[PackageName] {
        self.ignore_run_exports.as_slice()
    }

    /// Get the ignore run exports from.
    ///
    /// A recipe can choose to ignore all run exports of coming from some packages
    pub fn ignore_run_exports_from(&self) -> &[PackageName] {
        self.ignore_run_exports_from.as_slice()
    }

    /// Get the noarch type.
    pub const fn noarch(&self) -> &NoArchType {
        &self.noarch
    }

    /// Get the entry points.
    pub fn entry_points(&self) -> &[EntryPoint] {
        self.entry_points.as_slice()
    }

    /// Check if the build should be skipped.
    pub fn is_skip_build(&self) -> bool {
        !self.skip.is_empty() && self.skip.iter().any(|v| v.is_true())
    }
}

fn parse_skip(node: &Node, jinja: &Jinja) -> Result<Vec<Value>, PartialParsingError> {
    match node {
        Node::Scalar(s) => {
            let skip = jinja.eval(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error evaluating `skip` expression"
                )
            })?;
            Ok(vec![skip])
        }
        Node::Sequence(seq) => {
            let mut skip = Vec::new();
            for inner in seq.iter() {
                match inner {
                    SequenceNodeInternal::Simple(n) => skip.extend(parse_skip(n, jinja)?),
                    SequenceNodeInternal::Conditional(if_sel) => {
                        let if_res = if_sel.process(jinja)?;
                        if let Some(if_res) = if_res {
                            skip.extend(parse_skip(&if_res, jinja)?)
                        }
                    }
                }
            }
            Ok(skip)
        }
        Node::Mapping(_) => Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected scalar or sequence"
        )),
    }
}

fn parse_script(node: &Node, jinja: &Jinja) -> Result<Vec<String>, PartialParsingError> {
    match node {
        Node::Scalar(s) => {
            let script = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering `script`"
                )
            })?;
            Ok(vec![script])
        }
        Node::Sequence(seq) => {
            let mut scripts = Vec::new();
            for inner in seq.iter() {
                match inner {
                    SequenceNodeInternal::Simple(n) => scripts.extend(parse_script(n, jinja)?),
                    SequenceNodeInternal::Conditional(if_sel) => {
                        let if_res = if_sel.process(jinja)?;
                        if let Some(if_res) = if_res {
                            scripts.extend(parse_script(&if_res, jinja)?)
                        }
                    }
                }
            }
            Ok(scripts)
        }
        Node::Mapping(_) => Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected scalar or sequence"
        )),
    }
}

fn parse_entry_points(node: &Node, jinja: &Jinja) -> Result<Vec<EntryPoint>, PartialParsingError> {
    match node {
        Node::Scalar(s) => {
            let entry_point = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering `entry_points`"
                )
            })?;
            let entry_point = EntryPoint::from_str(&entry_point).map_err(|_err| {
                // TODO: Better handling of this
                _partialerror!(
                    *s.span(),
                    ErrorKind::Other,
                    label = "error in the entrypoint format"
                )
            })?;
            Ok(vec![entry_point])
        }
        Node::Sequence(seq) => {
            let mut entry_points = Vec::new();
            for inner in seq.iter() {
                match inner {
                    SequenceNodeInternal::Simple(n) => {
                        entry_points.extend(parse_entry_points(n, jinja)?)
                    }
                    SequenceNodeInternal::Conditional(if_sel) => {
                        let if_res = if_sel.process(jinja)?;
                        if let Some(if_res) = if_res {
                            entry_points.extend(parse_entry_points(&if_res, jinja)?)
                        }
                    }
                }
            }
            Ok(entry_points)
        }
        Node::Mapping(_) => Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected scalar or sequence"
        )),
    }
}

fn parse_ignore_run_exports(
    node: &Node,
    jinja: &Jinja,
) -> Result<Vec<PackageName>, PartialParsingError> {
    match node {
        Node::Scalar(s) => {
            let ignore_run_export = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering `ignore_run_exports`"
                )
            })?;

            if ignore_run_export.is_empty() {
                Err(_partialerror!(
                    *s.span(),
                    ErrorKind::Other,
                    label = "empty string is not allowed in `ignore_run_exports`"
                ))
            } else {
                let ignore_run_export =
                    PackageName::from_str(&ignore_run_export).map_err(|_err| {
                        // TODO: Better handling of this
                        _partialerror!(
                            *s.span(),
                            ErrorKind::Other,
                            label = "error parsing `ignore_run_exports`"
                        )
                    })?;
                Ok(vec![ignore_run_export])
            }
        }
        Node::Sequence(seq) => {
            let mut ignore_run_exports = Vec::new();
            for inner in seq.iter() {
                match inner {
                    SequenceNodeInternal::Simple(n) => {
                        ignore_run_exports.extend(parse_ignore_run_exports(n, jinja)?)
                    }
                    SequenceNodeInternal::Conditional(if_sel) => {
                        let if_res = if_sel.process(jinja)?;
                        if let Some(if_res) = if_res {
                            ignore_run_exports.extend(parse_ignore_run_exports(&if_res, jinja)?)
                        }
                    }
                }
            }
            Ok(ignore_run_exports)
        }
        Node::Mapping(_) => Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected scalar or sequence"
        )),
    }
}

/// Extra environment variables to set during the build script execution
#[derive(Debug, Default, Clone, Serialize)]
pub struct ScriptEnv {
    /// Environments variables to leak into the build environment from the host system.
    /// During build time these variables are recorded and stored in the package output.
    /// Use `secrets` for environment variables that should not be recorded.
    pub(super) passthrough: Vec<String>,
    /// Environment variables to set in the build environment.
    pub(super) env: BTreeMap<String, String>,
    /// Environment variables to leak into the build environment from the host system that
    /// contain sensitve information. Use with care because this might make recipes no
    /// longer reproducible on other machines.
    pub(super) secrets: Vec<String>,
}

impl ScriptEnv {
    fn from_node(node: &Node, jinja: &Jinja) -> Result<Self, PartialParsingError> {
        if let Some(map) = node.as_mapping() {
            let env = map
                .get("env")
                .map(|node| parse_env(node, jinja))
                .transpose()?
                .unwrap_or_default();

            let passthrough = map
                .get("passthrough")
                .map(|node| parse_passthrough(node, jinja))
                .transpose()?
                .unwrap_or_default();

            let secrets = map
                .get("secrets")
                .map(|node| parse_secrets(node, jinja))
                .transpose()?
                .unwrap_or_default();

            Ok(Self {
                passthrough,
                env,
                secrets,
            })
        } else {
            Err(_partialerror!(
                *node.span(),
                ErrorKind::Other,
                label = "expected mapping on `script_env`"
            ))
        }
    }

    /// Check if the script environment is empty is all its fields.
    pub fn is_empty(&self) -> bool {
        self.passthrough.is_empty() && self.env.is_empty() && self.secrets.is_empty()
    }

    /// Get the passthrough environment variables.
    ///
    /// Those are the environments variables to leak into the build environment from the host system.
    ///
    /// During build time these variables are recorded and stored in the package output.
    /// Use `secrets` for environment variables that should not be recorded.
    pub fn passthrough(&self) -> &[String] {
        self.passthrough.as_slice()
    }

    /// Get the environment variables to set in the build environment.
    pub fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }

    /// Get the secrets environment variables.
    ///
    /// Environment variables to leak into the build environment from the host system that
    /// contain sensitve information.
    ///
    /// # Warning
    /// Use with care because this might make recipes no longer reproducible on other machines.
    pub fn secrets(&self) -> &[String] {
        self.secrets.as_slice()
    }
}

fn parse_env(node: &Node, jinja: &Jinja) -> Result<BTreeMap<String, String>, PartialParsingError> {
    if let Some(map) = node.as_mapping() {
        let mut env = BTreeMap::new();
        for (key, value) in map.iter() {
            let key = key.as_str();
            let value = value.as_scalar().ok_or_else(|| {
                _partialerror!(*value.span(), ErrorKind::Other, label = "expected scalar")
            })?;
            let value = jinja.render_str(value.as_str()).map_err(|err| {
                _partialerror!(
                    *value.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering `env` map value"
                )
            })?;
            env.insert(key.to_owned(), value);
        }
        Ok(env)
    } else {
        Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected mapping on `env`"
        ))
    }
}

// TODO: make the `secrets` not possible to be seen in the memory
fn parse_secrets(node: &Node, jinja: &Jinja) -> Result<Vec<String>, PartialParsingError> {
    match node {
        Node::Scalar(s) => {
            let secret = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering `secrets`"
                )
            })?;

            if secret.is_empty() {
                Err(_partialerror!(
                    *s.span(),
                    ErrorKind::Other,
                    label = "empty string is not allowed in `secrets`"
                ))
            } else {
                Ok(vec![secret])
            }
        }
        Node::Sequence(seq) => {
            let mut secrets = Vec::new();
            for inner in seq.iter() {
                match inner {
                    SequenceNodeInternal::Simple(n) => secrets.extend(parse_secrets(n, jinja)?),
                    SequenceNodeInternal::Conditional(if_sel) => {
                        let if_res = if_sel.process(jinja)?;
                        if let Some(if_res) = if_res {
                            secrets.extend(parse_secrets(&if_res, jinja)?)
                        }
                    }
                }
            }
            Ok(secrets)
        }
        Node::Mapping(_) => Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected scalar or sequence"
        )),
    }
}

fn parse_passthrough(node: &Node, jinja: &Jinja) -> Result<Vec<String>, PartialParsingError> {
    match node {
        Node::Scalar(s) => {
            let passthrough = jinja.render_str(s.as_str()).map_err(|err| {
                _partialerror!(
                    *s.span(),
                    ErrorKind::JinjaRendering(err),
                    label = "error rendering `passthrough`"
                )
            })?;

            if passthrough.is_empty() {
                Err(_partialerror!(
                    *s.span(),
                    ErrorKind::Other,
                    label = "empty string is not allowed in `passthrough`"
                ))
            } else {
                Ok(vec![passthrough])
            }
        }
        Node::Sequence(seq) => {
            let mut passthrough = Vec::new();
            for inner in seq.iter() {
                match inner {
                    SequenceNodeInternal::Simple(n) => {
                        passthrough.extend(parse_passthrough(n, jinja)?)
                    }
                    SequenceNodeInternal::Conditional(if_sel) => {
                        let if_res = if_sel.process(jinja)?;
                        if let Some(if_res) = if_res {
                            passthrough.extend(parse_passthrough(&if_res, jinja)?)
                        }
                    }
                }
            }
            Ok(passthrough)
        }
        Node::Mapping(_) => Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected scalar or sequence"
        )),
    }
}

/// Run exports are applied to downstream packages that depend on this package.
#[derive(Debug, Default, Clone, Serialize)]
pub struct RunExports {
    /// Noarch run exports are the only ones looked at when building noarch packages.
    pub(super) noarch: Vec<Dependency>,
    /// Strong run exports apply from the build and host env to the run env.
    pub(super) strong: Vec<Dependency>,
    /// Strong run constrains add run_constrains from the build and host env.
    pub(super) strong_constrains: Vec<Dependency>,
    /// Weak run exports apply from the host env to the run env.
    pub(super) weak: Vec<Dependency>,
    /// Weak run constrains add run_constrains from the host env.
    pub(super) weak_constrains: Vec<Dependency>,
}

impl RunExports {
    fn from_node(node: &Node, jinja: &Jinja) -> Result<RunExports, PartialParsingError> {
        let mut run_exports = RunExports::default();

        match node {
            Node::Scalar(_) | Node::Sequence(_) => {
                let deps = parse_dependency(node, jinja)?;
                run_exports.strong = deps;
            }
            Node::Mapping(map) => {
                for (key, value) in map.iter() {
                    match key.as_str() {
                        "noarch" => {
                            let deps = parse_dependency(value, jinja)?;
                            run_exports.noarch = deps;
                        }
                        "strong" => {
                            let deps = parse_dependency(value, jinja)?;
                            run_exports.strong = deps;
                        }
                        "strong_constrains" => {
                            let deps = parse_dependency(value, jinja)?;
                            run_exports.strong_constrains = deps;
                        }
                        "weak" => {
                            let deps = parse_dependency(value, jinja)?;
                            run_exports.weak = deps;
                        }
                        "weak_constrains" => {
                            let deps = parse_dependency(value, jinja)?;
                            run_exports.weak_constrains = deps;
                        }
                        _ => unreachable!("invalid field: {}", key.as_str()),
                    }
                }
            }
        }
        Ok(run_exports)
    }

    /// Check if all fields are empty
    pub fn is_empty(&self) -> bool {
        self.noarch.is_empty()
            && self.strong.is_empty()
            && self.strong_constrains.is_empty()
            && self.weak.is_empty()
            && self.weak_constrains.is_empty()
    }

    /// Get the noarch run exports.
    pub fn noarch(&self) -> &[Dependency] {
        self.noarch.as_slice()
    }

    /// Get the strong run exports.
    pub fn strong(&self) -> &[Dependency] {
        self.strong.as_slice()
    }

    /// Get the strong run constrains.
    pub fn strong_constrains(&self) -> &[Dependency] {
        self.strong_constrains.as_slice()
    }

    /// Get the weak run exports.
    pub fn weak(&self) -> &[Dependency] {
        self.weak.as_slice()
    }

    /// Get the weak run constrains.
    pub fn weak_constrains(&self) -> &[Dependency] {
        self.weak_constrains.as_slice()
    }
}

fn parse_dependency(node: &Node, jinja: &Jinja) -> Result<Vec<Dependency>, PartialParsingError> {
    match node {
        Node::Scalar(s) => {
            let dep = Dependency::from_scalar(s, jinja)?;
            Ok(vec![dep])
        }
        Node::Sequence(seq) => {
            let mut deps = Vec::new();
            for inner in seq.iter() {
                match inner {
                    SequenceNodeInternal::Simple(n) => deps.extend(parse_dependency(n, jinja)?),
                    SequenceNodeInternal::Conditional(if_sel) => {
                        let if_res = if_sel.process(jinja)?;
                        if let Some(if_res) = if_res {
                            deps.extend(parse_dependency(&if_res, jinja)?)
                        }
                    }
                }
            }
            Ok(deps)
        }
        Node::Mapping(_) => Err(_partialerror!(
            *node.span(),
            ErrorKind::Other,
            label = "expected scalar or sequence"
        )),
    }
}
