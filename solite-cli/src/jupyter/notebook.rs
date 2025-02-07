//! Source: https://github.com/astral-sh/ruff/blob/113e259e6d26754bd42c4327b75eb2395387e37a/crates/ruff_notebook/src/schema.rs#L89

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;

fn sort_alphabetically<T: Serialize, S: serde::Serializer>(
    value: &T,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let value = serde_json::to_value(value).map_err(serde::ser::Error::custom)?;
    value.serialize(serializer)
}

/// This is used to serialize any value implementing [`Serialize`] alphabetically.
///
/// The reason for this is to maintain consistency in the generated JSON string,
/// which is useful for diffing. The default serializer keeps the order of the
/// fields as they are defined in the struct, which will not be consistent when
/// there are `extra` fields.
///
/// # Example
///
/// ```
/// use std::collections::BTreeMap;
///
/// use serde::Serialize;
///
/// use ruff_notebook::SortAlphabetically;
///
/// #[derive(Serialize)]
/// struct MyStruct {
///    a: String,
///    #[serde(flatten)]
///    extra: BTreeMap<String, String>,
///    b: String,
/// }
///
/// let my_struct = MyStruct {
///     a: "a".to_string(),
///     extra: BTreeMap::from([
///         ("d".to_string(), "d".to_string()),
///         ("c".to_string(), "c".to_string()),
///     ]),
///     b: "b".to_string(),
/// };
///
/// let serialized = serde_json::to_string_pretty(&SortAlphabetically(&my_struct)).unwrap();
/// assert_eq!(
///     serialized,
/// r#"{
///   "a": "a",
///   "b": "b",
///   "c": "c",
///   "d": "d"
/// }"#
/// );
/// ```
#[derive(Serialize)]
pub struct SortAlphabetically<T: Serialize>(#[serde(serialize_with = "sort_alphabetically")] pub T);

/// The root of the JSON of a Jupyter Notebook
///
/// Generated by <https://app.quicktype.io/> from
/// <https://github.com/jupyter/nbformat/blob/16b53251aabf472ad9406ddb1f78b0421c014eeb/nbformat/v4/nbformat.v4.schema.json>
/// Jupyter Notebook v4.5 JSON schema.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RawNotebook {
    /// Array of cells of the current notebook.
    pub cells: Vec<Cell>,
    /// Notebook root-level metadata.
    pub metadata: RawNotebookMetadata,
    /// Notebook format (major number). Incremented between backwards incompatible changes to the
    /// notebook format.
    pub nbformat: i64,
    /// Notebook format (minor number). Incremented for backward compatible changes to the
    /// notebook format.
    pub nbformat_minor: i64,
}

/// String identifying the type of cell.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "cell_type")]
pub enum Cell {
    #[serde(rename = "code")]
    Code(CodeCell),
    #[serde(rename = "markdown")]
    Markdown(MarkdownCell),
    #[serde(rename = "raw")]
    Raw(RawCell),
}

/// Notebook raw nbconvert cell.
#[skip_serializing_none]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RawCell {
    pub attachments: Option<Value>,
    /// Technically, id isn't required (it's not even present) in schema v4.0 through v4.4, but
    /// it's required in v4.5. Main issue is that pycharm creates notebooks without an id
    /// <https://youtrack.jetbrains.com/issue/PY-59438/Jupyter-notebooks-created-with-PyCharm-are-missing-the-id-field-in-cells-in-the-.ipynb-json>
    pub id: Option<String>,
    /// Cell-level metadata.
    pub metadata: Value,
    pub source: SourceValue,
}

/// Notebook markdown cell.
#[skip_serializing_none]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MarkdownCell {
    pub attachments: Option<Value>,
    /// Technically, id isn't required (it's not even present) in schema v4.0 through v4.4, but
    /// it's required in v4.5. Main issue is that pycharm creates notebooks without an id
    /// <https://youtrack.jetbrains.com/issue/PY-59438/Jupyter-notebooks-created-with-PyCharm-are-missing-the-id-field-in-cells-in-the-.ipynb-json>
    pub id: Option<String>,
    /// Cell-level metadata.
    pub metadata: Value,
    pub source: SourceValue,
}

/// Notebook code cell.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CodeCell {
    /// The code cell's prompt number. Will be null if the cell has not been run.
    pub execution_count: Option<i64>,
    /// Technically, id isn't required (it's not even present) in schema v4.0 through v4.4, but
    /// it's required in v4.5. Main issue is that pycharm creates notebooks without an id
    /// <https://youtrack.jetbrains.com/issue/PY-59438/Jupyter-notebooks-created-with-PyCharm-are-missing-the-id-field-in-cells-in-the-.ipynb-json>
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Cell-level metadata.
    pub metadata: Value,
    /// Execution, display, or stream outputs.
    pub outputs: Vec<Value>,
    pub source: SourceValue,
}

/// Notebook root-level metadata.
#[skip_serializing_none]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RawNotebookMetadata {
    /// The author(s) of the notebook document
    pub authors: Option<Value>,
    /// Kernel information.
    pub kernelspec: Option<Value>,
    /// Kernel information.
    pub language_info: Option<LanguageInfo>,
    /// Original notebook format (major number) before converting the notebook between versions.
    /// This should never be written to a file.
    pub orig_nbformat: Option<i64>,
    /// The title of the notebook document
    pub title: Option<String>,
    /// For additional properties.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Kernel information.
#[skip_serializing_none]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LanguageInfo {
    /// The codemirror mode to use for code in this language.
    pub codemirror_mode: Option<Value>,
    /// The file extension for files in this language.
    pub file_extension: Option<String>,
    /// The mimetype corresponding to files in this language.
    pub mimetype: Option<String>,
    /// The programming language which this kernel runs.
    pub name: String,
    /// The pygments lexer to use for code in this language.
    pub pygments_lexer: Option<String>,
    /// For additional properties.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// mimetype output (e.g. text/plain), represented as either an array of strings or a
/// string.
///
/// Contents of the cell, represented as an array of lines.
///
/// The stream's text output, represented as an array of strings.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SourceValue {
    String(String),
    StringArray(Vec<String>),
}
