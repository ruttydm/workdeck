//! Bounded, lossless planning documents.
//!
//! Reads retain the original bytes. Edits use a lossless YAML syntax tree and
//! are accepted only after the rendered document parses to the intended value.
//! Mapping keys must be strings. Anchors, aliases, merge keys, explicit tags,
//! directives, and multiple YAML documents are intentionally unsupported.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde_yaml_ng::{Mapping, Value};
use yaml_edit::{SyntaxKind, YamlFile};

/// Maximum encoded size of a complete planning document, including its body.
pub const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
/// Maximum nested collection depth admitted to the lossless syntax tree.
pub const MAX_DOCUMENT_DEPTH: usize = 64;
const MAX_YAML_TOKENS: usize = 262_144;

/// A document error with one-based coordinates in the original file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{path}:{line}:{column}: {message}")]
pub struct DocumentError {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl DocumentError {
    fn at(path: &Path, line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            path: path.to_owned(),
            line,
            column,
            message: message.into(),
        }
    }

    fn offset(
        path: &Path,
        source: &str,
        offset: usize,
        line_offset: usize,
        message: impl Into<String>,
    ) -> Self {
        let mut offset = offset.min(source.len());
        while !source.is_char_boundary(offset) {
            offset -= 1;
        }
        let prefix = &source[..offset];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1 + line_offset;
        let column = prefix
            .rsplit('\n')
            .next()
            .unwrap_or_default()
            .chars()
            .count()
            + 1;
        Self::at(path, line, column, message)
    }

    fn yaml(path: &Path, error: serde_yaml_ng::Error, line_offset: usize) -> Self {
        let (line, column) = error
            .location()
            .map_or((1, 1), |location| (location.line(), location.column()));
        Self::at(path, line + line_offset, column, error.to_string())
    }
}

/// One YAML mapping with original presentation retained between edits.
#[derive(Debug, Clone)]
pub struct YamlDocument {
    path: PathBuf,
    source: String,
    metadata: Mapping,
    line_offset: usize,
}

impl YamlDocument {
    pub fn parse(path: &Path, source: &str) -> Result<Self, DocumentError> {
        Self::parse_at(path, source, 0)
    }

    fn parse_at(path: &Path, source: &str, line_offset: usize) -> Result<Self, DocumentError> {
        validate_text(path, source, line_offset)?;
        preflight_yaml(path, source, line_offset)?;
        let value: Value = serde_yaml_ng::from_str(source)
            .map_err(|error| DocumentError::yaml(path, error, line_offset))?;
        validate_value(path, &value, 0, line_offset)?;
        let Value::Mapping(metadata) = value else {
            return Err(DocumentError::at(
                path,
                line_offset + 1,
                1,
                "YAML document must contain a mapping",
            ));
        };
        let tree = parse_tree(path, source, line_offset)?;
        if tree.to_string() != source {
            return Err(DocumentError::at(
                path,
                line_offset + 1,
                1,
                "YAML syntax cannot be preserved losslessly",
            ));
        }
        Ok(Self {
            path: path.to_owned(),
            source: source.to_owned(),
            metadata,
            line_offset,
        })
    }

    pub fn metadata(&self) -> &Mapping {
        &self.metadata
    }

    pub fn render(&self) -> String {
        self.source.clone()
    }

    pub fn deserialize<T: DeserializeOwned>(&self) -> Result<T, DocumentError> {
        serde_yaml_ng::from_str(&self.source)
            .map_err(|error| DocumentError::yaml(&self.path, error, self.line_offset))
    }

    /// Replace only named top-level values. Null removes a key.
    ///
    /// This method is transactional in memory: any failure leaves the original
    /// document intact. Unchanged values are skipped, preserving their quoting.
    pub fn patch(&mut self, changes: &Mapping) -> Result<(), DocumentError> {
        validate_mapping(&self.path, changes, 0, self.line_offset)?;
        let mut expected = self.metadata.clone();
        let mut source = self.source.clone();
        let newline = newline_style(&source);
        for (key, value) in changes {
            if (value.is_null() && !expected.contains_key(key))
                || (!value.is_null() && expected.get(key) == Some(value))
            {
                continue;
            }
            let tree = parse_tree(&self.path, &source, self.line_offset)?;
            let mapping = tree
                .document()
                .and_then(|document| document.as_mapping())
                .ok_or_else(|| {
                    DocumentError::at(
                        &self.path,
                        self.line_offset + 1,
                        1,
                        "YAML document has no editable mapping",
                    )
                })?;
            let key_text = key.as_str().expect("string keys validated above");
            if value.is_null() {
                mapping.remove(key_text);
                expected.remove(key);
            } else {
                // Insert canonical flow nodes, including JSON-quoted strings.
                // yaml-edit 0.3.1 can emit invalid YAML when a block collection
                // is transplanted into a flow mapping. Wrapping also avoids
                // its root block-scalar document-boundary ambiguity.
                let serialized = format!("{}: {}\n", flow_value(key), flow_value(value));
                validate_text(&self.path, &serialized, self.line_offset)?;
                let value_tree = parse_tree(&self.path, &serialized, self.line_offset)?;
                let replacement_mapping = value_tree
                    .document()
                    .and_then(|document| document.as_mapping())
                    .ok_or_else(|| {
                        DocumentError::at(
                            &self.path,
                            self.line_offset + 1,
                            1,
                            "replacement value has unsupported YAML syntax",
                        )
                    })?;
                let replacement_key = replacement_mapping.keys().next().ok_or_else(|| {
                    DocumentError::at(
                        &self.path,
                        self.line_offset + 1,
                        1,
                        "replacement key has unsupported YAML syntax",
                    )
                })?;
                let replacement_value = replacement_mapping.values().next().ok_or_else(|| {
                    DocumentError::at(
                        &self.path,
                        self.line_offset + 1,
                        1,
                        "replacement value has unsupported YAML syntax",
                    )
                })?;
                mapping.set(replacement_key, replacement_value);
                expected.insert(key.clone(), value.clone());
            }
            let mut rendered = tree.to_string();
            if expected.is_empty() && !mapping.is_flow_style() {
                // yaml-edit retains comments but leaves a drained block map
                // implicit (YAML null). Insert its explicit empty spelling at
                // the CST boundary without dropping those retained comments.
                rendered.insert_str(mapping.byte_range().start as usize, "{}\n");
            }
            source = normalize_changed_newlines(&source, &rendered, newline);
        }
        if source == self.source {
            return Ok(());
        }
        let candidate = Self::parse_at(&self.path, &source, self.line_offset)?;
        if candidate.metadata != expected {
            return Err(DocumentError::at(
                &self.path,
                self.line_offset + 1,
                1,
                "lossless edit changed values beyond the requested patch; original document retained",
            ));
        }
        *self = candidate;
        Ok(())
    }
}

/// A Markdown body preceded by exactly one YAML frontmatter mapping.
#[derive(Debug, Clone)]
pub struct MarkdownDocument {
    path: PathBuf,
    opening: String,
    yaml: YamlDocument,
    closing: String,
    body: String,
}

impl MarkdownDocument {
    pub fn parse(path: &Path, source: &str) -> Result<Self, DocumentError> {
        validate_text(path, source, 0)?;
        let opening = if source.starts_with("---\r\n") {
            "---\r\n"
        } else if source.starts_with("---\n") {
            "---\n"
        } else {
            return Err(DocumentError::at(
                path,
                1,
                1,
                "Markdown must start with a YAML frontmatter delimiter (---) on its own line",
            ));
        };
        let mut offset = opening.len();
        for line in source[offset..].split_inclusive('\n') {
            if line.trim_end_matches(['\r', '\n']) == "---" {
                let yaml = YamlDocument::parse_at(path, &source[opening.len()..offset], 1)?;
                return Ok(Self {
                    path: path.to_owned(),
                    opening: opening.into(),
                    yaml,
                    closing: line.into(),
                    body: source[offset + line.len()..].into(),
                });
            }
            offset += line.len();
        }
        Err(DocumentError::offset(
            path,
            source,
            source.len(),
            0,
            "missing closing YAML frontmatter delimiter (---)",
        ))
    }

    pub fn metadata(&self) -> &Mapping {
        self.yaml.metadata()
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn render(&self) -> String {
        format!(
            "{}{}{}{}",
            self.opening, self.yaml.source, self.closing, self.body
        )
    }

    pub fn deserialize<T: DeserializeOwned>(&self) -> Result<T, DocumentError> {
        self.yaml.deserialize()
    }

    pub fn patch(&mut self, changes: &Mapping) -> Result<(), DocumentError> {
        let mut candidate = self.clone();
        candidate.yaml.patch(changes)?;
        if !candidate.yaml.source.ends_with('\n') {
            candidate
                .yaml
                .source
                .push_str(newline_style(&candidate.opening));
        }
        // Validate the envelope too: a formatter must never join a closing
        // fence to its preceding scalar or turn replacement text into body.
        *self = Self::parse(&self.path, &candidate.render())?;
        Ok(())
    }

    pub fn set_body(&mut self, body: String) -> Result<(), DocumentError> {
        let mut candidate = self.clone();
        if !body.is_empty() && !candidate.closing.ends_with('\n') {
            candidate
                .closing
                .push_str(newline_style(&candidate.opening));
        }
        candidate.body = body;
        validate_text(&self.path, &candidate.render(), 0)?;
        *self = candidate;
        Ok(())
    }
}

fn validate_text(path: &Path, source: &str, line_offset: usize) -> Result<(), DocumentError> {
    if source.len() > MAX_DOCUMENT_BYTES {
        return Err(DocumentError::at(
            path,
            line_offset + 1,
            1,
            format!("document exceeds the {MAX_DOCUMENT_BYTES}-byte limit"),
        ));
    }
    for (line, text) in source.lines().enumerate() {
        if ["<<<<<<<", "|||||||", ">>>>>>>"].iter().any(|marker| {
            text == *marker
                || text
                    .strip_prefix(marker)
                    .is_some_and(|rest| rest.starts_with(' '))
        }) || text == "======="
        {
            return Err(DocumentError::at(
                path,
                line + line_offset + 1,
                1,
                "unresolved Git conflict marker",
            ));
        }
    }
    Ok(())
}

fn preflight_yaml(path: &Path, source: &str, line_offset: usize) -> Result<(), DocumentError> {
    let tokens = yaml_edit::lex(source);
    if tokens.len() > MAX_YAML_TOKENS {
        return Err(DocumentError::at(
            path,
            line_offset + 1,
            1,
            "YAML token count exceeds the document limit",
        ));
    }
    let mut flow_depth = 0usize;
    let mut indents = vec![0usize];
    let mut offset = 0usize;
    let mut line_start = 0usize;
    let mut line_indent = 0usize;
    let mut at_line_start = true;
    let mut block_scalar_indent = None;
    let mut block_scalar_line = false;
    let mut inline_collections = 0usize;
    for (kind, token) in tokens {
        let token_offset = offset;
        offset += token.len();
        if kind == SyntaxKind::NEWLINE {
            line_start = offset;
            line_indent = 0;
            at_line_start = true;
            block_scalar_line = false;
            inline_collections = 0;
            continue;
        }
        if at_line_start && matches!(kind, SyntaxKind::INDENT | SyntaxKind::WHITESPACE) {
            line_indent += token.len();
            continue;
        }
        if at_line_start {
            at_line_start = false;
            if let Some(base) = block_scalar_indent {
                if line_indent > base {
                    block_scalar_line = true;
                } else {
                    block_scalar_indent = None;
                }
            }
            if !block_scalar_line && kind != SyntaxKind::COMMENT {
                while indents.last().is_some_and(|indent| *indent > line_indent) {
                    indents.pop();
                }
                if indents.last().is_none_or(|indent| *indent < line_indent) {
                    indents.push(line_indent);
                }
            }
        }
        if block_scalar_line {
            continue;
        }
        match kind {
            SyntaxKind::PIPE | SyntaxKind::GREATER => {
                block_scalar_indent = Some(line_indent);
            }
            SyntaxKind::LEFT_BRACKET | SyntaxKind::LEFT_BRACE => {
                flow_depth += 1;
            }
            SyntaxKind::RIGHT_BRACKET | SyntaxKind::RIGHT_BRACE => {
                flow_depth = flow_depth.saturating_sub(1);
            }
            SyntaxKind::DASH => {
                inline_collections += 1;
            }
            SyntaxKind::ANCHOR
            | SyntaxKind::REFERENCE
            | SyntaxKind::TAG
            | SyntaxKind::MERGE_KEY
            | SyntaxKind::DIRECTIVE => {
                return Err(DocumentError::offset(
                    path,
                    source,
                    token_offset,
                    line_offset,
                    "YAML anchors, aliases, tags, merge keys, and directives are unsupported",
                ));
            }
            _ => {}
        }
        if flow_depth + indents.len() + inline_collections > MAX_DOCUMENT_DEPTH {
            return Err(DocumentError::offset(
                path,
                source,
                line_start,
                line_offset,
                format!("YAML nesting depth exceeds the {MAX_DOCUMENT_DEPTH}-level limit"),
            ));
        }
    }
    Ok(())
}

fn validate_value(
    path: &Path,
    value: &Value,
    depth: usize,
    line_offset: usize,
) -> Result<(), DocumentError> {
    if depth > MAX_DOCUMENT_DEPTH {
        return Err(DocumentError::at(
            path,
            line_offset + 1,
            1,
            "YAML nesting depth exceeds the document limit",
        ));
    }
    match value {
        Value::Mapping(mapping) => validate_mapping(path, mapping, depth, line_offset)?,
        Value::Sequence(sequence) => {
            for child in sequence {
                validate_value(path, child, depth + 1, line_offset)?;
            }
        }
        Value::Tagged(_) => {
            return Err(DocumentError::at(
                path,
                line_offset + 1,
                1,
                "explicit YAML tags are unsupported",
            ));
        }
        Value::String(value) if value.len() > MAX_DOCUMENT_BYTES => {
            return Err(DocumentError::at(
                path,
                line_offset + 1,
                1,
                "YAML string exceeds the document limit",
            ));
        }
        _ => {}
    }
    Ok(())
}

fn validate_mapping(
    path: &Path,
    mapping: &Mapping,
    depth: usize,
    line_offset: usize,
) -> Result<(), DocumentError> {
    for (key, child) in mapping {
        if key.as_str().is_none_or(|key| key == "<<") {
            return Err(DocumentError::at(
                path,
                line_offset + 1,
                1,
                "YAML mapping keys must be strings and merge keys are unsupported",
            ));
        }
        validate_value(path, key, depth + 1, line_offset)?;
        validate_value(path, child, depth + 1, line_offset)?;
    }
    Ok(())
}

fn parse_tree(path: &Path, source: &str, line_offset: usize) -> Result<YamlFile, DocumentError> {
    let parsed = YamlFile::parse(source);
    if let Some(error) = parsed.positioned_errors().first() {
        return Err(DocumentError::offset(
            path,
            source,
            error.range.start as usize,
            line_offset,
            &error.message,
        ));
    }
    let tree = parsed.tree();
    if tree.documents().count() != 1 {
        return Err(DocumentError::at(
            path,
            line_offset + 1,
            1,
            "exactly one YAML document is required",
        ));
    }
    Ok(tree)
}

fn newline_style(source: &str) -> &'static str {
    if source
        .find('\n')
        .is_some_and(|index| index > 0 && source.as_bytes()[index - 1] == b'\r')
    {
        "\r\n"
    } else {
        "\n"
    }
}

fn flow_value(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => serde_yaml_ng::to_string(value)
            .expect("numbers serialize to YAML")
            .trim_end()
            .into(),
        Value::String(value) => serde_json::to_string(value)
            .expect("strings serialize to JSON")
            .chars()
            .map(|character| match character {
                '\u{7f}'..='\u{9f}' | '\u{2028}' | '\u{2029}' | '\u{fffe}' | '\u{ffff}' => {
                    format!("\\u{:04X}", u32::from(character))
                }
                _ => character.to_string(),
            })
            .collect(),
        Value::Sequence(values) => format!(
            "[{}]",
            values.iter().map(flow_value).collect::<Vec<_>>().join(", ")
        ),
        Value::Mapping(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!("{}: {}", flow_value(key), flow_value(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Tagged(_) => unreachable!("tags are rejected before serialization"),
    }
}

/// Normalize only the replacement span, preserving untouched mixed-newline text.
fn normalize_changed_newlines(before: &str, after: &str, newline: &str) -> String {
    if newline == "\n" {
        return after.to_owned();
    }
    let mut prefix = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !after.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let mut suffix = before[prefix..]
        .bytes()
        .rev()
        .zip(after[prefix..].bytes().rev())
        .take_while(|(a, b)| a == b)
        .count();
    while !after.is_char_boundary(after.len() - suffix) {
        suffix -= 1;
    }
    let changed_end = after.len() - suffix;
    let mut output = String::with_capacity(after.len());
    output.push_str(&after[..prefix]);
    for (index, character) in after[prefix..changed_end].char_indices() {
        if character == '\n'
            && (prefix + index == 0 || after.as_bytes()[prefix + index - 1] != b'\r')
        {
            output.push('\r');
        }
        output.push(character);
    }
    output.push_str(&after[changed_end..]);
    output
}
