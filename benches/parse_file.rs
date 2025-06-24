use anyhow::Result;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use fs_err as fs;
use memmap2::Mmap;
use minijinja::Value as MiniJinjaValue;
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;
use std::hint::black_box;
use std::io::BufReader;
use std::path::Path;

// Parse YAML from reader
fn parse_yaml_from_reader(file_path: &Path) -> Result<YamlValue> {
    let file = fs::File::open(file_path)?;
    let reader = BufReader::new(file);
    Ok(serde_yaml::from_reader(reader)?)
}

// Parse YAML from string
fn parse_yaml_from_string(file_path: &Path) -> Result<YamlValue> {
    let content = fs::read_to_string(file_path)?;
    Ok(serde_yaml::from_str(&content)?)
}

// Parse JSON from reader
fn parse_json_from_reader(file_path: &Path) -> Result<JsonValue> {
    let file = fs::File::open(file_path)?;
    let reader = BufReader::new(file);
    Ok(serde_json::from_reader(reader)?)
}

// Parse JSON from string
fn parse_json_from_string(file_path: &Path) -> Result<JsonValue> {
    let content = fs::read_to_string(file_path)?;
    Ok(serde_json::from_str(&content)?)
}

// Helper function to determine the file format based on the file extension
fn get_file_format(file_path: &Path) -> &'static str {
    file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| match ext.to_lowercase().as_str() {
            "yaml" | "yml" => "yaml",
            "json" => "json",
            _ => "unknown",
        })
        .unwrap_or("unknown")
}

// Read and parse the file based on its format
fn read_and_parse_file(file_path: &Path) -> Result<MiniJinjaValue> {
    let file = fs::File::open(file_path)?;
    let reader = BufReader::new(file);

    match get_file_format(file_path) {
        "yaml" => Ok(serde_yaml::from_reader(reader)?),
        "json" => Ok(serde_json::from_reader(reader)?),
        _ => {
            let content = fs::read_to_string(file_path)?;
            Ok(MiniJinjaValue::from(content))
        }
    }
}

// Parse YAML to MiniJinja Value from reader
fn parse_yaml_to_minijinja_from_reader(file_path: &Path) -> Result<MiniJinjaValue> {
    read_and_parse_file(file_path)
}

// Parse YAML to MiniJinja Value from string
fn parse_yaml_to_minijinja_from_string(file_path: &Path) -> Result<MiniJinjaValue> {
    read_and_parse_file(file_path)
}

// Parse JSON to MiniJinja Value from reader
fn parse_json_to_minijinja_from_reader(file_path: &Path) -> Result<MiniJinjaValue> {
    read_and_parse_file(file_path)
}

// Parse JSON to MiniJinja Value from string
fn parse_json_to_minijinja_from_string(file_path: &Path) -> Result<MiniJinjaValue> {
    read_and_parse_file(file_path)
}

// Parse YAML using memory mapping
fn parse_yaml_from_mmap(file_path: &Path) -> Result<YamlValue> {
    let file = fs::File::open(file_path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    Ok(serde_yaml::from_slice(&mmap)?)
}

// Parse JSON using memory mapping
fn parse_json_from_mmap(file_path: &Path) -> Result<JsonValue> {
    let file = fs::File::open(file_path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    Ok(serde_json::from_slice(&mmap)?)
}

// Parse YAML to MiniJinja Value using memory mapping
fn parse_yaml_to_minijinja_from_mmap(file_path: &Path) -> Result<MiniJinjaValue> {
    let file = fs::File::open(file_path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    Ok(serde_yaml::from_slice(&mmap)?)
}

// Parse JSON to MiniJinja Value using memory mapping
fn parse_json_to_minijinja_from_mmap(file_path: &Path) -> Result<MiniJinjaValue> {
    let file = fs::File::open(file_path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    Ok(serde_json::from_slice(&mmap)?)
}

fn create_test_files() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp_dir = tempfile::tempdir().unwrap();

    // Create a test YAML file
    let yaml_path = temp_dir.path().join("test.yaml");
    let yaml_content = r#"
name: test-package
version: 1.0.0
description: A test package for benchmarking
dependencies:
  - dep1 >=1.0.0
  - dep2 >=2.0.0
  - dep3 >=3.0.0
build:
  number: 0
  script:
    - echo "Building test package"
    - make install
test:
  commands:
    - test -f $PREFIX/bin/test-package
about:
  home: https://example.com
  license: MIT
  summary: Test package for benchmarking
"#;
    fs::write(&yaml_path, yaml_content).unwrap();

    // Create a test JSON file
    let json_path = temp_dir.path().join("test.json");
    let json_content = r#"
{
  "name": "test-package",
  "version": "1.0.0",
  "description": "A test package for benchmarking",
  "dependencies": [
    "dep1 >=1.0.0",
    "dep2 >=2.0.0",
    "dep3 >=3.0.0"
  ],
  "build": {
    "number": 0,
    "script": [
      "echo \"Building test package\"",
      "make install"
    ]
  },
  "test": {
    "commands": [
      "test -f $PREFIX/bin/test-package"
    ]
  },
  "about": {
    "home": "https://example.com",
    "license": "MIT",
    "summary": "Test package for benchmarking"
  }
}
"#;
    fs::write(&json_path, json_content).unwrap();

    (temp_dir, yaml_path, json_path)
}

fn benchmark_yaml_parsing(c: &mut Criterion) {
    let (temp_dir, yaml_path, _) = create_test_files();
    let mut group = c.benchmark_group("yaml_parsing");
    group.sample_size(5000);

    let file_size = fs::metadata(&yaml_path).unwrap().len();
    group.throughput(Throughput::Bytes(file_size));

    group.bench_function("from_reader", |b| {
        b.iter(|| {
            black_box(parse_yaml_from_reader(&yaml_path).unwrap());
        });
    });

    group.bench_function("from_string", |b| {
        b.iter(|| {
            black_box(parse_yaml_from_string(&yaml_path).unwrap());
        });
    });

    group.bench_function("from_mmap", |b| {
        b.iter(|| {
            black_box(parse_yaml_from_mmap(&yaml_path).unwrap());
        });
    });

    group.finish();
    drop(temp_dir);
}

fn benchmark_json_parsing(c: &mut Criterion) {
    let (temp_dir, _, json_path) = create_test_files();
    let mut group = c.benchmark_group("json_parsing");
    group.sample_size(5000);

    let file_size = fs::metadata(&json_path).unwrap().len();
    group.throughput(Throughput::Bytes(file_size));

    group.bench_function("from_reader", |b| {
        b.iter(|| {
            black_box(parse_json_from_reader(&json_path).unwrap());
        });
    });

    group.bench_function("from_string", |b| {
        b.iter(|| {
            black_box(parse_json_from_string(&json_path).unwrap());
        });
    });

    group.bench_function("from_mmap", |b| {
        b.iter(|| {
            black_box(parse_json_from_mmap(&json_path).unwrap());
        });
    });

    group.finish();
    drop(temp_dir);
}

fn benchmark_yaml_to_minijinja_parsing(c: &mut Criterion) {
    let (temp_dir, yaml_path, _) = create_test_files();
    let mut group = c.benchmark_group("yaml_to_minijinja_parsing");
    group.sample_size(5000);

    let file_size = fs::metadata(&yaml_path).unwrap().len();
    group.throughput(Throughput::Bytes(file_size));

    group.bench_function("from_reader", |b| {
        b.iter(|| {
            black_box(parse_yaml_to_minijinja_from_reader(&yaml_path).unwrap());
        });
    });

    group.bench_function("from_string", |b| {
        b.iter(|| {
            black_box(parse_yaml_to_minijinja_from_string(&yaml_path).unwrap());
        });
    });

    group.bench_function("from_mmap", |b| {
        b.iter(|| {
            black_box(parse_yaml_to_minijinja_from_mmap(&yaml_path).unwrap());
        });
    });

    group.finish();
    drop(temp_dir);
}

fn benchmark_json_to_minijinja_parsing(c: &mut Criterion) {
    let (temp_dir, _, json_path) = create_test_files();
    let mut group = c.benchmark_group("json_to_minijinja_parsing");
    group.sample_size(5000);

    let file_size = fs::metadata(&json_path).unwrap().len();
    group.throughput(Throughput::Bytes(file_size));

    group.bench_function("from_reader", |b| {
        b.iter(|| {
            black_box(parse_json_to_minijinja_from_reader(&json_path).unwrap());
        });
    });

    group.bench_function("from_string", |b| {
        b.iter(|| {
            black_box(parse_json_to_minijinja_from_string(&json_path).unwrap());
        });
    });

    group.bench_function("from_mmap", |b| {
        b.iter(|| {
            black_box(parse_json_to_minijinja_from_mmap(&json_path).unwrap());
        });
    });

    group.finish();
    drop(temp_dir);
}

criterion_group!(
    benches,
    benchmark_yaml_parsing,
    benchmark_json_parsing,
    benchmark_yaml_to_minijinja_parsing,
    benchmark_json_to_minijinja_parsing
);
criterion_main!(benches);
