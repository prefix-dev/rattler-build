//! Enhanced output parsing that supports cache outputs according to CEP specification

use crate::{
    _partialerror,
    recipe::{
        ParsingError,
        custom_yaml::{HasSpan, RenderedMappingNode, RenderedNode, TryConvertNode},
        error::{ErrorKind, PartialParsingError},
    },
    source_code::SourceCode,
};

use super::cache_output::CacheOutput;
use super::common_output::{
    ALLOWED_KEYS_MULTI_OUTPUTS, DEEP_MERGE_KEYS, extract_recipe_version_rendered,
    merge_rendered_mapping_if_not_exists, parse_root_as_mapping_rendered,
    validate_outputs_sequence_rendered,
};
use super::output_parser::{Output, OutputType};

/// Convert a vector of PartialParsingError to a single ParsingError
fn convert_errors<S: SourceCode>(errors: Vec<PartialParsingError>, src: S) -> ParsingError<S> {
    errors
        .into_iter()
        .map(|e| ParsingError::from_partial(src.clone(), e))
        .next()
        .expect("convert_errors called with empty vector")
}

/// Validates root keys for multi-output recipes (RenderedMappingNode version)
fn validate_multi_output_root_v2(
    root_map: &RenderedMappingNode,
    has_outputs: bool,
) -> Result<(), PartialParsingError> {
    if !has_outputs {
        return Ok(());
    }

    if root_map.contains_key("package") {
        let key = root_map
            .keys()
            .find(|k| k.as_str() == "package")
            .expect("unreachable we preemptively check for if contains");
        return Err(_partialerror!(
            *key.span(),
            ErrorKind::InvalidField("package".to_string().into()),
            help = "recipe cannot have both `outputs` and `package` fields. Rename `package` to `recipe` or remove `outputs`"
        ));
    }

    if root_map.contains_key("requirements") {
        let key = root_map
            .keys()
            .find(|k| k.as_str() == "requirements")
            .expect("unreachable we preemptively check for if contains");
        return Err(_partialerror!(
            *key.span(),
            ErrorKind::InvalidField("requirements".to_string().into()),
            help = "multi-output recipes cannot have a top-level requirements field. Move `requirements` inside the individual output."
        ));
    }

    for key in root_map.keys() {
        if !ALLOWED_KEYS_MULTI_OUTPUTS.contains(&key.as_str()) {
            return Err(_partialerror!(
                *key.span(),
                ErrorKind::InvalidField(key.as_str().to_string().into()),
                help = format!("invalid key `{}` in root node", key.as_str())
            ));
        }
    }

    Ok(())
}

/// Parse outputs from source with support for cache outputs
pub fn find_outputs_v2<S: SourceCode>(src: S) -> Result<Vec<OutputType>, ParsingError<S>> {
    let root_node = RenderedNode::parse_yaml(0, src.clone())?;
    let root_map = parse_root_as_mapping_rendered(&root_node, &src)?;

    let Some(outputs) = root_map.get("outputs") else {
        return Err(ParsingError::from_partial(
            src,
            _partialerror!(
                *root_node.span(),
                ErrorKind::MissingField("outputs".into()),
                help = "v2 recipes must have an 'outputs' field"
            ),
        ));
    };

    if let Err(err) = validate_multi_output_root_v2(root_map, true) {
        return Err(ParsingError::from_partial(src, err));
    }

    let outputs = validate_outputs_sequence_rendered(outputs, &src)?;

    let mut res = Vec::with_capacity(outputs.len());

    let recipe_version = extract_recipe_version_rendered(root_map, &src)?;

    for output in outputs.iter() {
        let Some(output_map) = output.as_mapping() else {
            return Err(ParsingError::from_partial(
                src,
                _partialerror!(
                    *output.span(),
                    ErrorKind::ExpectedMapping,
                    help = "individual `output` must always be a mapping"
                ),
            ));
        };

        let is_cache = output_map.contains_key("cache");
        let is_package = output_map.contains_key("package");

        if is_cache && is_package {
            return Err(ParsingError::from_partial(
                src,
                _partialerror!(
                    *output.span(),
                    ErrorKind::InvalidField("output".into()),
                    help = "output cannot have both 'cache' and 'package' keys"
                ),
            ));
        }

        if !is_cache && !is_package {
            return Err(ParsingError::from_partial(
                src,
                _partialerror!(
                    *output.span(),
                    ErrorKind::InvalidField("output".into()),
                    help = "output must have either 'cache' or 'package' key"
                ),
            ));
        }

        let parsed_output = if is_cache {
            let cache_output: CacheOutput =
                match TryConvertNode::<CacheOutput>::try_convert(output, "outputs.cache") {
                    Ok(cache) => cache,
                    Err(err) => return Err(convert_errors(err, src.clone())),
                };
            OutputType::Cache(Box::new(cache_output))
        } else {
            let output_span = *output.span();
            let mut processed_map = output_map.clone();

            for (key, value) in root_map.iter() {
                if key.as_str() == "outputs"
                    || key.as_str() == "recipe"
                    || key.as_str() == "schema_version"
                    || key.as_str() == "cache"
                {
                    continue;
                }

                if !processed_map.contains_key(key.as_str()) {
                    processed_map.insert(key.clone(), value.clone());
                } else if DEEP_MERGE_KEYS.contains(&key.as_str()) {
                    if let (Some(output_value), Some(root_value_map)) = (
                        processed_map.get(key.as_str()).and_then(|v| v.as_mapping()),
                        value.as_mapping(),
                    ) {
                        let mut merged_map = output_value.clone();
                        merge_rendered_mapping_if_not_exists(&mut merged_map, root_value_map);
                        processed_map.insert(key.clone(), RenderedNode::from(merged_map));
                    }
                }
            }

            if let Some(version) = recipe_version.as_ref() {
                if let Some(package_node) = processed_map.get("package") {
                    if let Some(package_map) = package_node.as_mapping() {
                        if !package_map.contains_key("version") {
                            let mut new_package_map = package_map.clone();
                            new_package_map.insert("version".into(), version.clone());
                            processed_map
                                .insert("package".into(), RenderedNode::from(new_package_map));
                        }
                    }
                } else {
                    return Err(ParsingError::from_partial(
                        src.clone(),
                        _partialerror!(
                            output_span,
                            ErrorKind::MissingField("package".to_string().into())
                        ),
                    ));
                }
            }

            let processed_output_node = RenderedNode::from(processed_map);
            let output_with_inherit: Output =
                match TryConvertNode::<Output>::try_convert(&processed_output_node, "output") {
                    Ok(output) => output,
                    Err(err) => return Err(convert_errors(err, src.clone())),
                };
            OutputType::Package(Box::new(output_with_inherit))
        };

        res.push(parsed_output);
    }

    if let Err(errs) = resolve_inheritance(&mut res) {
        return Err(convert_errors(errs, src));
    }

    Ok(res)
}

/// Resolve inheritance relationships between cache and package outputs
pub fn resolve_inheritance(outputs: &mut [OutputType]) -> Result<(), Vec<PartialParsingError>> {
    let mut cache_outputs = std::collections::HashMap::new();

    for (idx, output) in outputs.iter().enumerate() {
        if let OutputType::Cache(cache) = output {
            if cache_outputs.contains_key(&cache.name) {
                return Err(vec![_partialerror!(
                    marked_yaml::Span::new_blank(),
                    ErrorKind::InvalidField(
                        format!("duplicate cache output name: {}", cache.name).into()
                    ),
                    help = format!(
                        "Each cache output must have a unique name. Rename one of the cache outputs with name '{}'.",
                        cache.name
                    )
                )]);
            }
            cache_outputs.insert(cache.name.clone(), idx);
        }
    }

    let mut inheritance_todo = Vec::new();

    for (idx, output) in outputs.iter().enumerate() {
        if let OutputType::Package(package) = output {
            if let Some(inherit_spec) = &package.inherit {
                let cache_name = inherit_spec.cache_name();

                let cache_idx = *cache_outputs.get(cache_name).ok_or_else(|| {
                    let available_caches = cache_outputs.keys()
                        .map(|name| format!("'{}'", name))
                        .collect::<Vec<_>>()
                        .join(", ");

                    let help_text = if available_caches.is_empty() {
                        format!("No cache outputs are defined in this recipe. Add a cache output with name '{}' or remove the inherit field.", cache_name)
                    } else {
                        format!("Available cache outputs: {}. Make sure the cache name is spelled correctly.", available_caches)
                    };

                    vec![_partialerror!(
                        marked_yaml::Span::new_blank(),
                        ErrorKind::InvalidField(
                            format!("cache output '{}' not found", cache_name).into()
                        ),
                        help = help_text
                    )]
                })?;

                inheritance_todo.push((idx, cache_idx));
            }
        }
    }

    for (pkg_idx, cache_idx) in inheritance_todo {
        let cache_clone = if let OutputType::Cache(cache) = &outputs[cache_idx] {
            (**cache).clone()
        } else {
            continue;
        };

        if let OutputType::Package(package) = &mut outputs[pkg_idx] {
            package.apply_cache_inheritance(&cache_clone);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_output_parsing() {
        let yaml = r#"
outputs:
  - cache:
      name: foo-cache
    source:
      - url: https://foo.bar/source.tar.bz2
        sha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
    requirements:
      build:
        - cmake
      host:
        - libfoo
    build:
      script: build_cache.sh

  - package:
      name: foo-headers
    inherit:
      from: foo-cache
      run_exports: false
    build:
      files:
        - include/

  - package:
      name: foo
    inherit: foo-cache
"#;

        let outputs = find_outputs_v2(yaml).unwrap();
        assert_eq!(outputs.len(), 3);

        assert!(matches!(&outputs[0], OutputType::Cache(_)));
        assert!(matches!(&outputs[1], OutputType::Package(_)));
        assert!(matches!(&outputs[2], OutputType::Package(_)));

        // Check that packages have the cache in their caches list (inheritance is already resolved in find_outputs_v2)
        if let OutputType::Package(pkg) = &outputs[1] {
            assert_eq!(pkg.caches.len(), 1);
            assert_eq!(pkg.caches[0].name, "foo-cache");
        }

        if let OutputType::Package(pkg) = &outputs[2] {
            assert_eq!(pkg.caches.len(), 1);
            assert_eq!(pkg.caches[0].name, "foo-cache");
        }
    }

    #[test]
    fn test_error_messages() {
        let yaml = r#"
outputs:
  - cache:
      name: foo-cache
    build:
      script: build_cache.sh

  - package:
      name: foo
    inherit: non-existent-cache
"#;

        let result = find_outputs_v2(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_str = format!("{:?}", err);
        assert!(err_str.contains("cache output 'non-existent-cache' not found"));
        assert!(err_str.contains("Available cache outputs: 'foo-cache'"));

        let yaml = r#"
outputs:
  - cache:
      name: foo-cache
    build:
      script: build_cache.sh

  - cache:
      name: foo-cache
    build:
      script: another_script.sh
"#;

        let result = find_outputs_v2(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_str = format!("{:?}", err);
        assert!(err_str.contains("duplicate cache output name: foo-cache"));
        assert!(err_str.contains("Each cache output must have a unique name"));

        let yaml = r#"
outputs:
  - cache:
      name: foo-cache
    invalid_field: value
    build:
      script: build_cache.sh
"#;

        let result = find_outputs_v2(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_str = format!("{:?}", err);
        assert!(err_str.contains("InvalidField(\"invalid_field\")"));
        assert!(err_str.contains("valid fields for cache outputs are:"));

        let yaml = r#"
outputs:
  - cache:
      name: foo-cache
    build:
      script: build_cache.sh
      invalid_field: value
"#;

        let result = find_outputs_v2(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_str = format!("{:?}", err);
        assert!(err_str.contains("InvalidField(\"invalid_field\")"));
        assert!(err_str.contains("only 'script' is allowed"));

        let yaml = r#"
outputs:
  - cache:
      name: foo-cache
    build:
      script: build_cache.sh
    requirements:
      invalid_field:
        - package
"#;

        let result = find_outputs_v2(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_str = format!("{:?}", err);
        assert!(err_str.contains("InvalidField(\"invalid_field\")"));
        assert!(err_str.contains("cache outputs can only have 'build' and 'host' requirements"));
    }
}
