#![deny(missing_docs)]

//! rattler-build library.

pub mod build;
pub mod console_utils;
pub mod metadata;
pub mod opt;
pub mod package_test;
pub mod packaging;
pub mod recipe;
pub mod render;
pub mod script;
pub mod selectors;
pub mod source;
pub mod system_tools;
pub mod tool_configuration;
#[cfg(feature = "tui")]
pub mod tui;
pub mod used_variables;
pub mod utils;
pub mod variant_config;

mod env_vars;
pub mod hash;
mod linux;
mod macos;
mod post_process;
pub mod rebuild;
pub mod recipe_generator;
mod unix;
pub mod upload;
mod windows;

use dunce::canonicalize;
use fs_err as fs;
use metadata::Output;
use miette::IntoDiagnostic;
use petgraph::{algo::toposort, graph::DiGraph, visit::DfsPostOrder};
use rattler_conda_types::{package::ArchiveType, Platform};
use std::{
    collections::{BTreeMap, HashMap},
    env::current_dir,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tool_configuration::Configuration;

use {
    build::run_build,
    console_utils::LoggingOutputHandler,
    hash::HashInfo,
    metadata::{
        BuildConfiguration, BuildSummary, Directories, PackageIdentifier, PackagingSettings,
    },
    opt::*,
    package_test::TestConfiguration,
    recipe::{
        parser::{find_outputs_from_src, Recipe},
        ParsingError,
    },
    selectors::SelectorConfig,
    system_tools::SystemTools,
    variant_config::{ParseErrors, VariantConfig},
};

/// Returns the recipe path.
pub fn get_recipe_path(path: &Path) -> miette::Result<PathBuf> {
    let recipe_path = canonicalize(path);
    if let Err(e) = &recipe_path {
        match e.kind() {
            std::io::ErrorKind::NotFound => {
                return Err(miette::miette!(
                    "The file {} could not be found.",
                    path.to_string_lossy()
                ));
            }
            std::io::ErrorKind::PermissionDenied => {
                return Err(miette::miette!(
                    "Permission denied when trying to access the file {}.",
                    path.to_string_lossy()
                ));
            }
            _ => {
                return Err(miette::miette!(
                    "An unknown error occurred while trying to access the file {}: {:?}",
                    path.to_string_lossy(),
                    e
                ));
            }
        }
    }
    let mut recipe_path = recipe_path.into_diagnostic()?;

    // If the recipe_path is a directory, look for "recipe.yaml" in the directory.
    if recipe_path.is_dir() {
        let recipe_yaml_path = recipe_path.join("recipe.yaml");
        if recipe_yaml_path.exists() && recipe_yaml_path.is_file() {
            recipe_path = recipe_yaml_path;
        } else {
            return Err(miette::miette!(
                "'recipe.yaml' not found in the directory {}",
                path.to_string_lossy()
            ));
        }
    }

    Ok(recipe_path)
}

/// Returns the tool configuration.
pub fn get_tool_config(
    args: &BuildOpts,
    fancy_log_handler: &LoggingOutputHandler,
) -> Configuration {
    let client =
        tool_configuration::reqwest_client_from_auth_storage(args.common.auth_file.clone());
    Configuration {
        client,
        fancy_log_handler: fancy_log_handler.clone(),
        no_clean: args.keep_build,
        no_test: args.no_test,
        use_zstd: args.common.use_zstd,
        use_bz2: args.common.use_bz2,
        render_only: args.render_only,
    }
}

/// Returns the output for the build.
pub async fn get_build_output(
    args: &BuildOpts,
    recipe_path: &Path,
    tool_config: &Configuration,
) -> miette::Result<Vec<Output>> {
    let output_dir = args
        .common
        .output_dir
        .clone()
        .unwrap_or(current_dir().into_diagnostic()?.join("output"));
    if output_dir.starts_with(
        recipe_path
            .parent()
            .expect("Could not get parent of recipe"),
    ) {
        return Err(miette::miette!(
            "The output directory cannot be in the recipe directory.\nThe current output directory is: {}\nSelect a different output directory with the --output-dir option or set the CONDA_BLD_PATH environment variable"
        , output_dir.to_string_lossy()));
    }

    let recipe_text = fs::read_to_string(recipe_path).into_diagnostic()?;

    if args.target_platform == Platform::NoArch || args.build_platform == Platform::NoArch {
        return Err(miette::miette!(
            "target-platform / build-platform cannot be `noarch` - that should be defined in the recipe"
        ));
    }

    let selector_config = SelectorConfig {
        // We ignore noarch here
        target_platform: args.target_platform,
        hash: None,
        build_platform: args.build_platform,
        variant: BTreeMap::new(),
        experimental: args.common.experimental,
    };

    let span = tracing::info_span!("Finding outputs from recipe");

    let enter = span.enter();
    // First find all outputs from the recipe
    let outputs = find_outputs_from_src(&recipe_text)?;

    let variant_config =
        VariantConfig::from_files(&args.variant_config, &selector_config).into_diagnostic()?;

    let outputs_and_variants =
        variant_config.find_variants(&outputs, &recipe_text, &selector_config)?;

    tracing::info!("Found {} variants\n", outputs_and_variants.len());
    for discovered_output in &outputs_and_variants {
        tracing::info!(
            "Build variant: {}-{}-{}",
            discovered_output.name,
            discovered_output.version,
            discovered_output.build_string
        );

        let mut table = comfy_table::Table::new();
        table
            .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
            .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
            .set_header(vec!["Variant", "Version"]);
        for (key, value) in discovered_output.used_vars.iter() {
            table.add_row(vec![key, value]);
        }
        tracing::info!("\n{}\n", table);
    }
    drop(enter);

    let mut subpackages = BTreeMap::new();
    let mut outputs = Vec::new();
    for discovered_output in outputs_and_variants {
        let hash =
            HashInfo::from_variant(&discovered_output.used_vars, &discovered_output.noarch_type);

        let selector_config = SelectorConfig {
            variant: discovered_output.used_vars.clone(),
            hash: Some(hash.clone()),
            target_platform: selector_config.target_platform,
            build_platform: selector_config.build_platform,
            experimental: args.common.experimental,
        };

        let recipe =
            Recipe::from_node(&discovered_output.node, selector_config).map_err(|err| {
                let errs: ParseErrors = err
                    .into_iter()
                    .map(|err| ParsingError::from_partial(&recipe_text, err))
                    .collect::<Vec<ParsingError>>()
                    .into();
                errs
            })?;

        if recipe.build().skip() {
            tracing::info!(
                "Skipping build for variant: {:#?}",
                discovered_output.used_vars
            );
            continue;
        }

        subpackages.insert(
            recipe.package().name().clone(),
            PackageIdentifier {
                name: recipe.package().name().clone(),
                version: recipe.package().version().to_owned(),
                build_string: recipe
                    .build()
                    .string()
                    .expect("Shouldn't be unset, needs major refactoring, for handling this better")
                    .to_owned(),
            },
        );

        let name = recipe.package().name().clone();
        // Add the channels from the args and by default always conda-forge
        let channels = args
            .channel
            .clone()
            .unwrap_or_else(|| vec!["conda-forge".to_string()]);

        let timestamp = chrono::Utc::now();

        let output = metadata::Output {
            recipe,
            build_configuration: BuildConfiguration {
                target_platform: discovered_output.target_platform,
                host_platform: args.target_platform,
                build_platform: args.build_platform,
                hash,
                variant: discovered_output.used_vars.clone(),
                directories: Directories::setup(
                    name.as_normalized(),
                    recipe_path,
                    &output_dir,
                    args.no_build_id,
                    &timestamp,
                )
                .into_diagnostic()?,
                channels,
                timestamp,
                subpackages: subpackages.clone(),
                packaging_settings: PackagingSettings::from_args(
                    args.package_format.archive_type,
                    args.package_format.compression_level,
                    args.compression_threads,
                ),
                store_recipe: !args.no_include_recipe,
                force_colors: args.color_build_log && console::colors_enabled(),
            },
            finalized_dependencies: None,
            finalized_sources: None,
            system_tools: SystemTools::new(),
            build_summary: Arc::new(Mutex::new(BuildSummary::default())),
        };

        if args.render_only && args.with_solve {
            let output_with_resolved_dependencies = output
                .resolve_dependencies(tool_config)
                .await
                .into_diagnostic()?;
            outputs.push(output_with_resolved_dependencies);
            continue;
        }
        outputs.push(output);
    }

    Ok(outputs)
}

/// Runs build.
pub async fn run_build_from_args(
    build_output: Vec<Output>,
    tool_config: Configuration,
) -> miette::Result<()> {
    let mut outputs: Vec<metadata::Output> = Vec::new();
    for output in build_output {
        let output = match run_build(output, &tool_config).await {
            Ok((output, _archive)) => {
                output.record_build_end();
                output
            }
            Err(e) => {
                tracing::error!("Error building package: {}", e);
                return Err(e);
            }
        };
        outputs.push(output);
    }

    let span = tracing::info_span!("Build summary");
    let _enter = span.enter();
    for output in outputs {
        // print summaries for each output
        let _ = output.log_build_summary().map_err(|e| {
            tracing::error!("Error writing build summary: {}", e);
            e
        });
    }

    Ok(())
}

/// Runs test.
pub async fn run_test_from_args(
    args: TestOpts,
    fancy_log_handler: LoggingOutputHandler,
) -> miette::Result<()> {
    let package_file = canonicalize(args.package_file).into_diagnostic()?;
    let client = tool_configuration::reqwest_client_from_auth_storage(args.common.auth_file);

    let tempdir = tempfile::tempdir().into_diagnostic()?;

    let test_options = TestConfiguration {
        test_prefix: tempdir.path().to_path_buf(),
        target_platform: None,
        keep_test_prefix: false,
        channels: args
            .channel
            .unwrap_or_else(|| vec!["conda-forge".to_string()]),
        tool_configuration: Configuration {
            client,
            fancy_log_handler,
            // duplicate from `keep_test_prefix`?
            no_clean: false,
            ..Default::default()
        },
    };

    let package_name = package_file
        .file_name()
        .ok_or_else(|| miette::miette!("Could not get file name from package file"))?
        .to_string_lossy()
        .to_string();

    let span = tracing::info_span!("Running tests for ", recipe = %package_name);
    let _enter = span.enter();
    package_test::run_test(&package_file, &test_options)
        .await
        .into_diagnostic()?;

    Ok(())
}

/// Rebuild.
pub async fn rebuild_from_args(
    args: RebuildOpts,
    fancy_log_handler: LoggingOutputHandler,
) -> miette::Result<()> {
    tracing::info!("Rebuilding {}", args.package_file.to_string_lossy());
    // we extract the recipe folder from the package file (info/recipe/*)
    // and then run the rendered recipe with the same arguments as the original build
    let temp_folder = tempfile::tempdir().into_diagnostic()?;

    rebuild::extract_recipe(&args.package_file, temp_folder.path()).into_diagnostic()?;

    let temp_dir = temp_folder.into_path();

    tracing::info!("Extracted recipe to: {:?}", temp_dir);

    let rendered_recipe =
        fs::read_to_string(temp_dir.join("rendered_recipe.yaml")).into_diagnostic()?;

    let mut output: metadata::Output = serde_yaml::from_str(&rendered_recipe).into_diagnostic()?;

    // set recipe dir to the temp folder
    output.build_configuration.directories.recipe_dir = temp_dir;

    // create output dir and set it in the config
    let output_dir = args
        .common
        .output_dir
        .unwrap_or(current_dir().into_diagnostic()?.join("output"));

    fs::create_dir_all(&output_dir).into_diagnostic()?;
    output.build_configuration.directories.output_dir =
        canonicalize(output_dir).into_diagnostic()?;

    let client = tool_configuration::reqwest_client_from_auth_storage(args.common.auth_file);

    let tool_config = tool_configuration::Configuration {
        client,
        fancy_log_handler,
        no_clean: true,
        no_test: args.no_test,
        use_zstd: args.common.use_zstd,
        use_bz2: args.common.use_bz2,
        render_only: false,
    };

    output
        .build_configuration
        .directories
        .recreate_directories()
        .into_diagnostic()?;

    run_build(output, &tool_config).await?;

    Ok(())
}

/// Upload.
pub async fn upload_from_args(args: UploadOpts) -> miette::Result<()> {
    if args.package_files.is_empty() {
        return Err(miette::miette!("No package files were provided."));
    }

    for package_file in &args.package_files {
        if ArchiveType::try_from(package_file).is_none() {
            return Err(miette::miette!(
                "The file {} does not appear to be a conda package.",
                package_file.to_string_lossy()
            ));
        }
    }

    let store = tool_configuration::get_auth_store(args.common.auth_file);

    match args.server_type {
        ServerType::Quetz(quetz_opts) => {
            upload::upload_package_to_quetz(
                &store,
                quetz_opts.api_key,
                &args.package_files,
                quetz_opts.url,
                quetz_opts.channel,
            )
            .await?;
        }
        ServerType::Artifactory(artifactory_opts) => {
            upload::upload_package_to_artifactory(
                &store,
                artifactory_opts.username,
                artifactory_opts.password,
                &args.package_files,
                artifactory_opts.url,
                artifactory_opts.channel,
            )
            .await?;
        }
        ServerType::Prefix(prefix_opts) => {
            upload::upload_package_to_prefix(
                &store,
                prefix_opts.api_key,
                &args.package_files,
                prefix_opts.url,
                prefix_opts.channel,
            )
            .await?;
        }
        ServerType::Anaconda(anaconda_opts) => {
            upload::upload_package_to_anaconda(
                &store,
                anaconda_opts.api_key,
                &args.package_files,
                anaconda_opts.url,
                anaconda_opts.owner,
                anaconda_opts.channel,
                anaconda_opts.force,
            )
            .await?;
        }
        ServerType::CondaForge(conda_forge_opts) => {
            upload::conda_forge::upload_packages_to_conda_forge(
                conda_forge_opts,
                &args.package_files,
            )
            .await?;
        }
    }

    Ok(())
}

/// Sort the build outputs (recipes) topologically based on their dependencies.
pub fn sort_build_outputs_topologically(
    outputs: &mut Vec<Output>,
    up_to: Option<&str>,
) -> miette::Result<()> {
    let mut graph = DiGraph::<usize, ()>::new();
    let mut name_to_index = HashMap::new();

    // Index outputs by their produced names for quick lookup
    for (idx, output) in outputs.iter().enumerate() {
        let idx = graph.add_node(idx);
        name_to_index.insert(output.name().clone(), idx);
    }

    // Add edges based on dependencies
    for output in outputs.iter() {
        let output_idx = *name_to_index
            .get(output.name())
            .expect("We just inserted it");
        for dep in output.recipe.requirements().all() {
            let dep_name = match dep {
                recipe::parser::Dependency::Spec(spec) => spec
                    .name
                    .clone()
                    .expect("MatchSpec should always have a name"),
                recipe::parser::Dependency::PinSubpackage(pin) => pin.pin_value().name.clone(),
                recipe::parser::Dependency::PinCompatible(pin) => pin.pin_value().name.clone(),
                recipe::parser::Dependency::Compiler(_) => continue,
            };
            if let Some(&dep_idx) = name_to_index.get(&dep_name) {
                // do not point to self (circular dependency) - this can happen with
                // pin_subpackage in run_exports, for example.
                if output_idx == dep_idx {
                    continue;
                }
                graph.add_edge(output_idx, dep_idx, ());
            }
        }
    }

    let sorted_indices = if let Some(up_to) = up_to {
        // Find the node index for the "up-to" package
        let up_to_index = name_to_index.get(up_to).copied().ok_or_else(|| {
            miette::miette!("The package '{}' was not found in the outputs", up_to)
        })?;

        // Perform a DFS post-order traversal from the "up-to" node to find all dependencies
        let mut dfs = DfsPostOrder::new(&graph, up_to_index);
        let mut sorted_indices = Vec::new();
        while let Some(nx) = dfs.next(&graph) {
            sorted_indices.push(nx);
        }

        sorted_indices
    } else {
        // Perform topological sort
        let mut sorted_indices = toposort(&graph, None).map_err(|cycle| {
            let node = cycle.node_id();
            let name = outputs[node.index()].name();
            miette::miette!("Cycle detected in dependencies: {}", name.as_source())
        })?;
        sorted_indices.reverse();
        sorted_indices
    };

    sorted_indices
        .iter()
        .map(|idx| &outputs[idx.index()])
        .for_each(|output| {
            tracing::debug!("ordered output: {:?}", output.name().as_normalized());
        });

    // Reorder outputs based on the sorted indices
    *outputs = sorted_indices
        .iter()
        .map(|node| outputs[node.index()].clone())
        .collect();

    Ok(())
}
