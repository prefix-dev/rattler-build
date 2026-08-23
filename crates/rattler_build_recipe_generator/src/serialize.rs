use std::fmt;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use indexmap::IndexMap;
use serde::{Serialize, Serializer};
use serde_with::{OneOrMany, formats::PreferOne, serde_as};
use serde_yaml::{Mapping, Value};

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum SourceElement {
    Url(UrlSourceElement),
    Git(GitSourceElement),
}

impl From<UrlSourceElement> for SourceElement {
    fn from(url: UrlSourceElement) -> Self {
        SourceElement::Url(url)
    }
}

impl From<GitSourceElement> for SourceElement {
    fn from(git: GitSourceElement) -> Self {
        SourceElement::Git(git)
    }
}

#[serde_as]
#[derive(Default, Debug, Serialize)]
pub struct UrlSourceElement {
    #[serde_as(as = "OneOrMany<_, PreferOne>")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub url: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub md5: Option<String>,
}

#[derive(Default, Debug, Serialize)]
pub struct GitSourceElement {
    pub git: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

#[derive(Default, Debug, Serialize)]
pub struct Build {
    #[serde(serialize_with = "serialize_build_number")]
    pub number: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub noarch: Option<String>,
    pub script: Script,
    #[serde(skip_serializing_if = "Python::is_default")]
    pub python: Python,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dynamic_linking: Option<DynamicLinking>,
}

/// Emit the build number as an integer when it is one (e.g. `0`) and as the
/// raw string otherwise (e.g. `${{ build_number }}`).
fn serialize_build_number<S: Serializer>(number: &str, serializer: S) -> Result<S::Ok, S::Error> {
    match number.parse::<u64>() {
        Ok(number) => serializer.serialize_u64(number),
        Err(_) => serializer.serialize_str(number),
    }
}

/// The `build.script` field: a single command, or a list of steps.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(untagged)]
pub enum Script {
    Command(String),
    Steps(Vec<ScriptStep>),
}

impl Default for Script {
    fn default() -> Self {
        Script::Command(String::new())
    }
}

impl From<String> for Script {
    fn from(command: String) -> Self {
        Script::Command(command)
    }
}

impl From<&str> for Script {
    fn from(command: &str) -> Self {
        Script::Command(command.to_string())
    }
}

/// One step of a `build.script` list: an `if`/`then`/`else` selector choosing
/// between commands.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ScriptStep {
    #[serde(rename = "if")]
    pub condition: String,
    pub then: String,
    #[serde(rename = "else", skip_serializing_if = "Option::is_none")]
    pub otherwise: Option<String>,
}

/// One entry of a requirements list: a match spec, or an `if`/`then`
/// selector adding specs only when the condition holds.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(untagged)]
pub enum Requirement {
    Spec(String),
    Conditional {
        #[serde(rename = "if")]
        condition: String,
        then: Vec<String>,
    },
}

impl From<String> for Requirement {
    fn from(spec: String) -> Self {
        Requirement::Spec(spec)
    }
}

impl From<&str> for Requirement {
    fn from(spec: &str) -> Self {
        Requirement::Spec(spec.to_string())
    }
}

/// Dynamic linking settings for compiled packages (e.g. `rpaths`).
#[derive(Default, Debug, Serialize)]
pub struct DynamicLinking {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rpaths: Vec<String>,
}

#[derive(Default, Debug, Serialize)]
pub struct Python {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub entry_points: Vec<String>,
}

impl Python {
    fn is_default(&self) -> bool {
        self.entry_points.is_empty()
    }
}

#[derive(Default, Debug, Serialize)]
pub struct About {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub license_file: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    /// Optional YAML comment to emit above the `license:` field.
    /// Not serialized by serde — injected by the `Display` impl.
    #[serde(skip)]
    pub license_warning: Option<String>,
}

#[derive(Default, Debug, Serialize)]
pub struct Package {
    pub name: String,
    pub version: String,
}

/// Extra files from the source checkout to copy into the test environment.
#[derive(Default, Debug, Serialize)]
pub struct ScriptTestFiles {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<String>,
}

impl ScriptTestFiles {
    fn is_empty(&self) -> bool {
        self.source.is_empty()
    }
}

/// Additional packages to install into the test environment.
#[derive(Default, Debug, Serialize)]
pub struct ScriptTestRequirements {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub run: Vec<String>,
}

impl ScriptTestRequirements {
    fn is_empty(&self) -> bool {
        self.run.is_empty()
    }
}

#[derive(Default, Debug, Serialize)]
pub struct ScriptTest {
    #[serde(skip_serializing_if = "ScriptTestFiles::is_empty")]
    pub files: ScriptTestFiles,
    #[serde(skip_serializing_if = "ScriptTestRequirements::is_empty")]
    pub requirements: ScriptTestRequirements,
    pub script: Vec<String>,
}

#[derive(Default, Debug, Serialize)]
pub struct PythonTestInner {
    pub imports: Vec<String>,
    pub pip_check: bool,
}

#[derive(Default, Debug, Serialize)]
pub struct PythonTest {
    pub python: PythonTestInner,
}

#[derive(Default, Debug, Serialize)]
pub struct RTestInner {
    pub libraries: Vec<String>,
}

/// The `r:` test type: load each library in a fresh R session.
#[derive(Default, Debug, Serialize)]
pub struct RTest {
    pub r: RTestInner,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Test {
    Script(ScriptTest),
    Python(PythonTest),
    R(RTest),
}

/// Free-form `extra:` section.
#[derive(Default, Debug, Serialize)]
pub struct Extra {
    #[serde(rename = "recipe-maintainers", skip_serializing_if = "Vec::is_empty")]
    pub recipe_maintainers: Vec<String>,
}

impl Extra {
    fn is_empty(&self) -> bool {
        self.recipe_maintainers.is_empty()
    }
}

/// The recipe format version. This crate only emits the v1 format.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct SchemaVersion(u32);

impl Default for SchemaVersion {
    fn default() -> Self {
        Self(1)
    }
}

#[serde_as]
#[derive(Default, Debug, Serialize)]
pub struct Recipe {
    pub schema_version: SchemaVersion,
    pub context: IndexMap<String, String>,
    pub package: Package,
    /// A single source is emitted as a mapping, several as a list.
    #[serde_as(as = "OneOrMany<_, PreferOne>")]
    pub source: Vec<SourceElement>,
    pub build: Build,
    pub requirements: Requirements,
    pub tests: Vec<Test>,
    pub about: About,
    #[serde(skip_serializing_if = "Extra::is_empty")]
    pub extra: Extra,
}

#[derive(Default, Debug, Serialize)]
pub struct Requirements {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub build: Vec<Requirement>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub host: Vec<Requirement>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub run: Vec<Requirement>,
}

impl fmt::Display for Recipe {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let value = serde_yaml::to_value(self).map_err(|_| fmt::Error)?;
        let string = emit_document(&value);
        // add a newline before every top-level key
        let lines = string
            .trim_end_matches('\n')
            .split('\n')
            .collect::<Vec<&str>>();
        let mut first_line = true;
        for line in lines {
            if line.chars().next().map(|c| c.is_alphabetic()) == Some(true) && !first_line {
                writeln!(f)?;
            }
            first_line = false;
            // Inject a warning comment above the license field if present
            if line.starts_with("  license:")
                && let Some(warning) = &self.about.license_warning
            {
                for comment_line in warning.lines() {
                    writeln!(f, "  # {comment_line}")?;
                }
            }
            writeln!(f, "{}", line)?;
        }
        Ok(())
    }
}

/// Render a recipe as block-style YAML with sequences indented under their
/// parent key. `serde_yaml` (libyaml) always emits *indentless* sequences,
/// which is not the style used by recipes in the wild, so the tree is walked
/// here instead; scalar quoting is still delegated to `serde_yaml`.
fn emit_document(root: &Value) -> String {
    let mut out = String::new();
    if let Value::Mapping(map) = root {
        for (key, value) in map {
            match value {
                // `Recipe::context` holds versions and the like, which follow
                // their own quoting rule (see `context_value_text`).
                Value::Mapping(context)
                    if key.as_str() == Some("context") && !context.is_empty() =>
                {
                    out.push_str("context:\n");
                    for (name, value) in context {
                        out.push_str("  ");
                        out.push_str(&scalar_text(name, 2));
                        out.push_str(": ");
                        out.push_str(&context_value_text(value));
                        out.push('\n');
                    }
                }
                _ => emit_entry(&mut out, key, value, 0),
            }
        }
    }
    out
}

/// Text of a value under `context:`. Anything that starts with a digit
/// (typically a version such as `1.0` or `0.3.1`) is double-quoted so it is
/// never re-read as a number, matching the conda-forge convention; values that
/// `serde_yaml` would quote anyway are double-quoted as well, for consistency.
fn context_value_text(value: &Value) -> String {
    let rendered = scalar_text(value, 2);
    let Value::String(string) = value else {
        return rendered;
    };
    let needs_quotes = string.starts_with(|c: char| c.is_ascii_digit()) || rendered != *string;
    if needs_quotes && !string.contains('\n') {
        format!("\"{}\"", string.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        rendered
    }
}

/// Write `key: value` (the caller has already written the indentation of the
/// key) and descend into nested mappings and sequences.
fn emit_entry(out: &mut String, key: &Value, value: &Value, indent: usize) {
    out.push_str(&scalar_text(key, indent));
    out.push(':');
    match value {
        Value::Mapping(map) if !map.is_empty() => {
            out.push('\n');
            emit_mapping(out, map, indent + 2, false);
        }
        Value::Sequence(seq) if !seq.is_empty() => {
            out.push('\n');
            emit_sequence(out, seq, indent + 2);
        }
        _ => {
            out.push(' ');
            out.push_str(&scalar_text(value, indent));
            out.push('\n');
        }
    }
}

/// Write the entries of `map` at `indent`. With `inline_first` the first key
/// continues the current line (right after a `- ` sequence marker).
fn emit_mapping(out: &mut String, map: &Mapping, indent: usize, inline_first: bool) {
    for (i, (key, value)) in map.iter().enumerate() {
        if !(inline_first && i == 0) {
            out.push_str(&" ".repeat(indent));
        }
        emit_entry(out, key, value, indent);
    }
}

/// Write the items of `seq` as `- item` lines at `indent`.
fn emit_sequence(out: &mut String, seq: &[Value], indent: usize) {
    for item in seq {
        out.push_str(&" ".repeat(indent));
        match item {
            Value::Mapping(map) if !map.is_empty() => {
                out.push_str("- ");
                emit_mapping(out, map, indent + 2, true);
            }
            Value::Sequence(inner) if !inner.is_empty() => {
                out.push_str("-\n");
                emit_sequence(out, inner, indent + 2);
            }
            _ => {
                out.push_str("- ");
                out.push_str(&scalar_text(item, indent));
                out.push('\n');
            }
        }
    }
}

/// Text of a scalar (or of an empty collection), using `serde_yaml` for the
/// quoting decision. Multi-line strings become `|-` block scalars whose body
/// is re-indented relative to `indent`, the column of the owning key or dash.
fn scalar_text(value: &Value, indent: usize) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(string) => string_text(string, indent),
        Value::Mapping(map) if map.is_empty() => "{}".to_string(),
        Value::Sequence(seq) if seq.is_empty() => "[]".to_string(),
        // Non-empty collections are always handled by `emit_entry` /
        // `emit_sequence`; keep a valid fallback rather than panicking.
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim_end()
            .to_string(),
    }
}

fn string_text(string: &str, indent: usize) -> String {
    // Trailing newlines carry no meaning in a recipe, and keeping them would
    // make libyaml choose the `|+` (keep) block style, whose trailing blank
    // lines the surrounding layout does not preserve. Drop them so multi-line
    // text always becomes a `|-` block and single lines stay plain scalars.
    let string = string.trim_end_matches('\n');
    let rendered = serde_yaml::to_string(&Value::String(string.to_string())).unwrap_or_default();
    let rendered = rendered.trim_end_matches('\n');

    if string.contains('\n') {
        // libyaml emits `|-` followed by the body indented by two spaces;
        // shift the body so it sits two deeper than our key.
        let mut lines = rendered.lines();
        let mut text = lines.next().unwrap_or("|-").to_string();
        for line in lines {
            text.push('\n');
            if !line.is_empty() {
                text.push_str(&" ".repeat(indent));
            }
            text.push_str(line);
        }
        return text;
    }

    rendered.to_string()
}

/// Write a recipe to "{package_name}/recipe.yaml"
#[cfg(not(target_arch = "wasm32"))]
pub fn write_recipe(package_name: &str, recipe: &str) -> std::io::Result<()> {
    let path = PathBuf::from(format!("{package_name}/recipe.yaml"));
    fs_err::create_dir_all(path.parent().unwrap())?;

    if path.exists() {
        // move to backup
        let backup_path = path.with_extension("yaml.bak");
        tracing::warn!(
            "Existing recipe file will be backed up to {}",
            backup_path.display()
        );
        fs_err::rename(&path, backup_path)?;
    }

    tracing::info!("Writing recipe to {}", path.display());

    fs_err::write(path, recipe)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_recipe() -> Recipe {
        let mut recipe = Recipe::default();
        recipe
            .context
            .insert("version".to_string(), "1.0".to_string());
        recipe
            .context
            .insert("build_number".to_string(), "0".to_string());
        recipe
            .context
            .insert("name".to_string(), "${{ env.get('NAME') }}".to_string());
        recipe
            .context
            .insert("mirror".to_string(), "https://example.com".to_string());
        recipe.package.name = "demo".to_string();
        recipe.package.version = "${{ version }}".to_string();
        recipe.source.push(
            UrlSourceElement {
                url: vec!["https://example.com/demo-${{ version }}.tar.gz".to_string()],
                sha256: Some("abc".to_string()),
                md5: None,
            }
            .into(),
        );
        recipe.build.number = "${{ build_number }}".to_string();
        recipe.build.script = "make install".into();
        recipe.build.noarch = Some("generic".to_string());
        recipe.requirements.host = vec!["r-base".into()];
        recipe.requirements.build = vec![
            Requirement::Conditional {
                condition: "build_platform != target_platform".to_string(),
                then: vec!["cross-r-base ${{ r_base }}".to_string()],
            },
            "${{ compiler('c') }}".into(),
        ];
        recipe.tests.push(Test::R(RTest {
            r: RTestInner {
                libraries: vec!["demo".to_string()],
            },
        }));
        recipe.tests.push(Test::Script(ScriptTest {
            files: ScriptTestFiles {
                source: vec!["tests/".to_string()],
            },
            requirements: ScriptTestRequirements {
                run: vec!["r-testthat".to_string()],
            },
            script: vec!["Rscript -e \"cat('hi: there')\"".to_string()],
        }));
        recipe.about.summary = Some("A demo: with a colon".to_string());
        recipe.about.description =
            Some("First line.\n\nThird line, indented:\n  - item".to_string());
        recipe.about.license = Some("MIT".to_string());
        recipe.about.license_file = vec!["LICENSE".to_string()];
        recipe.about.license_warning = Some("check this".to_string());
        recipe.extra.recipe_maintainers = vec!["octocat".to_string()];
        recipe
    }

    #[test]
    fn emits_indented_sequences_and_single_source_mapping() {
        insta::assert_snapshot!(sample_recipe().to_string());
    }

    #[test]
    fn emits_integer_build_number_and_multiple_sources_as_list() {
        let mut recipe = sample_recipe();
        recipe.build.number = "0".to_string();
        recipe.source.push(
            GitSourceElement {
                git: "https://example.com/demo.git".to_string(),
                tag: Some("v1.0".to_string()),
                branch: None,
            }
            .into(),
        );
        recipe.tests.clear();
        recipe.extra = Extra::default();
        recipe.about.license_warning = None;
        insta::assert_snapshot!(recipe.to_string());
    }

    #[test]
    fn context_values_starting_with_a_digit_are_quoted() {
        let mut recipe = Recipe::default();
        recipe
            .context
            .insert("version".to_string(), "0.7-5.1".to_string());
        recipe
            .context
            .insert("posix".to_string(), "'m2-' if win else ''".to_string());
        recipe
            .context
            .insert("flag".to_string(), "true".to_string());
        recipe
            .context
            .insert("plain".to_string(), "hello".to_string());
        recipe
            .context
            .insert("inner_quotes".to_string(), "say \"hi\"".to_string());
        let yaml = recipe.to_string();
        // starts with a digit -> always quoted
        assert!(yaml.contains("  version: \"0.7-5.1\"\n"), "{yaml}");
        // serde_yaml would single-quote these -> double-quoted instead
        assert!(
            yaml.contains("  posix: \"'m2-' if win else ''\"\n"),
            "{yaml}"
        );
        assert!(yaml.contains("  flag: \"true\"\n"), "{yaml}");
        // plain scalars stay plain (YAML allows inner double quotes)
        assert!(yaml.contains("  plain: hello\n"), "{yaml}");
        assert!(yaml.contains("  inner_quotes: say \"hi\"\n"), "{yaml}");
    }

    #[test]
    fn trailing_newlines_are_dropped_rather_than_kept_by_the_block_style() {
        let mut recipe = Recipe::default();
        recipe.about.summary = Some("one line\n".to_string());
        recipe.about.description = Some("First.\nSecond.\n\n".to_string());
        let yaml = recipe.to_string();
        assert!(yaml.contains("  summary: one line\n"), "{yaml}");
        assert!(
            yaml.contains("  description: |-\n    First.\n    Second.\n"),
            "{yaml}"
        );
        let reparsed: Value = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(reparsed["about"]["summary"].as_str(), Some("one line"));
        assert_eq!(
            reparsed["about"]["description"].as_str(),
            Some("First.\nSecond.")
        );
    }

    #[test]
    fn emitted_yaml_round_trips() {
        let recipe = sample_recipe();
        let yaml = recipe.to_string();
        let reparsed: Value = serde_yaml::from_str(&yaml).expect("emitted YAML must parse");
        let expected = serde_yaml::to_value(&recipe).unwrap();
        assert_eq!(reparsed, expected);
    }
}
