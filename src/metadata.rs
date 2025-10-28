//! All the metadata that makes up a recipe file
pub use crate::types::{
    BuildConfiguration, Debug, Output, PlatformWithVirtualPackages, build_reindexed_channels,
};

#[cfg(test)]
mod test {
    use chrono::TimeZone;
    use fs_err as fs;
    use insta::assert_yaml_snapshot;
    use rattler_conda_types::{
        MatchSpec, NoArchType, PackageName, PackageRecord, ParseStrictness, RepoDataRecord,
        VersionWithSource,
    };
    use rattler_digest::{Md5, Sha256, parse_digest_from_hex};
    use rstest::*;
    use std::str::FromStr;
    use url::Url;

    use super::Output;
    use crate::render::resolved_dependencies::{self, SourceDependency};

    #[test]
    fn test_resolved_dependencies_rendering() {
        let resolved_dependencies = resolved_dependencies::ResolvedDependencies {
            specs: vec![
                SourceDependency {
                    spec: MatchSpec::from_str("python 3.12.* h12332", ParseStrictness::Strict)
                        .unwrap(),
                }
                .into(),
            ],
            resolved: vec![RepoDataRecord {
                package_record: PackageRecord {
                    arch: Some("x86_64".into()),
                    build: "h123".into(),
                    build_number: 0,
                    constrains: vec![],
                    depends: vec![],
                    features: None,
                    legacy_bz2_md5: None,
                    legacy_bz2_size: None,
                    license: Some("MIT".into()),
                    license_family: None,
                    md5: parse_digest_from_hex::<Md5>("68b329da9893e34099c7d8ad5cb9c940"),
                    name: PackageName::from_str("test").unwrap(),
                    noarch: NoArchType::none(),
                    platform: Some("linux".into()),
                    sha256: parse_digest_from_hex::<Sha256>(
                        "01ba4719c80b6fe911b091a7c05124b64eeece964e09c058ef8f9805daca546b",
                    ),
                    size: Some(123123),
                    subdir: "linux-64".into(),
                    timestamp: Some(chrono::Utc.timestamp_opt(123123, 0).unwrap().into()),
                    track_features: vec![],
                    version: VersionWithSource::from_str("1.2.3").unwrap(),
                    purls: None,
                    run_exports: None,
                    python_site_packages_path: None,
                    experimental_extra_depends: Default::default(),
                },
                file_name: "test-1.2.3-h123.tar.bz2".into(),
                url: Url::from_str("https://test.com/test/linux-64/test-1.2.3-h123.tar.bz2")
                    .unwrap(),
                channel: Some("test".into()),
            }],
        };

        // test yaml roundtrip
        assert_yaml_snapshot!(resolved_dependencies);
        let yaml = serde_yaml::to_string(&resolved_dependencies).unwrap();
        let resolved_dependencies2: resolved_dependencies::ResolvedDependencies =
            serde_yaml::from_str(&yaml).unwrap();
        let yaml2 = serde_yaml::to_string(&resolved_dependencies2).unwrap();
        assert_eq!(yaml, yaml2);

        let test_data_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/rendered_recipes");
        let yaml3 = fs::read_to_string(test_data_dir.join("dependencies.yaml")).unwrap();
        let parsed_yaml3: resolved_dependencies::ResolvedDependencies =
            serde_yaml::from_str(&yaml3).unwrap();

        assert_eq!("pip", parsed_yaml3.specs[0].render(false));
    }

    #[rstest]
    #[case::rich("rich_recipe.yaml")]
    #[case::curl("curl_recipe.yaml")]
    fn read_full_recipe(#[case] recipe_path: String) {
        let test_data_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/rendered_recipes");

        let recipe = fs::read_to_string(test_data_dir.join(&recipe_path)).unwrap();
        let output: Output = serde_yaml::from_str(&recipe).unwrap();
        assert_yaml_snapshot!(recipe_path, output);
    }

    #[test]
    fn read_recipe_with_sources() {
        let test_data_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/rendered_recipes");
        let recipe_1 = test_data_dir.join("git_source.yaml");
        let recipe_1 = fs::read_to_string(recipe_1).unwrap();

        let git_source_output: Output = serde_yaml::from_str(&recipe_1).unwrap();
        assert_yaml_snapshot!(git_source_output);
    }
}
