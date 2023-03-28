use std::{collections::BTreeMap, path::PathBuf};

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

pub fn load_variant_configs(files: &Vec<PathBuf>) -> BTreeMap<String, Vec<String>> {
    let mut variant_configs = Vec::new();
    for file in files {
        let file = std::fs::File::open(file).unwrap();
        let reader = std::io::BufReader::new(file);
        let yaml_value = serde_yaml::from_reader(reader).unwrap();
        variant_configs.push(value_to_map(yaml_value));
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
