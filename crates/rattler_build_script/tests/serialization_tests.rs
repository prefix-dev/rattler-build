use indexmap::IndexMap;
use rattler_build_script::{Script, ScriptContent};
use std::path::PathBuf;

#[test]
fn test_script_serialization_simple_command() {
    let script = Script {
        interpreter: None,
        env: IndexMap::new(),
        secrets: Vec::new(),
        content: ScriptContent::Command("echo 'Hello World'".to_string()),
        cwd: None,
        content_explicit: false,
    };

    insta::assert_yaml_snapshot!(script, @r###"content: "echo 'Hello World'""###);
}

#[test]
fn test_script_serialization_commands() {
    let script = Script {
        interpreter: None,
        env: IndexMap::new(),
        secrets: Vec::new(),
        content: ScriptContent::Commands(vec![
            "echo 'Step 1'".to_string(),
            "echo 'Step 2'".to_string(),
            "echo 'Step 3'".to_string(),
        ]),
        cwd: None,
        content_explicit: false,
    };

    insta::assert_yaml_snapshot!(script, @r###"
    - "echo 'Step 1'"
    - "echo 'Step 2'"
    - "echo 'Step 3'"
    "###);
}

#[test]
fn test_script_serialization_with_interpreter() {
    let script = Script {
        interpreter: Some("python".to_string()),
        env: IndexMap::new(),
        secrets: Vec::new(),
        content: ScriptContent::Command("print('Hello from Python')".to_string()),
        cwd: None,
        content_explicit: false,
    };

    insta::assert_yaml_snapshot!(script, @r###"
    interpreter: python
    content: "print('Hello from Python')"
    "###);
}

#[test]
fn test_script_serialization_with_env() {
    let mut env = IndexMap::new();
    env.insert("MY_VAR".to_string(), "my_value".to_string());
    env.insert("ANOTHER_VAR".to_string(), "another_value".to_string());

    let script = Script {
        interpreter: None,
        env,
        secrets: Vec::new(),
        content: ScriptContent::Commands(vec!["echo $MY_VAR".to_string()]),
        cwd: None,
        content_explicit: false,
    };

    insta::assert_yaml_snapshot!(script, @r###"
    env:
      MY_VAR: my_value
      ANOTHER_VAR: another_value
    content:
      - echo $MY_VAR
    "###);
}

#[test]
fn test_script_serialization_with_secrets() {
    let script = Script {
        interpreter: None,
        env: IndexMap::new(),
        secrets: vec!["SECRET_TOKEN".to_string(), "API_KEY".to_string()],
        content: ScriptContent::Command("deploy.sh".to_string()),
        cwd: None,
        content_explicit: false,
    };

    insta::assert_yaml_snapshot!(script, @r###"
    secrets:
      - SECRET_TOKEN
      - API_KEY
    content: deploy.sh
    "###);
}

#[test]
fn test_script_serialization_with_path() {
    let script = Script {
        interpreter: Some("bash".to_string()),
        env: IndexMap::new(),
        secrets: Vec::new(),
        content: ScriptContent::Path(PathBuf::from("build.sh")),
        cwd: None,
        content_explicit: false,
    };

    insta::assert_yaml_snapshot!(script, @r###"
    interpreter: bash
    file: build.sh
    "###);
}

#[test]
fn test_script_serialization_with_cwd() {
    let script = Script {
        interpreter: None,
        env: IndexMap::new(),
        secrets: Vec::new(),
        content: ScriptContent::Command("make install".to_string()),
        cwd: Some(PathBuf::from("src/subdir")),
        content_explicit: false,
    };

    insta::assert_yaml_snapshot!(script, @r###"
    content: make install
    cwd: src/subdir
    "###);
}

#[test]
fn test_script_serialization_full() {
    let mut env = IndexMap::new();
    env.insert("BUILD_TYPE".to_string(), "release".to_string());

    let script = Script {
        interpreter: Some("bash".to_string()),
        env,
        secrets: vec!["DEPLOY_KEY".to_string()],
        content: ScriptContent::Commands(vec![
            "mkdir -p build".to_string(),
            "cd build".to_string(),
            "cmake ..".to_string(),
            "make -j$(nproc)".to_string(),
        ]),
        cwd: Some(PathBuf::from("project")),
        content_explicit: false,
    };

    insta::assert_yaml_snapshot!(script, @r###"
    interpreter: bash
    env:
      BUILD_TYPE: release
    secrets:
      - DEPLOY_KEY
    content:
      - mkdir -p build
      - cd build
      - cmake ..
      - make -j$(nproc)
    cwd: project
    "###);
}

#[test]
fn test_script_deserialization_simple() {
    let yaml = r#"
        content: echo 'Hello'
    "#;

    let script: Script = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(script.interpreter, None);
    assert_eq!(script.env.len(), 0);
    assert_eq!(script.secrets.len(), 0);
    assert!(matches!(script.content, ScriptContent::Command(_)));
}

#[test]
fn test_script_deserialization_with_all_fields() {
    let yaml = r#"
        interpreter: python
        env:
          VAR1: value1
          VAR2: value2
        secrets:
          - SECRET1
        content:
          - echo step1
          - echo step2
        cwd: workdir
    "#;

    let script: Script = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(script.interpreter, Some("python".to_string()));
    assert_eq!(script.env.len(), 2);
    assert_eq!(script.secrets.len(), 1);
    assert_eq!(script.cwd, Some(PathBuf::from("workdir")));

    if let ScriptContent::Commands(commands) = script.content {
        assert_eq!(commands.len(), 2);
    } else {
        panic!("Expected Commands variant");
    }
}

#[test]
fn test_script_roundtrip() {
    let mut env = IndexMap::new();
    env.insert("KEY".to_string(), "VALUE".to_string());

    let original = Script {
        interpreter: Some("bash".to_string()),
        env,
        secrets: vec!["SECRET".to_string()],
        content: ScriptContent::Commands(vec!["cmd1".to_string(), "cmd2".to_string()]),
        cwd: Some(PathBuf::from("dir")),
        content_explicit: false,
    };

    let serialized = serde_yaml::to_string(&original).unwrap();
    let deserialized: Script = serde_yaml::from_str(&serialized).unwrap();

    assert_eq!(original, deserialized);
}
