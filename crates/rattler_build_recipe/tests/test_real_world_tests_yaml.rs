/// End-to-end test using a real tests.yaml file rendered by old rattler-build
/// This ensures our parsing is backwards compatible with existing packages
#[cfg(test)]
mod test {
    use rattler_build_recipe::stage1::tests::TestType;

    #[test]
    fn test_parse_real_world_tests_yaml() {
        // This is the actual tests.yaml from test-render/info/tests/tests.yaml
        // rendered by an old version of rattler-build
        let tests_yaml = r#"
- python:
    imports:
    - numpy.testing
- python:
    imports:
    - numpy.matrix
    python_version:
    - 3.8.*
- python:
    imports:
    - numpy.matrix
    python_version: 3.8.*
- script:
    content: echo "FOO"
- script:
    interpreter: python
    content: import numpy as np
- script:
    interpreter: python
    env:
      FOO: BAR
      BAZ: QUX
    secrets:
    - ABC
    - DEF
    content: print("i am the test")
- downstream: foo
- perl:
    uses:
    - Test::More
- r:
    libraries:
    - testthat
- ruby:
    requires:
    - minitest
"#;

        println!("\n=== Parsing real-world tests.yaml from old rattler-build ===");

        let tests: Vec<TestType> = serde_yaml::from_str(tests_yaml)
            .expect("Failed to parse tests.yaml - backwards compatibility broken!");

        println!("Found {} tests", tests.len());
        assert_eq!(tests.len(), 10, "Expected 10 tests");

        // Verify each test type in detail

        // Test 0: Python test with numpy.testing import
        println!("\nTest 0:");
        match &tests[0] {
            TestType::Python { python } => {
                println!("  ✓ Python test");
                assert_eq!(python.imports, vec!["numpy.testing"]);
                assert!(python.pip_check);
            }
            _ => panic!("Expected Python test at index 0"),
        }

        // Test 1: Python test with python_version as array
        println!("\nTest 1:");
        match &tests[1] {
            TestType::Python { python } => {
                println!("  ✓ Python test with version array");
                assert_eq!(python.imports, vec!["numpy.matrix"]);
                match &python.python_version {
                    rattler_build_recipe::stage1::tests::PythonVersion::Multiple(versions) => {
                        assert_eq!(versions, &vec!["3.8.*"]);
                    }
                    _ => panic!("Expected Multiple python_version"),
                }
            }
            _ => panic!("Expected Python test at index 1"),
        }

        // Test 2: Python test with python_version as single value
        println!("\nTest 2:");
        match &tests[2] {
            TestType::Python { python } => {
                println!("  ✓ Python test with single version");
                assert_eq!(python.imports, vec!["numpy.matrix"]);
                match &python.python_version {
                    rattler_build_recipe::stage1::tests::PythonVersion::Single(version) => {
                        assert_eq!(version, "3.8.*");
                    }
                    _ => panic!("Expected Single python_version"),
                }
            }
            _ => panic!("Expected Python test at index 2"),
        }

        // Test 3: Script test with simple content
        println!("\nTest 3:");
        match &tests[3] {
            TestType::Commands(cmd) => {
                println!("  ✓ Commands test with simple script");
                use rattler_build_script::ScriptContent;
                match &cmd.script.content {
                    ScriptContent::Command(content) => {
                        assert_eq!(content, "echo \"FOO\"");
                    }
                    _ => panic!("Expected Command content"),
                }
            }
            _ => panic!("Expected Commands test at index 3"),
        }

        // Test 4: Script test with interpreter
        println!("\nTest 4:");
        match &tests[4] {
            TestType::Commands(cmd) => {
                println!("  ✓ Commands test with interpreter");
                assert_eq!(cmd.script.interpreter, Some("python".to_string()));
            }
            _ => panic!("Expected Commands test at index 4"),
        }

        // Test 5: Script test with env and secrets
        println!("\nTest 5:");
        match &tests[5] {
            TestType::Commands(cmd) => {
                println!("  ✓ Commands test with env and secrets");
                assert_eq!(cmd.script.interpreter, Some("python".to_string()));
                assert_eq!(cmd.script.env.len(), 2);
                assert_eq!(cmd.script.env.get("FOO"), Some(&"BAR".to_string()));
                assert_eq!(cmd.script.env.get("BAZ"), Some(&"QUX".to_string()));
                assert_eq!(cmd.script.secrets.len(), 2);
                assert!(cmd.script.secrets.contains(&"ABC".to_string()));
                assert!(cmd.script.secrets.contains(&"DEF".to_string()));
            }
            _ => panic!("Expected Commands test at index 5"),
        }

        // Test 6: Downstream test - THE KEY FIX!
        println!("\nTest 6:");
        match &tests[6] {
            TestType::Downstream(d) => {
                println!("  ✓ Downstream test: {}", d.downstream);
                assert_eq!(d.downstream, "foo");
            }
            _ => panic!("Expected Downstream test at index 6, got: {:?}", tests[6]),
        }

        // Test 7: Perl test
        println!("\nTest 7:");
        match &tests[7] {
            TestType::Perl { perl } => {
                println!("  ✓ Perl test");
                assert_eq!(perl.uses, vec!["Test::More"]);
            }
            _ => panic!("Expected Perl test at index 7"),
        }

        // Test 8: R test
        println!("\nTest 8:");
        match &tests[8] {
            TestType::R { r } => {
                println!("  ✓ R test");
                assert_eq!(r.libraries, vec!["testthat"]);
            }
            _ => panic!("Expected R test at index 8"),
        }

        // Test 9: Ruby test
        println!("\nTest 9:");
        match &tests[9] {
            TestType::Ruby { ruby } => {
                println!("  ✓ Ruby test");
                assert_eq!(ruby.requires, vec!["minitest"]);
            }
            _ => panic!("Expected Ruby test at index 9"),
        }

        println!("\n=== All 10 tests parsed successfully! ===");
    }

    #[test]
    fn test_serialization_matches_old_format() {
        // Parse the old format
        let original_yaml = r#"
- python:
    imports:
    - numpy.testing
- downstream: foo
- script:
    content: echo "FOO"
"#;

        let tests: Vec<TestType> = serde_yaml::from_str(original_yaml).unwrap();

        // Serialize it back
        let serialized = serde_yaml::to_string(&tests).unwrap();
        println!("\n=== Original YAML ===\n{}", original_yaml);
        println!("\n=== Re-serialized YAML ===\n{}", serialized);

        // Parse the serialized version
        let reparsed: Vec<TestType> = serde_yaml::from_str(&serialized).unwrap();

        // Verify structure is preserved
        assert_eq!(tests.len(), reparsed.len());

        // Verify each type matches
        for (i, (original, reparsed)) in tests.iter().zip(reparsed.iter()).enumerate() {
            match (original, reparsed) {
                (TestType::Python { .. }, TestType::Python { .. }) => {
                    println!("Test {}: ✓ Python", i);
                }
                (TestType::Downstream(d1), TestType::Downstream(d2)) => {
                    println!("Test {}: ✓ Downstream", i);
                    assert_eq!(d1.downstream, d2.downstream);
                }
                (TestType::Commands(_), TestType::Commands(_)) => {
                    println!("Test {}: ✓ Commands", i);
                }
                _ => panic!("Type mismatch at index {}", i),
            }
        }

        // Most importantly: verify downstream is still downstream after roundtrip
        match &reparsed[1] {
            TestType::Downstream(d) => {
                assert_eq!(d.downstream, "foo");
                println!("\n✓ Downstream test survives roundtrip correctly!");
            }
            _ => panic!("Downstream test was corrupted during roundtrip"),
        }
    }

    #[test]
    fn test_downstream_with_different_script_orders() {
        // Test that downstream works regardless of where it appears relative to scripts
        let yaml1 = r#"
- downstream: pkg1
- script:
    content: echo "test"
"#;

        let yaml2 = r#"
- script:
    content: echo "test"
- downstream: pkg2
"#;

        let tests1: Vec<TestType> = serde_yaml::from_str(yaml1).unwrap();
        let tests2: Vec<TestType> = serde_yaml::from_str(yaml2).unwrap();

        // Verify yaml1: downstream then script
        match &tests1[0] {
            TestType::Downstream(d) => assert_eq!(d.downstream, "pkg1"),
            _ => panic!("Expected Downstream at index 0"),
        }
        assert!(matches!(tests1[1], TestType::Commands(_)));

        // Verify yaml2: script then downstream
        assert!(matches!(tests2[0], TestType::Commands(_)));
        match &tests2[1] {
            TestType::Downstream(d) => assert_eq!(d.downstream, "pkg2"),
            _ => panic!("Expected Downstream at index 1"),
        }

        println!("✓ Downstream works in any position relative to scripts!");
    }
}
