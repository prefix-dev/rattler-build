use marked_yaml::Node;

use crate::{
    ParseError,
    span::SpannedString,
    stage0::{
        parser::helpers::get_span,
        source::{GitRev, GitSource, GitUrl, PathSource, Source, UrlSource},
        types::Value,
    },
};

use super::{parse_conditional_list, parse_value};

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
            _ => {
                return Err(ParseError::invalid_value(
                    "git source",
                    &format!("unknown field '{}'", key),
                    (*key_node.span()).into(),
                )
                .with_suggestion(
                    "Valid fields are: git, rev, tag, branch, depth, patches, target_directory, lfs",
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
                sha256 = Some(parse_value(value_node)?);
            }
            "md5" => {
                md5 = Some(parse_value(value_node)?);
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
                    &format!("unknown field '{}'", key),
                    (*key_node.span()).into(),
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
    let mut filter = ConditionalList::default();

    for (key_node, value_node) in mapping.iter() {
        let key = key_node.as_str();

        match key {
            "path" => {
                path = Some(parse_value(value_node)?);
            }
            "sha256" => {
                sha256 = Some(parse_value(value_node)?);
            }
            "md5" => {
                md5 = Some(parse_value(value_node)?);
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
                    ParseError::expected_type("scalar", "non-scalar", get_span(value_node))
                        .with_message("Expected 'use_gitignore' to be a boolean")
                })?;
                let spanned = SpannedString::from(scalar);
                use_gitignore = match spanned.as_str() {
                    "true" | "True" | "yes" | "Yes" => true,
                    "false" | "False" | "no" | "No" => false,
                    _ => {
                        return Err(ParseError::invalid_value(
                            "use_gitignore",
                            &format!("not a valid boolean value (found '{}')", spanned.as_str()),
                            spanned.span(),
                        ));
                    }
                };
            }
            "filter" => {
                filter = parse_conditional_list(value_node)?;
            }
            _ => {
                return Err(ParseError::invalid_value(
                    "path source",
                    &format!("unknown field '{}'", key),
                    (*key_node.span()).into(),
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
