//! Output parsing is a bit more complicated than the other sections.
//!
//! The reason for this is that the `outputs` field is a list of mappings, and
//! each mapping can have its own `package`, `source`, `build`, `requirements`,
//! `test`, and `about` fields.
//!
//! (GrayJack): I think that the best way to do the merges are in the original Node

use crate::{
    _partialerror,
    recipe::{custom_yaml::Node, error::ErrorKind, ParsingError},
};

static DEEP_MERGE_KEYS: [&str; 4] = ["package", "about", "extra", "build"];

pub fn find_outputs_from_src(src: &str) -> Result<Vec<Node>, ParsingError> {
    let root_node = marked_yaml::parse_yaml(0, src)
        .map_err(|err| crate::recipe::error::load_error_handler(src, err))?;

    let root_map = root_node.as_mapping().ok_or_else(|| {
        ParsingError::from_partial(
            src,
            _partialerror!(
                *root_node.span(),
                ErrorKind::ExpectedMapping,
                help = "root node must always be a mapping"
            ),
        )
    })?;

    let Some(outputs) = root_map.get("outputs") else {
        let recipe =
            Node::try_from(root_node).map_err(|err| ParsingError::from_partial(src, err))?;
        return Ok(vec![recipe]);
    };

    // TODO: Schema
    let outputs = outputs.as_sequence().ok_or_else(|| {
        ParsingError::from_partial(
            src,
            _partialerror!(
                *outputs.span(),
                ErrorKind::ExpectedSequence,
                help = "`outputs` must always be a sequence"
            ),
        )
    })?;

    let mut res = Vec::with_capacity(outputs.len());

    // the schema says that `outputs` can be either an output, a if-selector or a sequence
    // of outputs and if-selectors. We need to handle all of these cases but for now, lets
    // handle only sequence of outputs
    for output in outputs.iter() {
        // 1. clone the root node
        // 2. remove the `outputs` key
        // 3. substitute repeated value (make sure to preserve the spans)
        // 4. merge skip values (make sure to preserve the spans)
        // Note: Make sure to preserve the spans of the original root span so the error
        // messages remain accurate and point the correct part of the original recipe src

        let mut root = root_map.clone();
        root.remove("outputs");

        // recipe.version, if exists in root, and package.version doesn't exist in output, we will
        // use that instead
        // ignore recipe.name
        let version = root
            .get("recipe")
            .and_then(|recipe| recipe.as_mapping())
            .and_then(|recipe| recipe.get("version"));

        let mut output_node = output.clone();

        let output_map = output_node.as_mapping_mut().ok_or_else(|| {
            ParsingError::from_partial(
                src,
                _partialerror!(
                    *output.span(),
                    ErrorKind::ExpectedMapping,
                    help = "`outputs` must always be a mapping"
                ),
            )
        })?;

        for (key, value) in root.iter() {
            if !output_map.contains_key(key) {
                output_map.insert(key.clone(), value.clone());
            } else {
                // deep merge
                if DEEP_MERGE_KEYS.contains(&key.as_str()) {
                    let output_map_span = *output_map.span();
                    let output_value = output_map.get_mut(key).ok_or_else(|| {
                        ParsingError::from_partial(
                            src,
                            _partialerror!(
                                output_map_span,
                                ErrorKind::MissingField(key.as_str().to_owned().into()),
                            ),
                        )
                    })?;
                    let output_value_span = *output_value.span();
                    let output_value_map = output_value.as_mapping_mut().ok_or_else(|| {
                        ParsingError::from_partial(
                            src,
                            _partialerror!(output_value_span, ErrorKind::ExpectedMapping,),
                        )
                    })?;

                    let mut root_value = value.clone();
                    let root_value_map = root_value.as_mapping_mut().ok_or_else(|| {
                        ParsingError::from_partial(
                            src,
                            _partialerror!(*value.span(), ErrorKind::ExpectedMapping,),
                        )
                    })?;

                    for (key, value) in root_value_map.iter() {
                        if !output_value_map.contains_key(key) {
                            output_value_map.insert(key.clone(), value.clone());
                        }
                    }
                }
            }
        }

        if let Some(version) = version {
            let Some(package_map) = output_map
                .get_mut("package")
                .and_then(|node| node.as_mapping_mut())
            else {
                return Err(ParsingError::from_partial(
                    src,
                    _partialerror!(
                        *output_node.span(),
                        ErrorKind::MissingField("package".to_string().into())
                    ),
                ));
            };

            if !package_map.contains_key("version") {
                package_map.insert("version".into(), version.clone());
            }
        }

        output_map.remove("recipe");

        let recipe =
            Node::try_from(output_node).map_err(|err| ParsingError::from_partial(src, err))?;
        res.push(recipe);
    }
    Ok(res)
}
