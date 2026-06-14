//! Desugaring of `subpackages` into the existing multi-output staging machinery.
//!
//! A subpackage splits files off of its owning output. Rather than building a
//! whole new pipeline, we desugar an output that has `subpackages` into an
//! equivalent multi-output recipe:
//!
//! - a **staging cache** that runs the build once (carrying the build script and
//!   the build/host requirements),
//! - the **parent** package output, inheriting from that cache, packaging the
//!   *remainder* of the files (everything not claimed by a subpackage),
//! - one **package output per subpackage**, inheriting from the same cache and
//!   selecting its files via globs.
//!
//! This reuses the staging/cache evaluation, `pin_subpackage` resolution,
//! topological sort, dependency resolution and packaging that already exist for
//! hand-written multi-output recipes.
//!
//! The "remainder" for the parent is expressed as an exclude glob set: the union
//! of every subpackage's *include* patterns. This is exact for include-only
//! subpackage `files` (the common `-dev`/`-doc`/`-static` case). A subpackage
//! that uses an internal `exclude` is an advanced case where the excluded files
//! are NOT returned to the parent; see `design/subpackages.md`.
//!
//! See `design/subpackages.md` for the full design.

use crate::stage0::{
    About, Build, ConditionalList, IncludeExclude, Inherit, Item, MultiOutputRecipe, Output,
    Package, PackageMetadata, PackageOutput, Recipe, RecipeMetadata, Requirements,
    SingleOutputRecipe, Script, StagingBuild, StagingMetadata, StagingOutput, Value,
};

/// Returns true if any output in the recipe declares `subpackages`.
pub fn recipe_has_subpackages(recipe: &Recipe) -> bool {
    match recipe {
        Recipe::SingleOutput(single) => !single.subpackages.is_empty(),
        Recipe::MultiOutput(multi) => multi.outputs.iter().any(
            |output| matches!(output, Output::Package(p) if !p.subpackages.is_empty()),
        ),
    }
}

/// Desugar all `subpackages` in the recipe into staging-based multi-output
/// outputs. Recipes without subpackages are returned unchanged.
pub fn desugar_subpackages(recipe: Recipe) -> Recipe {
    if !recipe_has_subpackages(&recipe) {
        return recipe;
    }

    match recipe {
        Recipe::SingleOutput(single) => desugar_single_output(*single),
        Recipe::MultiOutput(multi) => {
            let MultiOutputRecipe {
                schema_version,
                context,
                recipe,
                source,
                build,
                about,
                extra,
                tests,
                outputs,
            } = *multi;

            let mut new_outputs = Vec::with_capacity(outputs.len());
            let mut cache_idx = 0usize;
            for output in outputs {
                match output {
                    Output::Package(pkg) if !pkg.subpackages.is_empty() => {
                        new_outputs.extend(expand_output(*pkg, cache_idx));
                        cache_idx += 1;
                    }
                    other => new_outputs.push(other),
                }
            }

            Recipe::MultiOutput(Box::new(MultiOutputRecipe {
                schema_version,
                context,
                recipe,
                source,
                build,
                about,
                extra,
                tests,
                outputs: new_outputs,
            }))
        }
    }
}

/// Convert a single-output recipe with subpackages into an equivalent
/// multi-output recipe.
fn desugar_single_output(single: SingleOutputRecipe) -> Recipe {
    let SingleOutputRecipe {
        schema_version,
        context,
        package,
        build,
        requirements,
        about,
        extra,
        source,
        tests,
        subpackages,
    } = single;

    let Package { name, version } = package;

    // Build a package output that stands in for the single output, then expand
    // it through the same path used for multi-output recipes.
    let parent = PackageOutput {
        package: PackageMetadata {
            name: name.clone(),
            version: Some(version.clone()),
        },
        inherit: Inherit::TopLevel,
        source,
        requirements,
        build,
        about,
        tests,
        subpackages,
    };

    let outputs = expand_output(parent, 0);

    Recipe::MultiOutput(Box::new(MultiOutputRecipe {
        schema_version,
        context,
        recipe: RecipeMetadata {
            name: Some(name),
            version: Some(version),
        },
        source: ConditionalList::default(),
        build: Build::default(),
        about: About::default(),
        extra,
        tests: ConditionalList::default(),
        outputs,
    }))
}

/// Expand a single package output that declares `subpackages` into a staging
/// cache (if needed) plus one package output for the parent (remainder) and one
/// per subpackage.
fn expand_output(parent: PackageOutput, cache_idx: usize) -> Vec<Output> {
    let PackageOutput {
        package,
        inherit,
        source,
        requirements,
        build,
        about,
        tests,
        subpackages,
    } = parent;

    // Determine where the parent + subpackages should inherit from. If the
    // output already inherits from a staging cache we reuse it; otherwise we
    // create a fresh staging cache that carries the build script and
    // build/host requirements.
    let (inherit_for_children, staging) = match inherit {
        Inherit::TopLevel => {
            let cache_name = format!("_rb_subpackage_cache_{cache_idx}");
            let cache_value = Value::new_concrete(cache_name, None);
            let staging = StagingOutput {
                staging: StagingMetadata {
                    name: cache_value.clone(),
                },
                source,
                requirements: Requirements {
                    build: requirements.build.clone(),
                    host: requirements.host.clone(),
                    ignore_run_exports: requirements.ignore_run_exports.clone(),
                    ..Requirements::default()
                },
                build: StagingBuild {
                    script: build.script.clone(),
                },
            };
            (Inherit::CacheName(cache_value), Some(staging))
        }
        other => (other, None),
    };

    // The run-side requirements that belong on the parent package (build/host
    // live on the staging cache).
    let run_requirements = Requirements {
        run: requirements.run.clone(),
        run_constraints: requirements.run_constraints.clone(),
        run_exports: requirements.run_exports.clone(),
        ignore_run_exports: requirements.ignore_run_exports.clone(),
        extras: requirements.extras.clone(),
        ..Requirements::default()
    };

    // Compute the parent "remainder" file selection: keep the parent's own
    // include patterns, and exclude the union of every subpackage's includes.
    let (parent_include, mut parent_exclude) = match &build.files {
        IncludeExclude::List(list) => (list.clone(), Vec::new()),
        IncludeExclude::Mapping { include, exclude } => {
            (include.clone(), exclude.iter().cloned().collect::<Vec<_>>())
        }
    };
    for subpackage in &subpackages {
        parent_exclude.extend(include_items(&subpackage.files));
    }

    // Parent package output (the remainder).
    let parent_version = package.version.clone();
    let mut parent_build = build.clone();
    parent_build.script = Script::default();
    parent_build.files = IncludeExclude::Mapping {
        include: parent_include,
        exclude: ConditionalList::new(parent_exclude),
    };
    let parent_pkg = PackageOutput {
        package,
        inherit: inherit_for_children.clone(),
        source: ConditionalList::default(),
        requirements: run_requirements,
        build: parent_build,
        about: about.clone(),
        tests,
        subpackages: Vec::new(),
    };

    let mut outputs = Vec::with_capacity(2 + subpackages.len());
    if let Some(staging) = staging {
        outputs.push(Output::Staging(Box::new(staging)));
    }
    outputs.push(Output::Package(Box::new(parent_pkg)));

    // One package output per subpackage.
    for subpackage in subpackages {
        let mut sub_build = build.clone();
        sub_build.script = Script::default();
        sub_build.files = subpackage.files;

        let version = subpackage.package.version.or_else(|| parent_version.clone());
        let sub_about = merge_about(about.clone(), subpackage.about);

        let sub_pkg = PackageOutput {
            package: PackageMetadata {
                name: subpackage.package.name,
                version,
            },
            inherit: inherit_for_children.clone(),
            source: ConditionalList::default(),
            requirements: subpackage.requirements,
            build: sub_build,
            about: sub_about,
            tests: subpackage.tests,
            subpackages: Vec::new(),
        };
        outputs.push(Output::Package(Box::new(sub_pkg)));
    }

    outputs
}

/// The include patterns of an `IncludeExclude` (a plain list is all-include).
fn include_items(files: &IncludeExclude) -> Vec<Item<String>> {
    match files {
        IncludeExclude::List(list) => list.iter().cloned().collect(),
        IncludeExclude::Mapping { include, .. } => include.iter().cloned().collect(),
    }
}

/// Merge two about sections, with `overlay`'s set fields taking precedence over
/// `base`.
fn merge_about(mut base: About, overlay: About) -> About {
    let About {
        homepage,
        license,
        license_file,
        license_family,
        summary,
        description,
        documentation,
        repository,
    } = overlay;

    if homepage.is_some() {
        base.homepage = homepage;
    }
    if license.is_some() {
        base.license = license;
    }
    if license_file.is_some() {
        base.license_file = license_file;
    }
    if license_family.is_some() {
        base.license_family = license_family;
    }
    if summary.is_some() {
        base.summary = summary;
    }
    if description.is_some() {
        base.description = description;
    }
    if documentation.is_some() {
        base.documentation = documentation;
    }
    if repository.is_some() {
        base.repository = repository;
    }
    base
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage0::parser::{parse_recipe_from_source, parse_recipe_or_multi_from_source};

    fn package_names(outputs: &[Output]) -> Vec<String> {
        outputs
            .iter()
            .filter_map(|o| match o {
                Output::Package(p) => {
                    p.package.name.as_concrete().map(|n| n.to_string())
                }
                Output::Staging(_) => None,
            })
            .collect()
    }

    fn exclude_globs(files: &IncludeExclude) -> Vec<String> {
        match files {
            IncludeExclude::Mapping { exclude, .. } => exclude
                .iter()
                .filter_map(|i| i.as_value().and_then(|v| v.as_concrete()).cloned())
                .collect(),
            IncludeExclude::List(_) => Vec::new(),
        }
    }

    fn include_globs(files: &IncludeExclude) -> Vec<String> {
        match files {
            IncludeExclude::List(list) => list
                .iter()
                .filter_map(|i| i.as_value().and_then(|v| v.as_concrete()).cloned())
                .collect(),
            IncludeExclude::Mapping { include, .. } => include
                .iter()
                .filter_map(|i| i.as_value().and_then(|v| v.as_concrete()).cloned())
                .collect(),
        }
    }

    const SINGLE_RECIPE: &str = r#"
package:
  name: mylib
  version: 1.2.3

build:
  script: make install

requirements:
  build:
    - cmake
  run:
    - libstdcxx

subpackages:
  - package:
      name: mylib-dev
    files:
      - include/**
      - lib/**/*.so
    requirements:
      run:
        - ${{ pin_subpackage('mylib', exact=true) }}
    about:
      summary: Dev files
  - package:
      name: mylib-doc
    files:
      - share/man/**
"#;

    #[test]
    fn test_recipe_has_subpackages() {
        let with = parse_recipe_or_multi_from_source(SINGLE_RECIPE).unwrap();
        assert!(recipe_has_subpackages(&with));

        let without = parse_recipe_or_multi_from_source(
            "package:\n  name: x\n  version: 1.0.0\n",
        )
        .unwrap();
        assert!(!recipe_has_subpackages(&without));
    }

    #[test]
    fn test_single_output_desugars_to_staging_multi_output() {
        let recipe = parse_recipe_or_multi_from_source(SINGLE_RECIPE).unwrap();
        let desugared = desugar_subpackages(recipe);

        let Recipe::MultiOutput(multi) = desugared else {
            panic!("expected multi-output after desugaring");
        };

        // recipe-level name/version carried over
        assert_eq!(
            multi.recipe.name.as_ref().unwrap().as_concrete().unwrap().to_string(),
            "mylib"
        );

        // staging + parent + 2 subpackages
        assert_eq!(multi.outputs.len(), 4);
        let staging_count = multi
            .outputs
            .iter()
            .filter(|o| matches!(o, Output::Staging(_)))
            .count();
        assert_eq!(staging_count, 1);
        assert_eq!(package_names(&multi.outputs), vec!["mylib", "mylib-dev", "mylib-doc"]);

        // staging carries the build script and build requirements
        let Output::Staging(staging) = &multi.outputs[0] else {
            panic!("first output should be the staging cache");
        };
        assert!(!staging.requirements.build.is_empty());
        assert!(staging.requirements.run.is_empty());

        // parent (remainder) excludes the union of subpackage includes
        let Output::Package(parent) = &multi.outputs[1] else {
            panic!("second output should be the parent package");
        };
        assert!(parent.requirements.build.is_empty(), "build reqs moved to staging");
        assert!(!parent.requirements.run.is_empty(), "run reqs kept on parent");
        let parent_excludes = exclude_globs(&parent.build.files);
        assert!(parent_excludes.contains(&"include/**".to_string()));
        assert!(parent_excludes.contains(&"lib/**/*.so".to_string()));
        assert!(parent_excludes.contains(&"share/man/**".to_string()));

        // parent + subpackages all inherit from the same staging cache
        for output in multi.outputs.iter().skip(1) {
            let Output::Package(pkg) = output else { unreachable!() };
            assert!(
                matches!(&pkg.inherit, Inherit::CacheName(_)),
                "expected cache inheritance for {:?}",
                pkg.package.name
            );
            assert!(pkg.subpackages.is_empty());
        }
    }

    #[test]
    fn test_subpackage_files_and_version_inheritance() {
        let recipe = parse_recipe_or_multi_from_source(SINGLE_RECIPE).unwrap();
        let Recipe::MultiOutput(multi) = desugar_subpackages(recipe) else {
            panic!("expected multi-output");
        };

        let Output::Package(dev) = &multi.outputs[2] else {
            panic!("expected mylib-dev package output");
        };
        assert_eq!(dev.package.name.as_concrete().unwrap().to_string(), "mylib-dev");
        // version inherited from the parent
        assert_eq!(
            dev.package.version.as_ref().unwrap().as_concrete().unwrap().to_string(),
            "1.2.3"
        );
        let dev_includes = include_globs(&dev.build.files);
        assert_eq!(dev_includes, vec!["include/**", "lib/**/*.so"]);
        // subpackage about overrides summary, build script is cleared
        assert_eq!(
            dev.about.summary.as_ref().unwrap().as_concrete().unwrap(),
            "Dev files"
        );
    }

    #[test]
    fn test_multi_output_expands_only_outputs_with_subpackages() {
        let recipe_yaml = r#"
recipe:
  name: proj
  version: 1.0.0

outputs:
  - package:
      name: plain
    requirements:
      run:
        - python
  - package:
      name: withsub
    build:
      script: make install
    subpackages:
      - package:
          name: withsub-dev
        files:
          - include/**
"#;
        let recipe = parse_recipe_or_multi_from_source(recipe_yaml).unwrap();
        let Recipe::MultiOutput(multi) = desugar_subpackages(recipe) else {
            panic!("expected multi-output");
        };

        // plain (untouched) + staging + withsub + withsub-dev
        assert_eq!(multi.outputs.len(), 4);
        assert_eq!(
            package_names(&multi.outputs),
            vec!["plain", "withsub", "withsub-dev"]
        );
    }

    #[test]
    fn test_no_subpackages_is_unchanged() {
        let recipe =
            parse_recipe_from_source("package:\n  name: x\n  version: 1.0.0\n").unwrap();
        let recipe = Recipe::SingleOutput(Box::new(recipe));
        let desugared = desugar_subpackages(recipe.clone());
        assert_eq!(recipe, desugared);
    }
}
