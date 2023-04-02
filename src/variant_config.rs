use std::{collections::BTreeMap, path::PathBuf};

use crate::selectors::{flatten_selectors, SelectorConfig};

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
            variant_config
                .entry(key)
                .or_insert(Vec::new())
                .extend(value);
        }
    }

    variant_config
}

#[cfg(test)]
mod tests {
    use crate::selectors::{flatten_toplevel, SelectorConfig};
    use rstest::rstest;
    use serde_yaml::Value as YamlValue;

    #[rstest]
    #[case("selectors/config_1.yaml")]
    fn test_flatten_selectors(#[case] filename: &str) {
        let test_data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data");
        let yaml_file = std::fs::read_to_string(test_data_dir.join(filename)).unwrap();
        let mut yaml: YamlValue = serde_yaml::from_str(&yaml_file).unwrap();

        let selector_config = SelectorConfig {
            target_platform: "linux-64".into(),
            build_platform: "linux-64".into(),
            variant: vec![("python_version".into(), "3.8.5".into())]
                .into_iter()
                .collect(),
        };

        let res = flatten_toplevel(&mut yaml, &selector_config);
        // set_snapshot_suffix!("{}", filename.replace('/', "_"));
        insta::assert_yaml_snapshot!(res);
    }
}
