use fs_err as fs;
use indicatif::HumanBytes;
use rattler_conda_types::{
    PackageName, Platform, RepoDataRecord, VersionWithSource,
    package::{PathType, PathsEntry, PathsJson},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    borrow::Cow,
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
};

use crate::{
    NormalizedKey,
    console_utils::github_integration_enabled,
    recipe::{Recipe, parser::Source, variable::Variable},
    render::resolved_dependencies::FinalizedDependencies,
    system_tools::SystemTools,
    types::{BuildConfiguration, BuildSummary, PlatformWithVirtualPackages},
};

/// A output. This is the central element that is passed to the `run_build`
/// function and fully specifies all the options and settings to run the build.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildOutput {
    /// The rendered recipe that is used to build this output
    pub recipe: Recipe,
    /// The build configuration for this output (e.g. target_platform, channels,
    /// and other settings)
    pub build_configuration: BuildConfiguration,
    /// The finalized dependencies for this output. If this is `None`, the
    /// dependencies have not been resolved yet. During the `run_build`
    /// functions, the dependencies are resolved and this field is filled.
    pub finalized_dependencies: Option<FinalizedDependencies>,
    /// The finalized sources for this output. Contain the exact git hashes for
    /// the sources that are used to build this output.
    pub finalized_sources: Option<Vec<Source>>,

    /// The finalized dependencies from the cache (if there is a cache
    /// instruction)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finalized_cache_dependencies: Option<FinalizedDependencies>,
    /// The finalized sources from the cache (if there is a cache instruction)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finalized_cache_sources: Option<Vec<Source>>,

    /// Summary of the build
    #[serde(skip)]
    pub build_summary: Arc<Mutex<BuildSummary>>,
    /// The system tools that are used to build this output
    pub system_tools: SystemTools,
    /// Some extra metadata that should be recorded additionally in about.json
    /// Usually it is used during the CI build to record link to the CI job
    /// that created this artifact
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_meta: Option<BTreeMap<String, Value>>,
}

impl BuildOutput {
    /// The name of the package
    pub fn name(&self) -> &PackageName {
        self.recipe.package().name()
    }

    /// The version of the package
    pub fn version(&self) -> &VersionWithSource {
        self.recipe.package().version()
    }

    /// The build string is either the build string from the recipe or computed
    /// from the hash and build number.
    pub fn build_string(&self) -> Cow<'_, str> {
        self.recipe
            .build()
            .string
            .as_resolved()
            .expect("Build string is not resolved")
            .into()
    }

    /// retrieve an identifier for this output ({name}-{version}-{build_string})
    pub fn identifier(&self) -> String {
        format!(
            "{}-{}-{}",
            self.name().as_normalized(),
            self.version(),
            &self.build_string()
        )
    }

    /// Record a warning during the build
    pub fn record_warning(&self, warning: &str) {
        self.build_summary
            .lock()
            .unwrap()
            .warnings
            .push(warning.to_string());
    }

    /// Record the start of the build
    pub fn record_build_start(&self) {
        self.build_summary.lock().unwrap().build_start = Some(chrono::Utc::now());
    }

    /// Record the artifact that was created during the build
    pub fn record_artifact(&self, artifact: &Path, paths: &PathsJson) {
        let mut summary = self.build_summary.lock().unwrap();
        summary.artifact = Some(artifact.to_path_buf());
        summary.paths = Some(paths.clone());
    }

    /// Record the end of the build
    pub fn record_build_end(&self) {
        let mut summary = self.build_summary.lock().unwrap();
        summary.build_end = Some(chrono::Utc::now());
    }

    /// Shorthand to retrieve the variant configuration for this output
    pub fn variant(&self) -> &BTreeMap<NormalizedKey, Variable> {
        &self.build_configuration.variant
    }

    /// Shorthand to retrieve the host prefix for this output
    pub fn prefix(&self) -> &Path {
        &self.build_configuration.directories.host_prefix
    }

    /// Shorthand to retrieve the build prefix for this output
    pub fn build_prefix(&self) -> &Path {
        &self.build_configuration.directories.build_prefix
    }

    /// Shorthand to retrieve the target platform for this output
    pub fn target_platform(&self) -> &Platform {
        &self.build_configuration.target_platform
    }

    /// Shorthand to retrieve the target platform for this output
    pub fn host_platform(&self) -> &PlatformWithVirtualPackages {
        &self.build_configuration.host_platform
    }

    /// Search for the resolved package with the given name in the host prefix
    /// Returns a tuple of the package and a boolean indicating whether the
    /// package is directly requested
    pub fn find_resolved_package(&self, name: &str) -> Option<(&RepoDataRecord, bool)> {
        let host = self.finalized_dependencies.as_ref()?.host.as_ref()?;
        let record = host
            .resolved
            .iter()
            .find(|p| p.package_record.name.as_normalized() == name);

        let is_requested = host.specs.iter().any(|s| {
            s.spec()
                .name
                .as_ref()
                .map(|n| n.to_string() == name)
                .unwrap_or(false)
        });

        record.map(|r| (r, is_requested))
    }

    /// Print a nice summary of the build
    pub fn log_build_summary(&self) -> Result<(), std::io::Error> {
        let summary = self.build_summary.lock().unwrap();
        let identifier = self.identifier();
        let span = tracing::info_span!(
            "Build summary for",
            recipe = identifier,
            span_color = identifier
        );
        let _enter = span.enter();

        tracing::info!("{}", self);

        if !summary.warnings.is_empty() {
            tracing::warn!("Warnings:");
            for warning in &summary.warnings {
                tracing::warn!("{}", warning);
            }
        }

        if let Some(artifact) = &summary.artifact {
            let bytes = HumanBytes(fs::metadata(artifact).map(|m| m.len()).unwrap_or(0));
            tracing::info!("Artifact: {} ({})", artifact.display(), bytes);
        } else {
            tracing::info!("No artifact was created");
        }

        if let Ok(github_summary) = std::env::var("GITHUB_STEP_SUMMARY") {
            if !github_integration_enabled() {
                return Ok(());
            }
            // append to the summary file
            let mut summary_file = fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(github_summary)?;

            writeln!(summary_file, "### Build summary for {}", identifier)?;
            if let Some(article) = &summary.artifact {
                let bytes = HumanBytes(fs::metadata(article).map(|m| m.len()).unwrap_or(0));
                writeln!(
                    summary_file,
                    "**Artifact**: {} ({})",
                    article.display(),
                    bytes
                )?;
            } else {
                writeln!(summary_file, "**No artifact was created**")?;
            }

            if let Some(paths) = &summary.paths {
                if paths.paths.is_empty() {
                    writeln!(summary_file, "Included files: **No files included**")?;
                } else {
                    /// Github detail expander
                    fn format_entry(entry: &PathsEntry) -> String {
                        let mut extra_info = Vec::new();
                        if entry.prefix_placeholder.is_some() {
                            extra_info.push("contains prefix");
                        }
                        if entry.no_link {
                            extra_info.push("no link");
                        }
                        match entry.path_type {
                            PathType::SoftLink => extra_info.push("soft link"),
                            // skip default
                            PathType::HardLink => {}
                            PathType::Directory => extra_info.push("directory"),
                        }
                        let bytes = entry.size_in_bytes.unwrap_or(0);

                        format!(
                            "| `{}` | {} | {} |",
                            entry.relative_path.to_string_lossy(),
                            HumanBytes(bytes),
                            extra_info.join(", ")
                        )
                    }

                    writeln!(summary_file, "<details>")?;
                    writeln!(
                        summary_file,
                        "<summary>Included files ({} files)</summary>\n",
                        paths.paths.len()
                    )?;
                    writeln!(summary_file, "| Path | Size | Extra info |")?;
                    writeln!(summary_file, "| --- | --- | --- |")?;
                    for path in &paths.paths {
                        writeln!(summary_file, "{}", format_entry(path))?;
                    }
                    writeln!(summary_file, "\n</details>\n")?;
                }
            }

            if !summary.warnings.is_empty() {
                writeln!(summary_file, "> [!WARNING]")?;
                writeln!(summary_file, "> **Warnings during build:**\n>")?;
                for warning in &summary.warnings {
                    writeln!(summary_file, "> - {}", warning)?;
                }
                writeln!(summary_file)?;
            }

            writeln!(
                summary_file,
                "<details><summary>Resolved dependencies</summary>\n\n{}\n</details>\n",
                self.format_as_markdown()
            )?;
        }
        Ok(())
    }

    /// Format the output as a markdown table
    pub fn format_as_markdown(&self) -> String {
        let mut output = String::new();
        self.format_table_with_option(&mut output, comfy_table::presets::ASCII_MARKDOWN, true)
            .expect("Could not format table");
        output
    }

    fn format_table_with_option(
        &self,
        f: &mut impl fmt::Write,
        table_format: &str,
        long: bool,
    ) -> std::fmt::Result {
        let template = || -> comfy_table::Table {
            let mut table = comfy_table::Table::new();
            if table_format == comfy_table::presets::UTF8_FULL {
                table
                    .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
                    .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS);
            } else {
                table.load_preset(table_format);
            }
            table
        };

        writeln!(f, "Variant configuration (hash: {}):", self.build_string())?;
        let mut table = template();
        if table_format != comfy_table::presets::UTF8_FULL {
            table.set_header(["Key", "Value"]);
        }
        self.build_configuration.variant.iter().for_each(|(k, v)| {
            table.add_row([k.normalize(), format!("{:?}", v)]);
        });
        writeln!(f, "{}\n", table)?;

        if let Some(finalized_dependencies) = &self.finalized_dependencies {
            if let Some(build) = &finalized_dependencies.build {
                writeln!(f, "Build dependencies:")?;
                writeln!(f, "{}\n", build.to_table(template(), long))?;
            }

            if let Some(host) = &finalized_dependencies.host {
                writeln!(f, "Host dependencies:")?;
                writeln!(f, "{}\n", host.to_table(template(), long))?;
            }

            if !finalized_dependencies.run.depends.is_empty() {
                writeln!(f, "Run dependencies:")?;
                writeln!(
                    f,
                    "{}\n",
                    finalized_dependencies.run.to_table(template(), long)
                )?;
            }
        }

        Ok(())
    }
}

impl Display for BuildOutput {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.format_table_with_option(f, comfy_table::presets::UTF8_FULL, false)
    }
}

impl crate::post_process::path_checks::WarningRecorder for BuildOutput {
    fn record_warning(&self, warning: &str) {
        self.record_warning(warning);
    }
}
