use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use fs_err as fs;
use memmap2::Mmap;
use minijinja::Value as MiniJinjaValue;
use serde::de::DeserializeOwned;
use std::hint::black_box;
use std::io::BufReader;
use std::path::Path;

// Generic parse from reader
fn parse_from_reader<T: DeserializeOwned>(
    file_path: &Path,
    parser: impl FnOnce(BufReader<fs::File>) -> T,
) -> T {
    let file = fs::File::open(file_path).unwrap();
    let reader = BufReader::new(file);
    parser(reader)
}

// Generic parse from string
fn parse_from_string<T: DeserializeOwned>(file_path: &Path, parser: impl FnOnce(&str) -> T) -> T {
    let content = fs::read_to_string(file_path).unwrap();
    parser(&content)
}

// Generic parse from memory map
fn parse_from_mmap<T: DeserializeOwned>(file_path: &Path, parser: impl FnOnce(&[u8]) -> T) -> T {
    let file = fs::File::open(file_path).unwrap();
    let mmap = unsafe { Mmap::map(&file).unwrap() };
    parser(&mmap)
}

// YAML parsers
fn parse_yaml_from_reader(file_path: &Path) -> MiniJinjaValue {
    parse_from_reader(file_path, |r| serde_yaml::from_reader(r).unwrap())
}

fn parse_yaml_from_string(file_path: &Path) -> MiniJinjaValue {
    parse_from_string(file_path, |s| serde_yaml::from_str(s).unwrap())
}

fn parse_yaml_from_mmap(file_path: &Path) -> MiniJinjaValue {
    parse_from_mmap(file_path, |b| serde_yaml::from_slice(b).unwrap())
}

// JSON parsers
fn parse_json_from_reader(file_path: &Path) -> MiniJinjaValue {
    parse_from_reader(file_path, |r| serde_json::from_reader(r).unwrap())
}

fn parse_json_from_string(file_path: &Path) -> MiniJinjaValue {
    parse_from_string(file_path, |s| serde_json::from_str(s).unwrap())
}

fn parse_json_from_mmap(file_path: &Path) -> MiniJinjaValue {
    parse_from_mmap(file_path, |b| serde_json::from_slice(b).unwrap())
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

type Parsers = [(&'static str, fn(&Path) -> MiniJinjaValue); 3];

// Generic benchmark helper
fn run_benchmark_group(c: &mut Criterion, group_name: &str, file_path: &Path, parsers: Parsers) {
    let mut group = c.benchmark_group(group_name);
    group.sample_size(5000);

    let file_size = fs::metadata(file_path).unwrap().len();
    group.throughput(Throughput::Bytes(file_size));

    for (name, parser) in parsers {
        group.bench_function(name, |b| {
            b.iter(|| {
                black_box(parser(file_path));
            });
        });
    }

    group.finish();
}

fn benchmark_yaml_parsing(c: &mut Criterion) {
    let (temp_dir, yaml_path, _) = create_test_files();
    run_benchmark_group(
        c,
        "yaml_parsing",
        &yaml_path,
        [
            ("from_reader", parse_yaml_from_reader),
            ("from_string", parse_yaml_from_string),
            ("from_mmap", parse_yaml_from_mmap),
        ],
    );
    drop(temp_dir);
}

fn benchmark_json_parsing(c: &mut Criterion) {
    let (temp_dir, _, json_path) = create_test_files();
    run_benchmark_group(
        c,
        "json_parsing",
        &json_path,
        [
            ("from_reader", parse_json_from_reader),
            ("from_string", parse_json_from_string),
            ("from_mmap", parse_json_from_mmap),
        ],
    );
    drop(temp_dir);
}

criterion_group!(benches, benchmark_yaml_parsing, benchmark_json_parsing);
criterion_main!(benches);
