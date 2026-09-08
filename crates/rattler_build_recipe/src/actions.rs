//! Render-time action sources shared by local and packaged action compilation.
use crate::{
    ParseError, Span,
    stage0::{JinjaTemplate, Value, parse_step_requirements, parse_steps},
};
use rattler_build_jinja::Variable;
use rattler_build_yaml_parser::ValueInner;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

/// Native source input, retaining individual templates inside lists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ActionValue {
    List(Vec<ActionValue>),
    /// Retained until the invocation condition is known; declared inputs reject maps.
    Mapping(indexmap::IndexMap<String, ActionValue>),
    Scalar(Value<Variable>),
}

impl ActionValue {
    pub(crate) fn used_variables(&self) -> Vec<String> {
        match self {
            Self::Scalar(value) => value.used_variables(),
            Self::List(values) => values.iter().flat_map(Self::used_variables).collect(),
            Self::Mapping(values) => values.values().flat_map(Self::used_variables).collect(),
        }
    }

    pub(crate) fn parse(node: &marked_yaml::Node) -> Result<Self, ParseError> {
        if let Some(sequence) = node.as_sequence() {
            return sequence
                .iter()
                .map(Self::parse)
                .collect::<Result<Vec<_>, _>>()
                .map(Self::List);
        }
        if let Some(mapping) = node.as_mapping() {
            return mapping
                .iter()
                .map(|(key, value)| Ok((key.as_str().to_owned(), Self::parse(value)?)))
                .collect::<Result<indexmap::IndexMap<_, _>, ParseError>>()
                .map(Self::Mapping);
        }
        let scalar = node.as_scalar().ok_or_else(|| {
            ParseError::generic(
                "action inputs must be scalars or homogeneous lists",
                Span::new_blank(),
            )
        })?;
        let text = scalar.as_str();
        if text.contains("${{") {
            let template = JinjaTemplate::new(text.to_owned())
                .map_err(|error| ParseError::jinja_error(error, *scalar.span()))?;
            return Ok(Self::Scalar(Value::new_template(
                template,
                Some(*scalar.span()),
            )));
        }
        let value = if scalar.may_coerce() {
            let native: serde_yaml::Value = serde_yaml::from_str(text)
                .map_err(|error| invalid(format!("invalid action scalar: {error}")))?;
            Variable::from(minijinja::Value::from_serialize(native))
        } else {
            Variable::from(text.to_owned())
        };
        Ok(Self::Scalar(Value::new_concrete(
            value,
            Some(*scalar.span()),
        )))
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ActionEnvironment {
    pub sources: ActionSources,
    pub origin: Option<PathBuf>,
    pub selected: Option<Vec<String>>,
    pub variants: indexmap::IndexMap<String, Variable>,
}

/// Compilation result; requirements belong to the effective recipe, not a run.
#[derive(Debug, Clone, Default)]
pub struct CompiledSteps {
    pub steps: Vec<crate::stage1::build::Step>,
    pub requirements: crate::stage1::build::StepRequirements,
    pub provenance: Vec<ActionProvenance>,
}

#[derive(Debug, Clone, Copy)]
enum ScalarType {
    String,
    Boolean,
    Integer,
}

#[derive(Debug, Clone)]
struct InputDefinition {
    scalar: ScalarType,
    list: bool,
    required: bool,
    default: Option<ActionValue>,
}

#[derive(Debug, Clone)]
struct ActionDocument {
    inputs: indexmap::IndexMap<String, InputDefinition>,
    requirements: crate::stage0::build::StepRequirements,
    steps: Vec<crate::stage0::build::Step>,
}

fn invalid(message: impl Into<String>) -> ParseError {
    ParseError::generic(message, Span::new_blank())
}

fn scalar_type(value: &str) -> Result<ScalarType, ParseError> {
    match value {
        "string" => Ok(ScalarType::String),
        "boolean" => Ok(ScalarType::Boolean),
        "integer" => Ok(ScalarType::Integer),
        _ => Err(invalid(format!("invalid action input type '{value}'"))),
    }
}

fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn text(node: &marked_yaml::Node) -> Result<&str, ParseError> {
    let scalar = node
        .as_scalar()
        .ok_or_else(|| invalid("expected a string"))?;
    if scalar.may_coerce()
        && !serde_yaml::from_str::<serde_yaml::Value>(scalar.as_str())
            .is_ok_and(|value| value.is_string())
    {
        return Err(invalid("expected a string"));
    }
    Ok(scalar.as_str())
}

fn parse_document(contents: &str) -> Result<ActionDocument, ParseError> {
    let node = rattler_build_yaml_parser::parse_yaml(contents)
        .map_err(|error| invalid(format!("invalid action YAML: {error}")))?;
    let mapping = node
        .as_mapping()
        .ok_or_else(|| invalid("action document must be a mapping"))?;
    let mut inputs = indexmap::IndexMap::new();
    let mut requirements = Default::default();
    let mut steps = None;
    for (key, value) in mapping.iter() {
        match key.as_str() {
            "schema_version" => {
                if !value
                    .as_scalar()
                    .is_some_and(|v| v.may_coerce() && v.as_i64() == Some(1))
                {
                    return Err(invalid("action schema_version must be integer 1"));
                }
            }
            "action" => {
                let metadata = value
                    .as_mapping()
                    .ok_or_else(|| invalid("action metadata must be a mapping"))?;
                for (key, value) in metadata.iter() {
                    if !matches!(key.as_str(), "name" | "description") {
                        return Err(invalid(format!(
                            "unknown action metadata field '{}'",
                            key.as_str()
                        )));
                    }
                    text(value)?;
                }
            }
            "inputs" => {
                let declarations = value
                    .as_mapping()
                    .ok_or_else(|| invalid("action inputs must be a mapping"))?;
                for (name, definition) in declarations.iter() {
                    if !identifier(name.as_str()) {
                        return Err(invalid("input names must match [A-Za-z_][A-Za-z0-9_]*"));
                    }
                    let fields = definition
                        .as_mapping()
                        .ok_or_else(|| invalid("input definition must be a mapping"))?;
                    let mut kind = None;
                    let mut items = None;
                    let mut required = None;
                    let mut default = None;
                    for (key, value) in fields.iter() {
                        match key.as_str() {
                            "type" => kind = Some(text(value)?.to_owned()),
                            "items" => items = Some(scalar_type(text(value)?)?),
                            "description" => {
                                text(value)?;
                            }
                            "required" => {
                                required = Some(
                                    value
                                        .as_scalar()
                                        .filter(|v| v.may_coerce())
                                        .and_then(|v| v.as_bool())
                                        .ok_or_else(|| invalid("required must be a boolean"))?,
                                )
                            }
                            "default" => {
                                let parsed = ActionValue::parse(value)?;
                                if !parsed.used_variables().is_empty() || contains_template(&parsed)
                                {
                                    return Err(invalid(
                                        "action defaults must be static, not Jinja",
                                    ));
                                }
                                default = Some(parsed);
                            }
                            other => return Err(invalid(format!("unknown input field '{other}'"))),
                        }
                    }
                    let kind =
                        kind.ok_or_else(|| invalid("action input requires an explicit type"))?;
                    let list = kind == "list";
                    let scalar = if list {
                        items.ok_or_else(|| invalid("list input requires scalar items type"))?
                    } else {
                        if items.is_some() {
                            return Err(invalid("items is only valid for list inputs"));
                        }
                        scalar_type(&kind)?
                    };
                    if required == Some(true) && default.is_some() {
                        return Err(invalid("required input cannot have a default"));
                    }
                    let definition = InputDefinition {
                        scalar,
                        list,
                        required: required.unwrap_or(default.is_none()),
                        default,
                    };
                    if let Some(default) = &definition.default {
                        let value =
                            evaluate_input(default, &crate::stage1::EvaluationContext::new())?;
                        validate_input(name.as_str(), &definition, &value)?;
                    }
                    inputs.insert(name.as_str().to_owned(), definition);
                }
            }
            "requirements" => {
                let fields = value
                    .as_mapping()
                    .ok_or_else(|| invalid("action requirements must be a mapping"))?;
                for (key, _) in fields.iter() {
                    if !matches!(key.as_str(), "build" | "host") {
                        return Err(invalid("actions can only own build and host requirements"));
                    }
                }
                requirements = parse_step_requirements(value)?;
            }
            "steps" => steps = Some(parse_steps(value)?),
            other => return Err(invalid(format!("unknown action document field '{other}'"))),
        }
    }
    Ok(ActionDocument {
        inputs,
        requirements,
        steps: steps
            .ok_or_else(|| invalid("action document requires steps (which may be empty)"))?,
    })
}

fn contains_template(value: &ActionValue) -> bool {
    match value {
        ActionValue::Scalar(value) => value.as_concrete().is_none(),
        ActionValue::List(values) => values.iter().any(contains_template),
        ActionValue::Mapping(values) => values.values().any(contains_template),
    }
}

fn evaluate_input(
    value: &ActionValue,
    context: &crate::stage1::EvaluationContext,
) -> Result<Variable, ParseError> {
    match value {
        ActionValue::Scalar(value) => {
            if let ValueInner::Template(template) = value.inner() {
                let source = template.source().trim();
                let standalone = source.starts_with("${{")
                    && source.ends_with("}}")
                    && !source[3..source.len() - 2].contains("${{");
                if !standalone {
                    let jinja = context.to_jinja();
                    let result = jinja.render_str(template.source()).map_err(|error| {
                        invalid(format!("invalid action input expression: {error}"))
                    })?;
                    for key in jinja.accessed_variables_excluding_functions() {
                        context.track_access(&key);
                    }
                    return Ok(Variable::from(result));
                }
            }
            crate::stage0::evaluate::evaluate_value_to_variable(value, context)
        }
        ActionValue::List(values) => values
            .iter()
            .map(|v| evaluate_input(v, context))
            .collect::<Result<Vec<_>, _>>()
            .map(Variable::from),
        ActionValue::Mapping(_) => Err(invalid(
            "action inputs must be scalars or homogeneous lists",
        )),
    }
}

fn validate_input(
    name: &str,
    definition: &InputDefinition,
    value: &Variable,
) -> Result<(), ParseError> {
    use minijinja::value::ValueKind;
    let value = value.as_ref();
    if value.is_none() {
        return if definition.required {
            Err(invalid(format!(
                "required action input '{name}' cannot be null"
            )))
        } else {
            Ok(())
        };
    }
    let scalar_valid = |value: &minijinja::Value| match definition.scalar {
        ScalarType::String => value.kind() == ValueKind::String,
        ScalarType::Integer => {
            value.kind() == ValueKind::Number
                && serde_json::to_value(value).is_ok_and(|value| value.is_i64())
        }
        ScalarType::Boolean => value.kind() == ValueKind::Bool,
    };
    let valid = if definition.list {
        value.kind() == ValueKind::Seq
            && value
                .try_iter()
                .map(|mut items| items.all(|item| scalar_valid(&item)))
                .unwrap_or(false)
    } else {
        scalar_valid(value)
    };
    if valid {
        Ok(())
    } else {
        Err(invalid(format!(
            "action input '{name}' does not match its declared type"
        )))
    }
}

/// An unresolved external reference and its compilation environment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActionRequest {
    pub reference: String,
    /// File containing the reference, not its parent directory.
    pub origin: PathBuf,
    pub channel_sources: Option<String>,
}

/// Exact provider identity retained in rendered recipes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedProvider {
    pub name: String,
    pub version: String,
    pub build: String,
    pub subdir: String,
    pub channel: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub md5: Option<String>,
}

/// Source identity without machine-specific source paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionProvenance {
    pub reference: String,
    pub provider: Option<ResolvedProvider>,
    pub content_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

/// A source document supplied by a transport. Evaluation belongs to the compiler.
#[derive(Debug, Clone)]
pub struct ActionSource {
    pub path: PathBuf,
    pub contents: String,
    pub fingerprint: Option<String>,
    pub provenance: Option<ActionProvenance>,
}

#[derive(Debug, Default)]
struct SourceRegistry {
    sources: HashMap<ActionRequest, ActionSource>,
    pending: Option<ActionRequest>,
}

/// Shared source registry and typed synchronous-to-asynchronous resolution bridge.
#[derive(Debug, Clone, Default)]
pub struct ActionSources(Arc<Mutex<SourceRegistry>>);

impl ActionSources {
    pub fn register(&self, request: ActionRequest, source: ActionSource) {
        let mut registry = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if registry.pending.as_ref() == Some(&request) {
            registry.pending = None;
        }
        registry.sources.insert(request, source);
    }

    pub fn take_pending(&self) -> Option<ActionRequest> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .pending
            .take()
    }

    pub fn len(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .sources
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn resolve(&self, request: ActionRequest) -> Option<ActionSource> {
        let mut registry = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(source) = registry.sources.get(&request) {
            return Some(source.clone());
        }
        registry.pending = Some(request);
        None
    }
}

/// Compile source invocations into an ordered executable plan.
pub fn compile_steps(
    steps: &[crate::stage0::build::Step],
    context: &crate::stage1::EvaluationContext,
) -> Result<CompiledSteps, ParseError> {
    let origin = context
        .actions
        .origin
        .clone()
        .unwrap_or_else(|| PathBuf::from("recipe.yaml"));
    let mut compiler = Compiler {
        stack: Vec::new(),
        parsed: HashMap::new(),
    };
    let mut compiled = compiler.compile(
        steps,
        context,
        &origin,
        context.actions.selected.as_deref(),
        None,
        "",
    )?;
    if let Some(selected) = &context.actions.selected {
        let mut policy = None;
        for name in selected {
            let step = steps
                .iter()
                .find(|step| source_metadata(step).0 == Some(name.as_str()))
                .ok_or_else(|| invalid(format!("unknown step '{name}'")))?;
            let next = match step {
                crate::stage0::build::Step::Run(step) => (
                    step.requirements.inherit.build,
                    step.requirements.inherit.host,
                ),
                crate::stage0::build::Step::Uses(_) => (true, true),
            };
            if policy.is_some_and(|policy| policy != next) {
                return Err(invalid(
                    "selected steps have conflicting requirements inheritance",
                ));
            }
            policy = Some(next);
        }
        if let Some((build, host)) = policy {
            compiled.requirements.inherit.build = build;
            compiled.requirements.inherit.host = host;
        }
    }
    Ok(compiled)
}

struct Compiler {
    stack: Vec<PathBuf>,
    parsed: HashMap<PathBuf, Arc<ActionDocument>>,
}

fn source_metadata(step: &crate::stage0::build::Step) -> (Option<&str>, bool, &[String]) {
    match step {
        crate::stage0::build::Step::Run(step) => {
            (step.name.as_deref(), step.optional, &step.depends_on)
        }
        crate::stage0::build::Step::Uses(step) => {
            (step.name.as_deref(), step.optional, &step.depends_on)
        }
    }
}

fn selected_order(
    steps: &[crate::stage0::build::Step],
    selected: Option<&[String]>,
) -> Result<Vec<usize>, ParseError> {
    let mut names = HashMap::new();
    for (index, step) in steps.iter().enumerate() {
        if let Some(name) = source_metadata(step).0 {
            if names.insert(name, index).is_some() {
                return Err(invalid(format!("duplicate step name '{name}'")));
            }
        }
    }
    fn visit(
        index: usize,
        steps: &[crate::stage0::build::Step],
        names: &HashMap<&str, usize>,
        state: &mut [u8],
        order: &mut Vec<usize>,
    ) -> Result<(), ParseError> {
        if state[index] == 2 {
            return Ok(());
        }
        if state[index] == 1 {
            return Err(invalid("cycle in source step dependencies"));
        }
        state[index] = 1;
        for name in source_metadata(&steps[index]).2 {
            let dependency = names
                .get(name.as_str())
                .ok_or_else(|| invalid(format!("unknown step dependency '{name}'")))?;
            visit(*dependency, steps, names, state, order)?;
        }
        state[index] = 2;
        order.push(index);
        Ok(())
    }
    let roots = if let Some(selected) = selected {
        selected
            .iter()
            .map(|name| {
                names
                    .get(name.as_str())
                    .copied()
                    .ok_or_else(|| invalid(format!("unknown step '{name}'")))
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        steps
            .iter()
            .enumerate()
            .filter_map(|(index, step)| (!source_metadata(step).1).then_some(index))
            .collect()
    };
    let mut state = vec![0; steps.len()];
    let mut order = Vec::new();
    for index in roots {
        visit(index, steps, &names, &mut state, &mut order)?;
    }
    Ok(order)
}

impl Compiler {
    fn compile(
        &mut self,
        steps: &[crate::stage0::build::Step],
        context: &crate::stage1::EvaluationContext,
        origin: &std::path::Path,
        selected: Option<&[String]>,
        provenance: Option<&ActionProvenance>,
        prefix: &str,
    ) -> Result<CompiledSteps, ParseError> {
        use crate::stage0::{
            build::Step,
            evaluate::{
                evaluate_condition, evaluate_dependency_list, evaluate_run_steps,
                evaluate_string_value,
            },
        };
        let mut compiled = CompiledSteps::default();
        for index in selected_order(steps, selected)? {
            let step = &steps[index];
            let local_name = source_metadata(step)
                .0
                .map(str::to_owned)
                .unwrap_or_else(|| format!("#{}", index + 1));
            let qualified = if prefix.is_empty() {
                local_name
            } else {
                format!("{prefix}/{local_name}")
            };
            match step {
                Step::Run(_) => {
                    for mut executable in evaluate_run_steps(std::slice::from_ref(step), context)? {
                        if !self.stack.is_empty() {
                            executable.action_context = Some(context.variables().clone());
                            executable.name = Some(qualified.clone());
                        }
                        executable.optional = false;
                        executable.depends_on.clear();
                        compiled
                            .requirements
                            .build
                            .extend(executable.requirements.build.iter().cloned());
                        compiled
                            .requirements
                            .host
                            .extend(executable.requirements.host.iter().cloned());
                        compiled.steps.push(executable);
                    }
                }
                Step::Uses(invocation) => {
                    if let Some(condition) = &invocation.condition
                        && !evaluate_condition(
                            condition,
                            context,
                            invocation.condition_span.as_ref(),
                        )?
                    {
                        continue;
                    }
                    let reference = evaluate_string_value(&invocation.uses, context)?;
                    let local = reference.starts_with("./")
                        || reference.starts_with("../")
                        || reference.starts_with(".\\")
                        || reference.starts_with("..\\");
                    let mut source = if local {
                        let path = std::path::Path::new(&reference);
                        if !matches!(
                            path.extension().and_then(|s| s.to_str()),
                            Some("yaml" | "yml")
                        ) {
                            return Err(invalid(
                                "local action references must name a .yaml or .yml document",
                            ));
                        }
                        let path = origin
                            .parent()
                            .unwrap_or(std::path::Path::new("."))
                            .join(path);
                        let contents = std::fs::read_to_string(&path).map_err(|error| {
                            invalid(format!("cannot load action '{}': {error}", path.display()))
                        })?;
                        ActionSource {
                            path,
                            contents,
                            fingerprint: provenance.and_then(|p| p.fingerprint.clone()),
                            provenance: provenance.cloned(),
                        }
                    } else {
                        if std::path::Path::new(&reference).is_absolute()
                            || !reference.contains(':')
                        {
                            return Err(invalid(
                                "action references must be explicit relative YAML paths or packaged references",
                            ));
                        }
                        let request = ActionRequest {
                            reference: reference.clone(),
                            origin: origin.to_owned(),
                            channel_sources: context
                                .actions
                                .variants
                                .get("channel_sources")
                                .map(ToString::to_string),
                        };
                        context.actions.sources.resolve(request).ok_or_else(|| {
                            invalid(format!(
                                "external action '{reference}' needs source resolution"
                            ))
                        })?
                    };
                    source.provenance = Some(ActionProvenance {
                        reference: reference.clone(),
                        provider: source.provenance.as_ref().and_then(|p| p.provider.clone()),
                        content_sha256: hex::encode(rattler_digest::compute_bytes_digest::<
                            rattler_digest::Sha256,
                        >(
                            source.contents.as_bytes()
                        )),
                        fingerprint: source.fingerprint.clone(),
                    });
                    let identity = source
                        .path
                        .canonicalize()
                        .or_else(|_| std::path::absolute(&source.path))
                        .map_err(|error| {
                            invalid(format!(
                                "cannot identify action '{}': {error}",
                                source.path.display()
                            ))
                        })?;
                    if self.stack.contains(&identity) {
                        let chain = self
                            .stack
                            .iter()
                            .chain(std::iter::once(&identity))
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(" -> ");
                        return Err(invalid(format!("action cycle: {chain}")));
                    }
                    if self.stack.len() >= 64 {
                        let chain = self
                            .stack
                            .iter()
                            .chain(std::iter::once(&identity))
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(" -> ");
                        return Err(invalid(format!(
                            "action nesting exceeds maximum depth 64: {chain}"
                        )));
                    }
                    let document = if let Some(document) = self.parsed.get(&identity) {
                        document.clone()
                    } else {
                        let document = Arc::new(parse_document(&source.contents)?);
                        self.parsed.insert(identity.clone(), document.clone());
                        document
                    };
                    for key in invocation.inputs.keys() {
                        if !document.inputs.contains_key(key) {
                            return Err(invalid(format!("undeclared action input '{key}'")));
                        }
                    }
                    let mut bindings = indexmap::IndexMap::new();
                    for (name, definition) in &document.inputs {
                        let value = if let Some(value) =
                            invocation.inputs.get(name).or(definition.default.as_ref())
                        {
                            evaluate_input(value, context)?
                        } else if definition.required {
                            return Err(invalid(format!("missing required action input '{name}'")));
                        } else {
                            Variable::from(minijinja::Value::from(()))
                        };
                        validate_input(name, definition, &value)?;
                        bindings.insert(name.clone(), value);
                    }
                    let mut variables = context.actions.variants.clone();
                    variables.insert(
                        "inputs".to_owned(),
                        Variable::from(minijinja::Value::from_serialize(&bindings)),
                    );
                    let mut action_context = crate::stage1::EvaluationContext::with_variables_config_os_env_keys_and_repodata_revision(
                        variables, context.jinja_config().clone(), context.os_env_var_keys().clone(), context.repodata_revision());
                    action_context.actions = context.actions.clone();
                    let build =
                        evaluate_dependency_list(&document.requirements.build, &action_context)?;
                    let host =
                        evaluate_dependency_list(&document.requirements.host, &action_context)?;
                    self.stack.push(identity);
                    let nested = self.compile(
                        &document.steps,
                        &action_context,
                        &source.path,
                        None,
                        source.provenance.as_ref(),
                        &qualified,
                    );
                    self.stack.pop();
                    let nested = nested?;
                    compiled.requirements.build.extend(build);
                    compiled.requirements.host.extend(host);
                    compiled
                        .requirements
                        .build
                        .extend(nested.requirements.build);
                    compiled.requirements.host.extend(nested.requirements.host);
                    compiled.steps.extend(nested.steps);
                    if let Some(provenance) = source.provenance {
                        compiled.provenance.push(provenance);
                    }
                    compiled.provenance.extend(nested.provenance);
                    for key in action_context.accessed_variables() {
                        if key != "inputs" {
                            context.track_access(&key);
                        }
                    }
                }
            }
        }
        Ok(compiled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_defaults_obey_native_types_and_signed_integer_bounds() -> Result<(), ParseError> {
        let document = parse_document(
            "inputs:\n  text:\n    type: string\n    default: 'true'\n  count:\n    type: integer\n    default: -9223372036854775808\nsteps: []\n",
        )?;
        let context = crate::stage1::EvaluationContext::new();
        let text = document
            .inputs
            .get("text")
            .and_then(|input| input.default.as_ref())
            .ok_or_else(|| invalid("missing text default"))?;
        assert_eq!(
            evaluate_input(text, &context)?.as_ref().as_str(),
            Some("true")
        );
        for value in ["9223372036854775808", "1.0", "'1'"] {
            assert!(
                parse_document(&format!(
                    "inputs:\n  count:\n    type: integer\n    default: {value}\nsteps: []\n"
                ))
                .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn list_defaults_reject_mixed_scalars_and_nested_lists() {
        for value in ["[one, true]", "[[one]]", "{one: two}"] {
            assert!(parse_document(&format!("inputs:\n  names:\n    type: list\n    items: string\n    default: {value}\nsteps: []\n")).is_err());
        }
    }

    #[test]
    fn action_schema_rejects_unknown_fields_and_dynamic_defaults() {
        for source in [
            "unknown: true\nsteps: []",
            "requirements:\n  run: [python]\nsteps: []",
            "inputs:\n  value:\n    type: string\n    required: true\n    default: x\nsteps: []",
            "inputs:\n  value:\n    type: string\n    default: '${{ \"literal\" }}'\nsteps: []",
            "inputs:\n  invalid-name:\n    type: string\nsteps: []",
            "inputs:\n  value:\n    type: list\nsteps: []",
        ] {
            assert!(parse_document(source).is_err());
        }
    }
}
