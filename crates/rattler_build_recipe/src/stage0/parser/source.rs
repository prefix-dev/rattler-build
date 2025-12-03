use marked_yaml::Node;
use rattler_build_yaml_parser::ParseError;
use rattler_digest::{Md5, Md5Hash, Sha256, Sha256Hash};

use crate::stage0::{
    parser::helpers::get_span,
    source::{GitRev, GitSource, GitUrl, PathSource, Source, UrlSource},
    types::{IncludeExclude, JinjaTemplate, Value},
};

use rattler_build_yaml_parser::{parse_conditional_list, parse_value};

/// Parse a SHA256 hash value (can be concrete or template)
fn parse_sha256_value(node: &Node) -> Result<Value<Sha256Hash>, ParseError> {
    // Check if it's a template
    if let Some(scalar) = node.as_scalar() {
        let s = scalar.as_str();
        let span = *scalar.span();

        // Check if it contains Jinja template syntax
        if s.contains("${{") {
            let template = JinjaTemplate::new(s.to_string())
                .map_err(|e| ParseError::invalid_value("sha256", &e, span))?;
            return Ok(Value::new_template(template, Some(span)));
        }

        // Otherwise parse as concrete SHA256 hash
        let hash = rattler_digest::parse_digest_from_hex::<Sha256>(s).ok_or_else(|| {
            ParseError::invalid_value("sha256", format!("Invalid SHA256 checksum: {}", s), span)
        })?;
        Ok(Value::new_concrete(hash, Some(span)))
    } else {
        Err(ParseError::expected_type(
            "scalar",
            "non-scalar",
            get_span(node),
        ))
    }
}

/// Parse an MD5 hash value (can be concrete or template)
fn parse_md5_value(node: &Node) -> Result<Value<Md5Hash>, ParseError> {
    // Check if it's a template
    if let Some(scalar) = node.as_scalar() {
        let s = scalar.as_str();
        let span = *scalar.span();

        // Check if it contains Jinja template syntax
        if s.contains("${{") {
            let template = JinjaTemplate::new(s.to_string())
                .map_err(|e| ParseError::invalid_value("md5", &e, span))?;
            return Ok(Value::new_template(template, Some(span)));
        }

        // Otherwise parse as concrete MD5 hash
        let hash = rattler_digest::parse_digest_from_hex::<Md5>(s).ok_or_else(|| {
            ParseError::invalid_value("md5", format!("Invalid MD5 checksum: {}", s), span)
        })?;
        Ok(Value::new_concrete(hash, Some(span)))
    } else {
        Err(ParseError::expected_type(
            "scalar",
            "non-scalar",
            get_span(node),
        ))
    }
}

/// Parse source filter field - can be a list or include/exclude mapping
fn parse_source_filter(node: &Node) -> Result<IncludeExclude, ParseError> {
    // Try parsing as a mapping with include/exclude first
    if let Some(mapping) = node.as_mapping() {
        let mut include = None;
        let mut exclude = None;

        for (key_node, value_node) in mapping.iter() {
            let key = key_node.as_str();

            match key {
                "include" => {
                    include = Some(parse_conditional_list(value_node)?);
                }
                "exclude" => {
                    exclude = Some(parse_conditional_list(value_node)?);
                }
                _ => {
                    return Err(ParseError::invalid_value(
                        "filter",
                        format!("unknown field '{}' in filter mapping", key),
                        *key_node.span(),
                    )
                    .with_suggestion("Valid fields are: include, exclude"));
                }
            }
        }

        return Ok(IncludeExclude::Mapping {
            include: include.unwrap_or_default(),
            exclude: exclude.unwrap_or_default(),
        });
    }

    // Otherwise parse as a simple list
    if node.as_sequence().is_some() {
        return Ok(IncludeExclude::List(parse_conditional_list(node)?));
    }

    Err(ParseError::expected_type(
        "sequence or mapping with include/exclude",
        "other",
        get_span(node),
    )
    .with_message(
        "filter must be either a list of glob patterns or a mapping with include/exclude keys",
    ))
}

/// Parse source section from YAML (can be single or list)
pub fn parse_source(node: &Node) -> Result<Vec<Source>, ParseError> {
    match node {
        Node::Sequence(seq) => {
            let mut sources = Vec::new();
            for item in seq.iter() {
                sources.push(parse_single_source(item)?);
            }
            Ok(sources)
        }
        Node::Mapping(_) => Ok(vec![parse_single_source(node)?]),
        _ => Err(ParseError::expected_type(
            "mapping or sequence",
            "non-mapping/sequence",
            get_span(node),
        )
        .with_message("Expected 'source' to be a mapping or sequence")),
    }
}

fn parse_single_source(node: &Node) -> Result<Source, ParseError> {
    let mapping = node.as_mapping().ok_or_else(|| {
        ParseError::expected_type("mapping", "non-mapping", get_span(node))
            .with_message("Each source must be a mapping")
    })?;

    // Determine source type by checking which field is present
    if mapping.get("git").is_some() {
        Ok(Source::Git(parse_git_source(mapping)?))
    } else if mapping.get("url").is_some() {
        Ok(Source::Url(parse_url_source(mapping)?))
    } else if mapping.get("path").is_some() {
        Ok(Source::Path(parse_path_source(mapping)?))
    } else {
        Err(
            ParseError::invalid_value("source", "missing git, url, or path field", get_span(node))
                .with_suggestion("Source must have one of: git, url, or path"),
        )
    }
}

fn parse_git_source(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> Result<GitSource, ParseError> {
    use crate::stage0::types::ConditionalList;

    let mut url = None;
    let mut rev = None;
    let mut tag = None;
    let mut branch = None;
    let mut depth = None;
    let mut patches = ConditionalList::default();
    let mut target_directory = None;
    let mut lfs = None;
    let mut expected_commit = None;

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();

        match key {
            "git" => {
                let url_value: Value<String> = parse_value(value_node)?;
                url = Some(GitUrl(url_value));
            }
            "rev" => {
                rev = Some(GitRev::Value(parse_value(value_node)?));
            }
            "tag" => {
                tag = Some(GitRev::Value(parse_value(value_node)?));
            }
            "branch" => {
                branch = Some(GitRev::Value(parse_value(value_node)?));
            }
            "depth" => {
                depth = Some(parse_value(value_node)?);
            }
            "patches" => {
                patches = parse_conditional_list(value_node)?;
            }
            "target_directory" => {
                target_directory = Some(parse_value(value_node)?);
            }
            "lfs" => {
                lfs = Some(parse_value(value_node)?);
            }
            "expected_commit" => {
                expected_commit = Some(parse_value(value_node)?);
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "git source",
                    format!("unknown field '{}'", key),
                    *key_node.span(),
                )
                .with_suggestion(
                    "Valid fields are: git, rev, tag, branch, depth, patches, target_directory, lfs, expected_commit",
                ));
            }
        }
    }

    let url = url.ok_or_else(|| {
        ParseError::missing_field("git", get_span(&Node::Mapping(mapping.clone())))
    })?;

    // Check for conflicting rev/tag/branch
    let rev_count = [rev.is_some(), tag.is_some(), branch.is_some()]
        .iter()
        .filter(|&&x| x)
        .count();
    if rev_count > 1 {
        return Err(ParseError::invalid_value(
            "git source",
            "cannot specify more than one of: rev, tag, branch",
            get_span(&Node::Mapping(mapping.clone())),
        ));
    }

    Ok(GitSource {
        url,
        rev,
        tag,
        branch,
        depth,
        patches,
        target_directory,
        lfs,
        expected_commit,
    })
}

fn parse_url_source(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> Result<UrlSource, ParseError> {
    use crate::stage0::types::ConditionalList;

    let mut url = Vec::new();
    let mut sha256 = None;
    let mut md5 = None;
    let mut file_name = None;
    let mut patches = ConditionalList::default();
    let mut target_directory = None;

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();

        match key {
            "url" => {
                // URL can be a single value or a list
                if let Some(seq) = value_node.as_sequence() {
                    for item in seq.iter() {
                        url.push(parse_value(item)?);
                    }
                } else {
                    url.push(parse_value(value_node)?);
                }
            }
            "sha256" => {
                sha256 = Some(parse_sha256_value(value_node)?);
            }
            "md5" => {
                md5 = Some(parse_md5_value(value_node)?);
            }
            "file_name" => {
                file_name = Some(parse_value(value_node)?);
            }
            "patches" => {
                patches = parse_conditional_list(value_node)?;
            }
            "target_directory" => {
                target_directory = Some(parse_value(value_node)?);
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "url source",
                    format!("unknown field '{}'", key),
                    *key_node.span(),
                )
                .with_suggestion(
                    "Valid fields are: url, sha256, md5, file_name, patches, target_directory",
                ));
            }
        }
    }

    if url.is_empty() {
        return Err(ParseError::missing_field(
            "url",
            get_span(&Node::Mapping(mapping.clone())),
        ));
    }

    Ok(UrlSource {
        url,
        sha256,
        md5,
        file_name,
        patches,
        target_directory,
    })
}

fn parse_path_source(
    mapping: &marked_yaml::types::MarkedMappingNode,
) -> Result<PathSource, ParseError> {
    use crate::stage0::types::ConditionalList;

    let mut path = None;
    let mut sha256 = None;
    let mut md5 = None;
    let mut patches = ConditionalList::default();
    let mut target_directory = None;
    let mut file_name = None;
    let mut use_gitignore = true;
    let mut filter = IncludeExclude::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();

        match key {
            "path" => {
                path = Some(parse_value(value_node)?);
            }
            "sha256" => {
                sha256 = Some(parse_sha256_value(value_node)?);
            }
            "md5" => {
                md5 = Some(parse_md5_value(value_node)?);
            }
            "patches" => {
                patches = parse_conditional_list(value_node)?;
            }
            "target_directory" => {
                target_directory = Some(parse_value(value_node)?);
            }
            "file_name" => {
                file_name = Some(parse_value(value_node)?);
            }
            "use_gitignore" => {
                let scalar = value_node.as_scalar().ok_or_else(|| {
                    ParseError::expected_type("boolean", "non-scalar", get_span(value_node))
                })?;
                use_gitignore = match scalar.as_bool() {
                    Some(b) => b,
                    None => {
                        return Err(ParseError::invalid_value(
                            "use_gitignore",
                            format!("expected boolean, got '{}'", scalar.as_str()),
                            *value_node.span(),
                        ));
                    }
                };
            }
            "filter" => {
                filter = parse_source_filter(value_node)?;
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "path source",
                    format!("unknown field '{}'", key),
                    *key_node.span(),
                )
                .with_suggestion(
                    "Valid fields are: path, sha256, md5, patches, target_directory, file_name, use_gitignore, filter",
                ));
            }
        }
    }

    let path = path.ok_or_else(|| {
        ParseError::missing_field("path", get_span(&Node::Mapping(mapping.clone())))
    })?;

    Ok(PathSource {
        path,
        sha256,
        md5,
        patches,
        target_directory,
        file_name,
        use_gitignore,
        filter,
    })
}
