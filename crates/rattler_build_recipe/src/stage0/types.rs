use std::{
    collections::BTreeSet,
    fmt::{Debug, Display},
    str::FromStr,
};

use serde::{Deserialize, Serialize};

/// Trait for types that can report which template variables they use
#[allow(dead_code)]
pub trait UsedVariables {
    fn used_variables(&self) -> Vec<String>;
}

/// A validated Jinja2 template with pre-computed variable dependencies
/// Templates include the `${{ }}` delimiters, e.g., `${{ name }}-${{ version }}`
#[derive(Debug, Clone, PartialEq)]
pub struct JinjaTemplate {
    source: String,
    variables: Vec<String>,
}

impl JinjaTemplate {
    /// Create a new JinjaTemplate, validating that it parses correctly
    pub fn new(source: String) -> Result<Self, String> {
        let variables = extract_variables_from_template(&source)
            .map_err(|e| format!("Failed to parse Jinja template: {}", e))?;
        Ok(Self { source, variables })
    }

    /// Get the raw template source
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Get the raw template source (alias for consistency)
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// Get the pre-computed list of variables used in this template (cached)
    pub fn used_variables(&self) -> &[String] {
        &self.variables
    }
}

impl Display for JinjaTemplate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.source)
    }
}

impl Serialize for JinjaTemplate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Serialize as just the source string
        self.source.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JinjaTemplate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let source = String::deserialize(deserializer)?;
        JinjaTemplate::new(source).map_err(serde::de::Error::custom)
    }
}

/// A validated Jinja2 expression (without `${{ }}` delimiters) used in conditionals
/// Examples: `linux`, `version >= '3.0'`, `target_platform == 'linux'`
#[derive(Debug, Clone, PartialEq)]
pub struct JinjaExpression {
    source: String,
    variables: Vec<String>,
}

impl JinjaExpression {
    /// Create a new JinjaExpression, validating that it parses correctly
    pub fn new(source: String) -> Result<Self, String> {
        let variables = extract_variables_from_expression_checked(&source)
            .map_err(|e| format!("Failed to parse Jinja expression: {}", e))?;
        Ok(Self { source, variables })
    }

    /// Get the raw expression source
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Get the pre-computed list of variables used in this expression (cached)
    pub fn used_variables(&self) -> &[String] {
        &self.variables
    }
}

impl Display for JinjaExpression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.source)
    }
}

impl Serialize for JinjaExpression {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Serialize as just the source string
        self.source.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JinjaExpression {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let source = String::deserialize(deserializer)?;
        JinjaExpression::new(source).map_err(serde::de::Error::custom)
    }
}

/// Extract variable names from a Jinja2 template string using minijinja's AST parsing
/// Returns Result for validation during construction of JinjaTemplate
fn extract_variables_from_template(template: &str) -> Result<Vec<String>, minijinja::Error> {
    use minijinja::machinery::{WhitespaceConfig, parse};

    let mut variables = BTreeSet::new();

    // Parse the template directly
    let ast = parse(
        template,
        "template",
        Default::default(),
        WhitespaceConfig::default(),
    )?;

    // Walk the AST and collect variable names
    collect_variables_from_ast(&ast, &mut variables);

    Ok(variables.into_iter().collect())
}

/// Extract variable names from a Jinja2 expression (without ${{ }} delimiters)
/// Returns Result for validation during construction of JinjaExpression
fn extract_variables_from_expression_checked(expr: &str) -> Result<Vec<String>, minijinja::Error> {
    use minijinja::machinery::parse_expr;

    let mut variables = BTreeSet::new();

    // Parse just the expression
    let ast = parse_expr(expr)?;
    collect_variables_from_expr(&ast, &mut variables);

    Ok(variables.into_iter().collect())
}

/// Recursively collect variable names from the minijinja AST
fn collect_variables_from_ast(
    node: &minijinja::machinery::ast::Stmt,
    variables: &mut BTreeSet<String>,
) {
    use minijinja::machinery::ast::Stmt;

    match node {
        Stmt::Template(template) => {
            for child in &template.children {
                collect_variables_from_ast(child, variables);
            }
        }
        Stmt::EmitExpr(emit) => {
            collect_variables_from_expr(&emit.expr, variables);
        }
        Stmt::ForLoop(for_loop) => {
            collect_variables_from_expr(&for_loop.iter, variables);
            for child in &for_loop.body {
                collect_variables_from_ast(child, variables);
            }
            for child in &for_loop.else_body {
                collect_variables_from_ast(child, variables);
            }
        }
        Stmt::IfCond(if_cond) => {
            collect_variables_from_expr(&if_cond.expr, variables);
            for child in &if_cond.true_body {
                collect_variables_from_ast(child, variables);
            }
            for child in &if_cond.false_body {
                collect_variables_from_ast(child, variables);
            }
        }
        Stmt::WithBlock(with_block) => {
            for (_target, expr) in &with_block.assignments {
                collect_variables_from_expr(expr, variables);
            }
            for child in &with_block.body {
                collect_variables_from_ast(child, variables);
            }
        }
        Stmt::Set(set) => {
            collect_variables_from_expr(&set.expr, variables);
        }
        Stmt::SetBlock(_) => {
            // SetBlock doesn't contain variable references we care about
        }
        Stmt::AutoEscape(auto_escape) => {
            collect_variables_from_expr(&auto_escape.enabled, variables);
            for child in &auto_escape.body {
                collect_variables_from_ast(child, variables);
            }
        }
        Stmt::FilterBlock(filter_block) => {
            collect_variables_from_expr(&filter_block.filter, variables);
            for child in &filter_block.body {
                collect_variables_from_ast(child, variables);
            }
        }
        Stmt::Block(block) => {
            for child in &block.body {
                collect_variables_from_ast(child, variables);
            }
        }
        Stmt::Extends(_) | Stmt::Include(_) => {
            // These don't contain variable references we care about for now
        }
        Stmt::Import(_) | Stmt::FromImport(_) => {
            // Import statements don't contain variable references we care about
        }
        Stmt::Do(do_stmt) => {
            collect_variables_from_call(&do_stmt.call, variables);
        }
        Stmt::EmitRaw(_) => {
            // Raw text doesn't contain variables
        }
        Stmt::Macro(_) => {
            // Macro definitions don't contain variable references we care about
        }
        Stmt::CallBlock(_) => {
            // CallBlock doesn't contain variable references we care about
        }
    }
}

/// Collect variables from a Call expression
fn collect_variables_from_call(
    call: &minijinja::machinery::ast::Call,
    variables: &mut BTreeSet<String>,
) {
    collect_variables_from_expr(&call.expr, variables);
    for arg in &call.args {
        collect_variables_from_call_arg(arg, variables);
    }
}

/// Collect variables from a CallArg
fn collect_variables_from_call_arg(
    arg: &minijinja::machinery::ast::CallArg,
    variables: &mut BTreeSet<String>,
) {
    use minijinja::machinery::ast::CallArg;

    // Match based on the CallArg variants available
    match arg {
        CallArg::Pos(expr) => {
            collect_variables_from_expr(expr, variables);
        }
        _ => {
            // Handle other CallArg variants if they exist
        }
    }
}

/// Recursively collect variable names from expressions
fn collect_variables_from_expr(
    expr: &minijinja::machinery::ast::Expr,
    variables: &mut BTreeSet<String>,
) {
    use minijinja::machinery::ast::Expr;

    match expr {
        Expr::Var(var) => {
            variables.insert(var.id.to_string());
        }
        Expr::Const(_) => {
            // Constants don't contain variables
        }
        Expr::UnaryOp(unary) => {
            collect_variables_from_expr(&unary.expr, variables);
        }
        Expr::BinOp(binop) => {
            collect_variables_from_expr(&binop.left, variables);
            collect_variables_from_expr(&binop.right, variables);
        }
        Expr::IfExpr(if_expr) => {
            collect_variables_from_expr(&if_expr.test_expr, variables);
            collect_variables_from_expr(&if_expr.true_expr, variables);
            if let Some(false_expr) = &if_expr.false_expr {
                collect_variables_from_expr(false_expr, variables);
            }
        }
        Expr::Filter(filter) => {
            if let Some(expr) = &filter.expr {
                collect_variables_from_expr(expr, variables);
            }
            for arg in &filter.args {
                collect_variables_from_call_arg(arg, variables);
            }
        }
        Expr::Test(test) => {
            collect_variables_from_expr(&test.expr, variables);
            for arg in &test.args {
                collect_variables_from_call_arg(arg, variables);
            }
        }
        Expr::GetAttr(get_attr) => {
            collect_variables_from_expr(&get_attr.expr, variables);
        }
        Expr::GetItem(get_item) => {
            collect_variables_from_expr(&get_item.expr, variables);
            collect_variables_from_expr(&get_item.subscript_expr, variables);
        }
        Expr::Slice(slice) => {
            collect_variables_from_expr(&slice.expr, variables);
            if let Some(start) = &slice.start {
                collect_variables_from_expr(start, variables);
            }
            if let Some(stop) = &slice.stop {
                collect_variables_from_expr(stop, variables);
            }
            if let Some(step) = &slice.step {
                collect_variables_from_expr(step, variables);
            }
        }
        Expr::Call(call) => {
            collect_variables_from_call(call, variables);
        }
        Expr::List(list) => {
            for item in &list.items {
                collect_variables_from_expr(item, variables);
            }
        }
        Expr::Map(map) => {
            for expr in &map.keys {
                collect_variables_from_expr(expr, variables);
            }
        }
    }
}

// Core enum for values that can be either concrete or templated
#[derive(Debug, Clone, PartialEq)]
pub enum Value<T> {
    Concrete(T),
    Template(JinjaTemplate), // Validated Jinja template
}

impl<T: ToString> Value<T> {
    /// Get the list of variables used in this value (cached for templates)
    pub fn used_variables(&self) -> Vec<String> {
        match self {
            Value::Concrete(_) => Vec::new(),
            Value::Template(template) => template.used_variables().to_vec(),
        }
    }
}

// Custom serialization for Value
impl<T: Serialize> Serialize for Value<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Value::Concrete(val) => val.serialize(serializer),
            Value::Template(template) => template.serialize(serializer),
        }
    }
}

// Custom deserialization for Value
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Value<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        // First try to deserialize as a string
        let value = serde_json::Value::deserialize(deserializer)?;

        if let Some(s) = value.as_str() {
            // Check if it's a template
            if s.contains("${{") {
                let template = JinjaTemplate::new(s.to_string()).map_err(D::Error::custom)?;
                return Ok(Value::Template(template));
            }
        }

        // Otherwise, deserialize as T
        let concrete = T::deserialize(value).map_err(D::Error::custom)?;
        Ok(Value::Concrete(concrete))
    }
}

impl<T: Display> Display for Value<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Concrete(val) => write!(f, "{val}"),
            Value::Template(template) => write!(f, "{template}"),
        }
    }
}

impl<T: ToString> Value<T> {
    pub fn concrete(&self) -> Option<&T> {
        if let Value::Concrete(val) = self {
            Some(val)
        } else {
            None
        }
    }
}

impl<T: ToString + FromStr> FromStr for Value<T>
where
    T::Err: std::fmt::Display,
{
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains("${{") {
            // If it contains some template syntax, validate and create template
            let template = JinjaTemplate::new(s.to_string())?;
            return Ok(Value::Template(template));
        }

        T::from_str(s)
            .map(Value::Concrete)
            .map_err(|e| format!("Failed to parse concrete value: {}", e))
    }
}

// Any item in a list can be either a value or a conditional
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Item<T> {
    Value(Value<T>),
    Conditional(Conditional<T>),
}

impl<T> Item<T> {
    pub fn new_from_conditional(
        condition: String,
        then: Vec<T>,
        else_value: Vec<T>,
    ) -> Result<Self, String> {
        let condition = JinjaExpression::new(condition)?;
        Ok(Item::Conditional(Conditional {
            condition,
            then: ListOrItem::new(then),
            else_value: ListOrItem::new(else_value),
        }))
    }
}

impl<T: Display> Display for Item<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Item::Value(value) => write!(f, "{value}"),
            Item::Conditional(cond) => write!(f, "{cond}"),
        }
    }
}

impl<T: PartialEq> PartialEq for Item<T> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Item::Value(Value::Concrete(a)), Item::Value(Value::Concrete(b))) => a == b,
            (Item::Conditional(a), Item::Conditional(b)) => {
                a.condition == b.condition && a.then == b.then && a.else_value == b.else_value
            }
            _ => false,
        }
    }
}

impl<T: Debug> Debug for Item<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Item::Value(value) => write!(f, "Value({value:?})"),
            Item::Conditional(cond) => write!(f, "Conditional({cond:?})"),
        }
    }
}

impl<T> From<Conditional<T>> for Item<T> {
    fn from(value: Conditional<T>) -> Self {
        Self::Conditional(value)
    }
}

impl<T: ToString + FromStr> FromStr for Item<T>
where
    T::Err: std::fmt::Display,
{
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains("${{") {
            // If it contains some template syntax, validate and create template
            let template = JinjaTemplate::new(s.to_string())?;
            return Ok(Item::Value(Value::Template(template)));
        }

        let value = T::from_str(s).map_err(|e| format!("Failed to parse: {}", e))?;
        Ok(Item::Value(Value::Concrete(value)))
    }
}

impl<T> Item<T> {
    /// Collect all variables used in this item
    /// Note: Requires T to be ToString for Conditional variant
    pub fn used_variables(&self) -> Vec<String>
    where
        T: ToString + Default + Debug,
    {
        match self {
            Item::Value(v) => v.used_variables(),
            Item::Conditional(c) => c.used_variables(),
        }
    }
}
#[derive(Clone)]
pub struct ListOrItem<T>(pub Vec<T>);

impl<T> Default for ListOrItem<T> {
    fn default() -> Self {
        ListOrItem(Vec::new())
    }
}

impl<T: PartialEq> PartialEq for ListOrItem<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T: Debug> Debug for ListOrItem<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_empty() {
            write!(f, "ListOrItem([])")
        } else if self.0.len() == 1 {
            write!(f, "ListOrItem({:?})", self.0[0])
        } else {
            write!(f, "ListOrItem({:?})", self.0)
        }
    }
}

impl<T: FromStr> FromStr for ListOrItem<T> {
    type Err = T::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(ListOrItem::single(s.parse()?))
    }
}

impl<T> serde::Serialize for ListOrItem<T>
where
    T: serde::Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.0.len() {
            1 => self.0[0].serialize(serializer),
            _ => self.0.serialize(serializer),
        }
    }
}

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for ListOrItem<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use std::fmt;

        use serde::de::{Error, Visitor};

        struct ListOrItemVisitor<T>(std::marker::PhantomData<T>);

        impl<'de, T: serde::Deserialize<'de>> Visitor<'de> for ListOrItemVisitor<T> {
            type Value = ListOrItem<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a single item or a list of items")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut vec = Vec::new();
                while let Some(item) = seq.next_element()? {
                    vec.push(item);
                }
                Ok(ListOrItem(vec))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                let item = T::deserialize(serde::de::value::StrDeserializer::new(value))?;
                Ok(ListOrItem(vec![item]))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: Error,
            {
                let item = T::deserialize(serde::de::value::StringDeserializer::new(value))?;
                Ok(ListOrItem(vec![item]))
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let item = T::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
                Ok(ListOrItem(vec![item]))
            }
        }

        deserializer.deserialize_any(ListOrItemVisitor(std::marker::PhantomData))
    }
}

impl<T: ToString> Display for ListOrItem<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.len() {
            0 => write!(f, "[]"),
            1 => write!(f, "{}", self.0[0].to_string()),
            _ => write!(
                f,
                "[{}]",
                self.0
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

impl<T> ListOrItem<T> {
    pub fn new(items: Vec<T>) -> Self {
        Self(items)
    }

    pub fn single(item: T) -> Self {
        Self(vec![item])
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> std::slice::Iter<T> {
        self.0.iter()
    }
}

// Conditional structure for if-else logic
#[derive(Clone, PartialEq)]
pub struct Conditional<T> {
    pub condition: JinjaExpression,
    pub then: ListOrItem<T>,
    pub else_value: ListOrItem<T>,
}

// Custom serialization for Conditional
impl<T: Serialize> Serialize for Conditional<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut state = serializer.serialize_struct("Conditional", 3)?;
        state.serialize_field("if", self.condition.source())?;
        state.serialize_field("then", &self.then)?;
        state.serialize_field("else", &self.else_value)?;
        state.end()
    }
}

// Custom deserialization for Conditional
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Conditional<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{Error, MapAccess, Visitor};
        use std::fmt;

        struct ConditionalVisitor<T>(std::marker::PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for ConditionalVisitor<T> {
            type Value = Conditional<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a conditional with if/then/else fields")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut condition = None;
                let mut then = None;
                let mut else_value = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "if" => {
                            let cond_str: String = map.next_value()?;
                            condition =
                                Some(JinjaExpression::new(cond_str).map_err(A::Error::custom)?);
                        }
                        "then" => {
                            then = Some(map.next_value()?);
                        }
                        "else" => {
                            else_value = Some(map.next_value()?);
                        }
                        _ => {
                            let _: serde::de::IgnoredAny = map.next_value()?;
                        }
                    }
                }

                Ok(Conditional {
                    condition: condition.ok_or_else(|| A::Error::missing_field("if"))?,
                    then: then.ok_or_else(|| A::Error::missing_field("then"))?,
                    else_value: else_value.unwrap_or_default(),
                })
            }
        }

        deserializer.deserialize_struct(
            "Conditional",
            &["if", "then", "else"],
            ConditionalVisitor(std::marker::PhantomData),
        )
    }
}

impl<T: Debug> Debug for Conditional<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Conditional {{ condition: {}, then: {:?}, else: {:?} }}",
            self.condition, self.then, self.else_value
        )
    }
}

impl<T: Display> Display for Conditional<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "if {} then {} else {}",
            self.condition.source(),
            self.then,
            self.else_value
        )
    }
}

/// Newtype for lists that can contain conditionals
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConditionalList<T>(Vec<Item<T>>);

impl<T> Default for ConditionalList<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> ConditionalList<T> {
    pub fn new(items: Vec<Item<T>>) -> Self {
        Self(items)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> std::slice::Iter<Item<T>> {
        self.0.iter()
    }

    pub fn into_inner(self) -> Vec<Item<T>> {
        self.0
    }
}

impl<'a, T> IntoIterator for &'a ConditionalList<T> {
    type Item = &'a Item<T>;
    type IntoIter = std::slice::Iter<'a, Item<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<T: ToString + Default + Debug> ConditionalList<T> {
    // used variables in all items
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();
        for item in self.iter() {
            match item {
                Item::Value(v) => vars.extend(v.used_variables()),
                Item::Conditional(c) => {
                    vars.extend(c.used_variables());
                }
            }
        }
        vars.sort();
        vars.dedup();
        vars
    }
}

impl<T: ToString + Default + Debug> Conditional<T> {
    pub fn new(condition: String, then_value: ListOrItem<T>) -> Result<Self, String> {
        let condition = JinjaExpression::new(condition)?;
        Ok(Self {
            condition,
            then: then_value,
            else_value: ListOrItem::default(),
        })
    }

    pub fn with_else(mut self, else_value: ListOrItem<T>) -> Self {
        self.else_value = else_value;
        self
    }

    /// Collect all variables used in this conditional (cached, O(1))
    pub fn used_variables(&self) -> Vec<String> {
        // Variables are pre-computed and cached in JinjaExpression
        self.condition.used_variables().to_vec()
    }
}

impl<T: ToString> Value<T> {
    pub fn is_template(&self) -> bool {
        matches!(self, Value::Template(_))
    }

    pub fn is_concrete(&self) -> bool {
        matches!(self, Value::Concrete(_))
    }
}

/// Script content - either a simple command string or an inline script with options
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScriptContent {
    /// Simple command string or file path
    Command(String),
    /// Inline script with optional interpreter, env vars, and content/file
    Inline(InlineScript),
}

impl Display for ScriptContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptContent::Command(s) => write!(f, "{}", s),
            ScriptContent::Inline(inline) => {
                if inline.content.is_some() {
                    write!(f, "InlineScript(content: [...])")
                } else if let Some(file) = &inline.file {
                    write!(f, "InlineScript(file: {})", file)
                } else {
                    write!(f, "InlineScript(empty)")
                }
            }
        }
    }
}

impl FromStr for ScriptContent {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(ScriptContent::Command(s.to_string()))
    }
}

impl Default for ScriptContent {
    fn default() -> Self {
        ScriptContent::Command(String::new())
    }
}

impl ScriptContent {
    /// Collect all variables used in this script content
    pub fn used_variables(&self) -> Vec<String> {
        match self {
            ScriptContent::Command(_) => Vec::new(),
            ScriptContent::Inline(inline) => inline.used_variables(),
        }
    }
}

/// Inline script specification with content or file reference
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InlineScript {
    /// Optional interpreter (e.g., "bash", "python")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpreter: Option<Value<String>>,

    /// Environment variables for the script
    #[serde(default, skip_serializing_if = "indexmap::IndexMap::is_empty")]
    pub env: indexmap::IndexMap<String, Value<String>>,

    /// Secrets to expose to the script
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secrets: Vec<String>,

    /// Inline script content - can be a string or array of commands with conditionals
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ConditionalList<String>>,

    /// File path to script
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<Value<String>>,
}

impl InlineScript {
    /// Collect all variables used in this inline script
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();

        if let Some(interpreter) = &self.interpreter {
            vars.extend(interpreter.used_variables());
        }

        for value in self.env.values() {
            vars.extend(value.used_variables());
        }

        if let Some(content) = &self.content {
            vars.extend(content.used_variables());
        }

        if let Some(file) = &self.file {
            vars.extend(file.used_variables());
        }

        vars.sort();
        vars.dedup();
        vars
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_concrete_no_variables() {
        let value: Value<String> = Value::Concrete("hello".to_string());
        assert_eq!(value.used_variables(), Vec::<String>::new());
    }

    #[test]
    fn test_value_template_simple_variable() {
        let value: Value<String> =
            Value::Template(JinjaTemplate::new("${{ name }}".to_string()).unwrap());
        let vars = value.used_variables();
        assert_eq!(vars, vec!["name"]);
    }

    #[test]
    fn test_value_template_multiple_variables() {
        let value: Value<String> =
            Value::Template(JinjaTemplate::new("${{ name }}-${{ version }}".to_string()).unwrap());
        let mut vars = value.used_variables();
        vars.sort();
        assert_eq!(vars, vec!["name", "version"]);
    }

    #[test]
    fn test_value_template_with_filter() {
        let value: Value<String> =
            Value::Template(JinjaTemplate::new("${{ name | lower }}".to_string()).unwrap());
        let vars = value.used_variables();
        assert_eq!(vars, vec!["name"]);
    }

    #[test]
    fn test_value_template_with_complex_expression() {
        let value: Value<String> = Value::Template(
            JinjaTemplate::new("${{ name ~ '-' ~ version if linux else name }}".to_string())
                .unwrap(),
        );
        let mut vars = value.used_variables();
        vars.sort();
        assert_eq!(vars, vec!["linux", "name", "version"]);
    }

    #[test]
    fn test_conditional_simple() {
        let cond = Conditional::new("linux".to_string(), ListOrItem::single("gcc".to_string()))
            .unwrap()
            .with_else(ListOrItem::single("clang".to_string()));
        let vars = cond.used_variables();
        assert_eq!(vars, vec!["linux"]);
    }

    #[test]
    fn test_conditional_complex_expression() {
        let cond = Conditional::new(
            "target_platform == 'linux' and version >= '3.0'".to_string(),
            ListOrItem::single("gcc".to_string()),
        )
        .unwrap();
        let mut vars = cond.used_variables();
        vars.sort();
        assert_eq!(vars, vec!["target_platform", "version"]);
    }

    #[test]
    fn test_item_value_variant() {
        let item: Item<String> = Item::Value(Value::Template(
            JinjaTemplate::new("${{ compiler }}".to_string()).unwrap(),
        ));
        let vars = item.used_variables();
        assert_eq!(vars, vec!["compiler"]);
    }

    #[test]
    fn test_item_conditional_variant() {
        let cond = Conditional::new("unix".to_string(), ListOrItem::single("bash".to_string()))
            .unwrap()
            .with_else(ListOrItem::single("cmd".to_string()));
        let item: Item<String> = Item::Conditional(cond);
        let vars = item.used_variables();
        assert_eq!(vars, vec!["unix"]);
    }

    #[test]
    fn test_conditional_list_mixed_items() {
        let items = vec![
            Item::Value(Value::Concrete("static-dep".to_string())),
            Item::Value(Value::Template(
                JinjaTemplate::new("${{ compiler('c') }}".to_string()).unwrap(),
            )),
            Item::Conditional(
                Conditional::new(
                    "linux".to_string(),
                    ListOrItem::single("linux-gcc".to_string()),
                )
                .unwrap()
                .with_else(ListOrItem::single("other-compiler".to_string())),
            ),
            Item::Value(Value::Template(
                JinjaTemplate::new("${{ python }}".to_string()).unwrap(),
            )),
        ];

        let list = ConditionalList::new(items);
        let mut vars = list.used_variables();
        vars.sort();
        // "compiler" is a function call, so it should extract "compiler" as a variable
        assert_eq!(vars, vec!["compiler", "linux", "python"]);
    }

    #[test]
    fn test_conditional_list_deduplication() {
        let items = vec![
            Item::Value(Value::Template(
                JinjaTemplate::new("${{ name }}".to_string()).unwrap(),
            )),
            Item::Value(Value::Template(
                JinjaTemplate::new("${{ name }}-${{ version }}".to_string()).unwrap(),
            )),
            Item::Conditional(
                Conditional::new(
                    "name == 'foo'".to_string(),
                    ListOrItem::single("bar".to_string()),
                )
                .unwrap(),
            ),
        ];

        let list = ConditionalList::new(items);
        let vars = list.used_variables();
        // Should deduplicate "name"
        assert_eq!(vars, vec!["name", "version"]);
    }

    #[test]
    fn test_conditional_list_empty() {
        let list: ConditionalList<String> = ConditionalList::new(vec![]);
        assert_eq!(list.used_variables(), Vec::<String>::new());
    }

    #[test]
    fn test_value_from_str_template() {
        let value: Value<String> = "${{ version }}".parse().unwrap();
        assert!(value.is_template());
        assert_eq!(value.used_variables(), vec!["version"]);
    }

    #[test]
    fn test_value_from_str_concrete() {
        let value: Value<String> = "plain_string".parse().unwrap();
        assert!(value.is_concrete());
        assert_eq!(value.used_variables(), Vec::<String>::new());
    }

    #[test]
    fn test_template_with_binary_operators() {
        let value: Value<String> =
            Value::Template(JinjaTemplate::new("${{ x + y * z }}".to_string()).unwrap());
        let mut vars = value.used_variables();
        vars.sort();
        assert_eq!(vars, vec!["x", "y", "z"]);
    }

    #[test]
    fn test_template_with_comparison() {
        let value: Value<String> = Value::Template(
            JinjaTemplate::new(
                "${{ version >= min_version and version < max_version }}".to_string(),
            )
            .unwrap(),
        );
        let mut vars = value.used_variables();
        vars.sort();
        assert_eq!(vars, vec!["max_version", "min_version", "version"]);
    }

    #[test]
    fn test_template_with_attribute_access() {
        let value: Value<String> =
            Value::Template(JinjaTemplate::new("${{ build.number }}".to_string()).unwrap());
        let vars = value.used_variables();
        assert_eq!(vars, vec!["build"]);
    }

    #[test]
    fn test_template_with_list() {
        let value: Value<String> =
            Value::Template(JinjaTemplate::new("${{ [a, b, c] }}".to_string()).unwrap());
        let mut vars = value.used_variables();
        vars.sort();
        assert_eq!(vars, vec!["a", "b", "c"]);
    }
}
