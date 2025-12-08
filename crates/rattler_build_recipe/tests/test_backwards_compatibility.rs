/// Test backwards compatibility by parsing a comprehensive tests.yaml file
/// This ensures we can still parse test files from existing packages
#[cfg(test)]
mod test {
    use rattler_build_recipe::stage1::tests::TestType;

    #[test]
    fn test_parse_comprehensive_tests_yaml() {
        // This represents a typical tests.yaml that would be extracted from a package
        // It includes all test types in the order they might appear in real recipes
        let tests_yaml = r#"
- script:
    content: echo "Running command test"
  requirements:
    run:
      - pytest
- downstream: some-downstream-package
- python:
    imports:
      - numpy
      - pandas
- perl:
    uses:
      - strict
      - warnings
- r:
    libraries:
      - ggplot2
- ruby:
    requires:
      - json
- package_contents:
    files:
      exists:
        - "*.txt"
      not_exists:
        - "*.pyc"
"#;

        println!("\n=== Parsing comprehensive tests.yaml ===");
        let tests: Vec<TestType> =
            serde_yaml::from_str(tests_yaml).expect("Failed to parse tests.yaml");

        println!("Found {} tests", tests.len());
        assert_eq!(tests.len(), 7, "Expected 7 tests");

        // Verify each test type
        println!("\nTest 0: {:?}", tests[0]);
        match &tests[0] {
            TestType::Commands(cmd) => {
                println!(
                    "  ✓ Commands test with {} run requirements",
                    cmd.requirements.run.len()
                );
                assert_eq!(cmd.requirements.run.len(), 1);
            }
            _ => panic!("Expected Commands test at index 0"),
        }

        println!("\nTest 1: {:?}", tests[1]);
        match &tests[1] {
            TestType::Downstream(d) => {
                println!("  ✓ Downstream test: {}", d.downstream);
                assert_eq!(d.downstream, "some-downstream-package");
            }
            _ => panic!("Expected Downstream test at index 1"),
        }

        println!("\nTest 2: {:?}", tests[2]);
        match &tests[2] {
            TestType::Python { python } => {
                println!("  ✓ Python test with {} imports", python.imports.len());
                assert_eq!(python.imports.len(), 2);
            }
            _ => panic!("Expected Python test at index 2"),
        }

        println!("\nTest 3: {:?}", tests[3]);
        match &tests[3] {
            TestType::Perl { perl } => {
                println!("  ✓ Perl test with {} uses", perl.uses.len());
                assert_eq!(perl.uses.len(), 2);
            }
            _ => panic!("Expected Perl test at index 3"),
        }

        println!("\nTest 4: {:?}", tests[4]);
        match &tests[4] {
            TestType::R { r } => {
                println!("  ✓ R test with {} libraries", r.libraries.len());
                assert_eq!(r.libraries.len(), 1);
            }
            _ => panic!("Expected R test at index 4"),
        }

        println!("\nTest 5: {:?}", tests[5]);
        match &tests[5] {
            TestType::Ruby { ruby } => {
                println!("  ✓ Ruby test with {} requires", ruby.requires.len());
                assert_eq!(ruby.requires.len(), 1);
            }
            _ => panic!("Expected Ruby test at index 5"),
        }

        println!("\nTest 6: {:?}", tests[6]);
        match &tests[6] {
            TestType::PackageContents { package_contents } => {
                println!("  ✓ PackageContents test");
                assert_eq!(package_contents.files.exists.include_globs().len(), 1);
                assert_eq!(package_contents.files.not_exists.include_globs().len(), 1);
            }
            _ => panic!("Expected PackageContents test at index 6"),
        }

        println!("\n=== All tests parsed successfully! ===");
    }

    #[test]
    fn test_roundtrip_preserves_structure() {
        // Parse the original YAML
        let original_yaml = r#"
- script:
    content: echo "test"
  requirements:
    run:
      - pytest
- downstream: my-package
- python:
    imports:
      - numpy
"#;

        let tests: Vec<TestType> = serde_yaml::from_str(original_yaml).unwrap();

        // Serialize back to YAML
        let serialized = serde_yaml::to_string(&tests).unwrap();
        println!("\n=== Original YAML ===\n{}", original_yaml);
        println!("\n=== Serialized YAML ===\n{}", serialized);

        // Parse again to verify structure is preserved
        let reparsed: Vec<TestType> = serde_yaml::from_str(&serialized).unwrap();
        assert_eq!(tests.len(), reparsed.len());

        // Verify types match
        assert!(matches!(tests[0], TestType::Commands(_)));
        assert!(matches!(tests[1], TestType::Downstream(_)));
        assert!(matches!(tests[2], TestType::Python { .. }));

        assert!(matches!(reparsed[0], TestType::Commands(_)));
        assert!(matches!(reparsed[1], TestType::Downstream(_)));
        assert!(matches!(reparsed[2], TestType::Python { .. }));

        println!("\n=== Roundtrip successful! ===");
    }
}
