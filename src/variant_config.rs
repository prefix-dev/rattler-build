use std::{collections::BTreeMap, path::PathBuf};

use crate::selectors::{flatten_selectors, SelectorConfig};

/// This function normalizes a Map of String and List values to a List of Strings
/// For example,
///
/// ```yaml
/// compiler:
///  - gcc
///  - clang
/// python: "3.9"
/// ```
///
/// becomes
///
/// ```yaml
/// compiler:
/// - gcc
/// - clang
/// python:
/// - "3.9"
/// ```
fn value_to_map(value: serde_yaml::Value) -> BTreeMap<String, Vec<String>> {
    let mut map = BTreeMap::new();
    for (key, value) in value.as_mapping().unwrap() {
        let key = key.as_str().unwrap().to_string();
        match value {
            serde_yaml::Value::String(value) => {
                map.insert(key, vec![value.to_string()]);
                continue;
            }
            serde_yaml::Value::Sequence(value) => {
                let value = value
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect();
                map.insert(key, value);
                continue;
            }
            _ => {
                panic!("Invalid value type");
            }
        }
    }
    map
}

/// This function loads multiple variant configuration files and merges them into a single
/// configuration. The configuration files are loaded in the order they are provided in the
/// `files` argument. The `selector_config` argument is used to select the correct configuration
/// for the target platform.
///
/// A variant configuration file is a YAML file that contains a mapping of package names to
/// a list of variants. For example:
///
/// ```yaml
/// python:
/// - "3.9"
/// - "3.8"
/// ```
///
/// The above configuration file will select the `python` package with the variants `3.9` and
/// `3.8`.
///
/// The `selector_config` argument is used to select the correct configuration for the target
/// platform. For example, if the `selector_config` is `unix`, the following configuration file:
///
/// ```yaml
/// sel(unix):
///   python:
///   - "3.9"
///   - "3.8"
/// sel(win):
///   python:
///   - "3.9"
/// ```
///
/// will be flattened to:
///
/// ```yaml
/// python:
/// - "3.9"
/// - "3.8"
/// ```
///
/// The `files` argument is a list of paths to the variant configuration files. The files are
/// loaded in the order they are provided in the `files` argument. The keys of a later file
/// replace keys from an earlier file (values are _not_ merged).
///
/// A special key, the `zip_keys` is used to "zip" the values of two keys. For example, if the
/// following configuration file is loaded:
///
/// ```yaml
/// compiler:
/// - gcc
/// - clang
/// python:
/// - "3.9"
/// - "3.8"
/// zip_keys:
/// - [compiler, python]
/// ```
///
/// the variant configuration will be zipped so that the following variants are selected:
///
/// ```txt
/// [python=3.9, compiler=gcc]
/// and
/// [python=3.8, compiler=clang]
/// ```
pub fn load_variant_configs(
    files: &Vec<PathBuf>,
    selector_config: &SelectorConfig,
) -> BTreeMap<String, Vec<String>> {
    let mut variant_configs = Vec::new();

    for file in files {
        let file = std::fs::File::open(file).unwrap();
        let reader = std::io::BufReader::new(file);
        let mut yaml_value = serde_yaml::from_reader(reader).unwrap();
        if let Some(yaml_value) = flatten_selectors(&mut yaml_value, selector_config) {
            variant_configs.push(value_to_map(yaml_value));
        }
    }

    let mut variant_config = BTreeMap::new();
    for config in variant_configs {
        for (key, value) in config {
            variant_config.insert(key, value);
        }
    }

    variant_config
}

#[cfg(test)]
mod tests {
    use crate::metadata::PlatformOrNoarch;
    use crate::selectors::{flatten_toplevel, SelectorConfig};
    use rattler_conda_types::Platform;
    use rstest::rstest;
    use serde_yaml::Value as YamlValue;

    #[rstest]
    #[case("selectors/config_1.yaml")]
    fn test_flatten_selectors(#[case] filename: &str) {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let yaml_file = std::fs::read_to_string(test_data_dir.join(filename)).unwrap();
        let mut yaml: YamlValue = serde_yaml::from_str(&yaml_file).unwrap();

        let selector_config = SelectorConfig {
            target_platform: PlatformOrNoarch::Platform(Platform::Linux64),
            build_platform: Platform::Linux64,
            variant: vec![("python_version".into(), "3.8.5".into())]
                .into_iter()
                .collect(),
        };

        let res = flatten_toplevel(&mut yaml, &selector_config);
        // set_snapshot_suffix!("{}", filename.replace('/', "_"));
        insta::assert_yaml_snapshot!(res);
    }
}
