//! Parser for pipeline structures
//!
//! This module handles parsing of pipeline definitions from YAML,
//! both inline in recipes and from external pipeline files.

use indexmap::IndexMap;
use marked_yaml::Node;
use rattler_build_yaml_parser::{ParseError, parse_conditional_list, parse_value_with_name};

use crate::stage0::{
    Conditional, ConditionalList, Item, JinjaExpression, NestedItemList,
    pipeline::{Pipeline, PipelineDefinition, PipelineInput, PipelineStep},
    types::Value,
};

use super::build::parse_script;
use super::helpers::get_span;

/// Macro to parse a value with automatic field name inference for better error messages
macro_rules! parse_field {
    ($field:literal, $node:expr) => {{
        parse_value_with_name($node, $field)?
    }};
}

/// Parse a pipeline list from YAML (in build section)
///
/// Example:
/// ```yaml
/// pipeline:
///   - uses: ./pipelines/cmake/configure.yaml
///     with:
///       cmake_args: ["-DCMAKE_BUILD_TYPE=Release"]
///   - script:
///       - echo "Done"
/// ```
pub fn parse_pipeline(node: &Node) -> Result<Pipeline, ParseError> {
    let sequence = node.as_sequence().ok_or_else(|| {
        ParseError::expected_type("sequence", "non-sequence", get_span(node))
            .with_message("pipeline must be a list of steps")
    })?;

    let mut items = Vec::new();
    for item_node in sequence.iter() {
        items.push(parse_pipeline_item(item_node)?);
    }

    Ok(ConditionalList::new(items))
}

/// Parse a single pipeline item which can be either a PipelineStep or a conditional
fn parse_pipeline_item(node: &Node) -> Result<Item<PipelineStep>, ParseError> {
    let mapping = node.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(node))
            .with_message("pipeline step must be a mapping")
    })?;

    // Check if it's a conditional (mapping with "if" key)
    if mapping.get("if").is_some() {
        return parse_conditional_pipeline_item(mapping);
    }

    // Not a conditional - parse as a regular PipelineStep
    let step = parse_pipeline_step_from_mapping(mapping)?;
    Ok(Item::Value(Value::new_concrete(step, Some(get_span(node)))))
}

/// Parse a conditional pipeline item with if/then/else branches
fn parse_conditional_pipeline_item(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> Result<Item<PipelineStep>, ParseError> {
    let if_node = mapping
        .get("if")
        .ok_or_else(|| ParseError::missing_field("if", *mapping.span()))?;

    let condition_str = if_node.as_scalar().ok_or_else(|| {
        ParseError::expected_type("scalar", "non-scalar", get_span(if_node))
            .with_message("'if' condition must be a string")
    })?;

    let condition_span = *condition_str.span();
    let condition = JinjaExpression::new(condition_str.as_str().to_string())
        .map_err(|e| ParseError::jinja_error(e, condition_span))?;

    let then_node = mapping
        .get("then")
        .ok_or_else(|| ParseError::missing_field("then", *mapping.span()))?;

    let then_items = parse_pipeline_list_as_values(then_node)?;

    let else_items = if let Some(else_node) = mapping.get("else") {
        Some(parse_pipeline_list_as_values(else_node)?)
    } else {
        None
    };

    Ok(Item::Conditional(Conditional {
        condition,
        then: then_items,
        else_value: else_items,
        condition_span: Some(condition_span),
    }))
}

/// Parse a pipeline list from a sequence node (for then/else branches)
fn parse_pipeline_list_as_values(node: &Node) -> Result<NestedItemList<PipelineStep>, ParseError> {
    if let Some(seq) = node.as_sequence() {
        let mut items = Vec::new();
        for item_node in seq.iter() {
            items.push(parse_pipeline_item(item_node)?);
        }
        Ok(NestedItemList::new(items))
    } else if node.as_mapping().is_some() {
        // Single pipeline step mapping
        let item = parse_pipeline_item(node)?;
        Ok(NestedItemList::single(item))
    } else {
        Err(ParseError::expected_type(
            "sequence or mapping",
            "non-sequence/mapping",
            get_span(node),
        )
        .with_message("'then' and 'else' must be sequences of pipeline steps or a single step"))
    }
}

/// Parse a pipeline step from a mapping node
fn parse_pipeline_step_from_mapping(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> Result<PipelineStep, ParseError> {
    let mut uses = None;
    let mut with = IndexMap::new();
    let mut script = None;
    let mut name = None;

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();

        match key {
            "uses" => {
                uses = Some(parse_field!("pipeline.uses", value_node));
            }
            "with" => {
                with = parse_with_arguments(value_node)?;
            }
            "script" => {
                script = Some(parse_script(value_node)?);
            }
            "name" => {
                name = Some(parse_field!("pipeline.name", value_node));
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "pipeline step",
                    format!("unknown field '{}'", key),
                    *key_node.span(),
                )
                .with_suggestion("Valid fields are: uses, with, script, name"));
            }
        }
    }

    // Validate mutual exclusivity of uses and script
    if uses.is_some() && script.is_some() {
        return Err(ParseError::invalid_value(
            "pipeline step",
            "cannot have both 'uses' and 'script' in the same step",
            *mapping.span(),
        )
        .with_suggestion("Use either 'uses' to reference an external pipeline, or 'script' for inline commands"));
    }

    // Validate that at least one of uses or script is present
    if uses.is_none() && script.is_none() {
        return Err(ParseError::invalid_value(
            "pipeline step",
            "pipeline step must have either 'uses' or 'script'",
            *mapping.span(),
        )
        .with_suggestion("Add either 'uses: ./path/to/pipeline.yaml' or 'script: [...]'"));
    }

    Ok(PipelineStep {
        uses,
        with,
        script,
        name,
    })
}

/// Parse `with` arguments - a mapping of key-value pairs
///
/// Values are kept as serde_yaml::Value to support various types
/// (strings, lists, mappings, etc.)
fn parse_with_arguments(node: &Node) -> Result<IndexMap<String, serde_yaml::Value>, ParseError> {
    let mapping = node.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(node))
            .with_message("'with' must be a mapping of arguments")
    })?;

    let mut args = IndexMap::new();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str().to_string();
        let value = node_to_serde_value(value_node)?;
        args.insert(key, value);
    }

    Ok(args)
}

/// Convert a marked_yaml Node to a serde_yaml::Value
fn node_to_serde_value(node: &Node) -> Result<serde_yaml::Value, ParseError> {
    match node {
        Node::Scalar(scalar) => {
            let s = scalar.as_str();
            // Try to parse as various types
            if let Some(b) = scalar.as_bool() {
                Ok(serde_yaml::Value::Bool(b))
            } else if let Ok(i) = s.parse::<i64>() {
                Ok(serde_yaml::Value::Number(i.into()))
            } else {
                // For floats and strings, keep as string
                // The Jinja rendering will handle type conversion as needed
                Ok(serde_yaml::Value::String(s.to_string()))
            }
        }
        Node::Sequence(seq) => {
            let mut values = Vec::new();
            for item in seq.iter() {
                values.push(node_to_serde_value(item)?);
            }
            Ok(serde_yaml::Value::Sequence(values))
        }
        Node::Mapping(mapping) => {
            let mut map = serde_yaml::Mapping::new();
            for (k, v) in mapping.iter() {
                let key = serde_yaml::Value::String(k.as_str().to_string());
                let value = node_to_serde_value(v)?;
                map.insert(key, value);
            }
            Ok(serde_yaml::Value::Mapping(map))
        }
    }
}

/// Parse a pipeline definition from an external YAML file
///
/// Example pipeline file:
/// ```yaml
/// name: Configure CMake
/// script:
///   - cmake -S . -B build ${{ input.cmake_args | join(" ") }}
/// inputs:
///   cmake_args:
///     description: Additional CMake arguments
///     default: []
/// outputs:
///   - ./build
/// cpu_cost: 1
/// ```
///
/// Note: Currently pipeline files are loaded via serde_yaml directly in script.rs,
/// but this parser is available for future stricter parsing with better error messages.
#[allow(dead_code)]
pub fn parse_pipeline_definition(node: &Node) -> Result<PipelineDefinition, ParseError> {
    let mapping = node.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(node))
            .with_message("Pipeline definition must be a mapping")
    })?;

    let mut definition = PipelineDefinition::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();

        match key {
            "name" => {
                let scalar = value_node.as_scalar().ok_or_else(|| {
                    ParseError::expected_type("scalar", "non-scalar", get_span(value_node))
                        .with_message("'name' must be a string")
                })?;
                definition.name = Some(scalar.as_str().to_string());
            }
            "script" => {
                definition.script = parse_script(value_node)?;
            }
            "inputs" => {
                definition.inputs = parse_pipeline_inputs(value_node)?;
            }
            "outputs" => {
                definition.outputs = parse_conditional_list(value_node)?;
            }
            "cpu_cost" => {
                definition.cpu_cost = Some(parse_field!("pipeline.cpu_cost", value_node));
            }
            "interpreter" => {
                definition.interpreter = Some(parse_field!("pipeline.interpreter", value_node));
            }
            "env" => {
                definition.env = parse_env_mapping(value_node)?;
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "pipeline definition",
                    format!("unknown field '{}'", key),
                    *key_node.span(),
                )
                .with_suggestion(
                    "Valid fields are: name, script, inputs, outputs, cpu_cost, interpreter, env",
                ));
            }
        }
    }

    Ok(definition)
}

/// Parse pipeline inputs definition
#[allow(dead_code)]
fn parse_pipeline_inputs(
    node: &Node,
) -> Result<IndexMap<String, PipelineInput>, ParseError> {
    let mapping = node.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(node))
            .with_message("'inputs' must be a mapping")
    })?;

    let mut inputs = IndexMap::new();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str().to_string();
        let input = parse_pipeline_input(value_node)?;
        inputs.insert(key, input);
    }

    Ok(inputs)
}

/// Parse a single pipeline input definition
#[allow(dead_code)]
fn parse_pipeline_input(node: &Node) -> Result<PipelineInput, ParseError> {
    let mapping = node.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(node))
            .with_message("Pipeline input must be a mapping with description/default/required")
    })?;

    let mut input = PipelineInput::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();

        match key {
            "description" => {
                let scalar = value_node.as_scalar().ok_or_else(|| {
                    ParseError::expected_type("scalar", "non-scalar", get_span(value_node))
                        .with_message("'description' must be a string")
                })?;
                input.description = Some(scalar.as_str().to_string());
            }
            "default" => {
                input.default = Some(node_to_serde_value(value_node)?);
            }
            "required" => {
                let scalar = value_node.as_scalar().ok_or_else(|| {
                    ParseError::expected_type("scalar", "non-scalar", get_span(value_node))
                        .with_message("'required' must be a boolean")
                })?;
                input.required = scalar.as_bool().ok_or_else(|| {
                    ParseError::invalid_value(
                        "required",
                        "expected boolean",
                        *scalar.span(),
                    )
                })?;
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "pipeline input",
                    format!("unknown field '{}'", key),
                    *key_node.span(),
                )
                .with_suggestion("Valid fields are: description, default, required"));
            }
        }
    }

    Ok(input)
}

/// Parse environment variable mapping
#[allow(dead_code)]
fn parse_env_mapping(node: &Node) -> Result<IndexMap<String, Value<String>>, ParseError> {
    let mapping = node.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(node))
            .with_message("'env' must be a mapping")
    })?;

    let mut env = IndexMap::new();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str().to_string();
        let value = parse_field!("env", value_node);
        env.insert(key, value);
    }

    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pipeline_with_uses() {
        let yaml = r#"
pipeline:
  - uses: ./pipelines/cmake/configure.yaml
    with:
      cmake_args:
        - -DCMAKE_BUILD_TYPE=Release
"#;
        let node = marked_yaml::parse_yaml(0, yaml).unwrap();
        let mapping = node.as_mapping().unwrap();
        let pipeline_node = mapping.get("pipeline").unwrap();
        let pipeline = parse_pipeline(pipeline_node).unwrap();
        assert_eq!(pipeline.len(), 1);

        let step = pipeline.iter().next().unwrap();
        if let Item::Value(value) = step {
            let step = value.as_concrete().unwrap();
            assert!(step.uses.is_some());
            assert!(step.script.is_none());
            assert!(!step.with.is_empty());
        } else {
            panic!("Expected Value, got Conditional");
        }
    }

    #[test]
    fn test_parse_pipeline_with_inline_script() {
        let yaml = r#"
pipeline:
  - script:
      - echo "Hello"
      - make install
    name: Build step
"#;
        let node = marked_yaml::parse_yaml(0, yaml).unwrap();
        let mapping = node.as_mapping().unwrap();
        let pipeline_node = mapping.get("pipeline").unwrap();
        let pipeline = parse_pipeline(pipeline_node).unwrap();
        assert_eq!(pipeline.len(), 1);

        let step = pipeline.iter().next().unwrap();
        if let Item::Value(value) = step {
            let step = value.as_concrete().unwrap();
            assert!(step.uses.is_none());
            assert!(step.script.is_some());
        } else {
            panic!("Expected Value, got Conditional");
        }
    }

    #[test]
    fn test_parse_pipeline_mutual_exclusivity() {
        let yaml = r#"
pipeline:
  - uses: ./pipelines/cmake/configure.yaml
    script:
      - echo "This should fail"
"#;
        let node = marked_yaml::parse_yaml(0, yaml).unwrap();
        let mapping = node.as_mapping().unwrap();
        let pipeline_node = mapping.get("pipeline").unwrap();
        let result = parse_pipeline(pipeline_node);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("cannot have both"));
    }

    #[test]
    fn test_parse_pipeline_definition() {
        let yaml = r#"
name: Configure CMake
script:
  - cmake -S . -B build
inputs:
  cmake_args:
    description: Additional CMake arguments
    default: []
outputs:
  - ./build
cpu_cost: "1"
"#;
        let node = marked_yaml::parse_yaml(0, yaml).unwrap();
        let definition = parse_pipeline_definition(&node).unwrap();
        assert_eq!(definition.name, Some("Configure CMake".to_string()));
        assert!(definition.script.content.is_some());
        assert!(!definition.inputs.is_empty());
        assert!(!definition.outputs.is_empty());
    }
}
