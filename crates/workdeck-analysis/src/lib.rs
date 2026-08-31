use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use tree_sitter::{Language, Node, Parser, Point, Query, QueryCursor, StreamingIterator, Tree};
use workdeck_domain::{AnalysisConfidence, RepositoryId, ReviewUnitKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLanguage {
    Rust,
    Swift,
    JavaScript,
    TypeScript,
    Tsx,
    Python,
    Go,
    Php,
    Bash,
    C,
    Cpp,
    CSharp,
    Css,
    Html,
    Java,
    Json,
    Ruby,
    Toml,
    Yaml,
    Markdown,
    Text,
}

impl SourceLanguage {
    pub fn from_path(path: &Path) -> Self {
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if matches!(filename.as_str(), "dockerfile" | "containerfile") {
            return Self::Bash;
        }
        if matches!(filename.as_str(), "gemfile" | "rakefile") {
            return Self::Ruby;
        }
        match path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "rs" => Self::Rust,
            "swift" => Self::Swift,
            "js" | "mjs" | "cjs" | "jsx" => Self::JavaScript,
            "ts" | "mts" | "cts" => Self::TypeScript,
            "tsx" => Self::Tsx,
            "py" | "pyi" => Self::Python,
            "go" => Self::Go,
            "php" => Self::Php,
            "sh" | "bash" | "zsh" => Self::Bash,
            "c" | "h" => Self::C,
            "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => Self::Cpp,
            "cs" => Self::CSharp,
            "css" => Self::Css,
            "html" | "htm" | "vue" => Self::Html,
            "java" => Self::Java,
            "json" | "jsonc" => Self::Json,
            "rb" | "rake" => Self::Ruby,
            "toml" => Self::Toml,
            "yaml" | "yml" => Self::Yaml,
            "md" | "mdx" | "markdown" => Self::Markdown,
            _ => Self::Text,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReviewDocument<'a> {
    pub repository_id: Option<RepositoryId>,
    pub path: PathBuf,
    pub content: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzedUnit {
    pub logical_key: String,
    pub repository_id: Option<RepositoryId>,
    pub path: PathBuf,
    pub qualified_name: Option<String>,
    pub kind: ReviewUnitKind,
    pub title: String,
    pub content: String,
    pub semantic_content: String,
    pub start_byte: u64,
    pub end_byte: u64,
    pub start_line: u32,
    pub end_line: u32,
    pub provenance: String,
    pub confidence: AnalysisConfidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolNode {
    pub qualified_name: String,
    pub kind: String,
    pub start_line: u32,
    pub end_line: u32,
    pub children: Vec<SymbolNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallEdge {
    pub caller: String,
    pub callee: String,
    pub line: u32,
    pub confidence: AnalysisConfidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalNode {
    pub kind: String,
    pub field: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub error: bool,
    pub children: Vec<CanonicalNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub language: SourceLanguage,
    pub units: Vec<AnalyzedUnit>,
    pub symbols: Vec<SymbolNode>,
    pub calls: Vec<CallEdge>,
    pub canonical_tree: Option<CanonicalNode>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightSpan {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub token: String,
}

/// Produces renderer-independent syntax spans with Tree-sitter. The spans are
/// byte based, non-overlapping, and safe to apply to the original UTF-8 source.
/// Markdown is handled separately because it intentionally has no executable
/// grammar in Workdeck's analysis pipeline.
pub fn highlight(path: &Path, source: &str) -> Result<Vec<HighlightSpan>> {
    let language = SourceLanguage::from_path(path);
    if language == SourceLanguage::Text {
        return Ok(Vec::new());
    }
    if language == SourceLanguage::Markdown {
        return Ok(highlight_markdown(source));
    }
    let mut parser = Parser::new();
    let language = grammar(language)?;
    parser
        .set_language(&language)
        .context("failed to install Tree-sitter highlight grammar")?;
    let tree = parser
        .parse(source, None)
        .context("Tree-sitter returned no highlight tree")?;
    let query = cached_highlight_query(language, SourceLanguage::from_path(path))?;
    Ok(query_highlights(&query, tree.root_node(), source))
}

#[derive(Debug, Clone)]
struct RawHighlight {
    line: usize,
    start: usize,
    end: usize,
    token: &'static str,
    priority: u8,
}

static HIGHLIGHT_QUERY_CACHE: OnceLock<Mutex<HashMap<SourceLanguage, Arc<Query>>>> =
    OnceLock::new();

fn cached_highlight_query(
    language: Language,
    source_language: SourceLanguage,
) -> Result<Arc<Query>> {
    let cache = HIGHLIGHT_QUERY_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(query) = cache
        .lock()
        .expect("highlight query cache")
        .get(&source_language)
    {
        return Ok(query.clone());
    }
    let source = highlight_query_source(source_language);
    let query = Arc::new(
        Query::new(&language, &source)
            .with_context(|| format!("invalid {source_language:?} highlight query"))?,
    );
    cache
        .lock()
        .expect("highlight query cache")
        .insert(source_language, query.clone());
    Ok(query)
}

fn highlight_query_source(language: SourceLanguage) -> Cow<'static, str> {
    match language {
        SourceLanguage::Rust => tree_sitter_rust::HIGHLIGHTS_QUERY.into(),
        SourceLanguage::Swift => tree_sitter_swift::HIGHLIGHTS_QUERY.into(),
        SourceLanguage::JavaScript => format!(
            "{}\n{}",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
        )
        .into(),
        SourceLanguage::TypeScript => format!(
            "{}\n{}",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY
        )
        .into(),
        SourceLanguage::Tsx => format!(
            "{}\n{}\n{}",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY
        )
        .into(),
        SourceLanguage::Python => tree_sitter_python::HIGHLIGHTS_QUERY.into(),
        SourceLanguage::Go => tree_sitter_go::HIGHLIGHTS_QUERY.into(),
        SourceLanguage::Php => tree_sitter_php::HIGHLIGHTS_QUERY.into(),
        SourceLanguage::Bash => tree_sitter_bash::HIGHLIGHT_QUERY.into(),
        SourceLanguage::C => tree_sitter_c::HIGHLIGHT_QUERY.into(),
        SourceLanguage::Cpp => format!(
            "{}\n{}",
            tree_sitter_c::HIGHLIGHT_QUERY,
            tree_sitter_cpp::HIGHLIGHT_QUERY
        )
        .into(),
        SourceLanguage::CSharp => tree_sitter_c_sharp::HIGHLIGHTS_QUERY.into(),
        SourceLanguage::Css => tree_sitter_css::HIGHLIGHTS_QUERY.into(),
        SourceLanguage::Html => tree_sitter_html::HIGHLIGHTS_QUERY.into(),
        SourceLanguage::Java => tree_sitter_java::HIGHLIGHTS_QUERY.into(),
        SourceLanguage::Json => tree_sitter_json::HIGHLIGHTS_QUERY.into(),
        SourceLanguage::Ruby => tree_sitter_ruby::HIGHLIGHTS_QUERY.into(),
        SourceLanguage::Toml => tree_sitter_toml_ng::HIGHLIGHTS_QUERY.into(),
        SourceLanguage::Yaml => tree_sitter_yaml::HIGHLIGHTS_QUERY.into(),
        SourceLanguage::Markdown | SourceLanguage::Text => Cow::Borrowed(""),
    }
}

fn query_highlights(query: &Query, root: Node<'_>, source: &str) -> Vec<HighlightSpan> {
    let line_lengths = source.split('\n').map(str::len).collect::<Vec<_>>();
    let capture_names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut captures = cursor.captures(query, root, source.as_bytes());
    let mut raw = Vec::new();
    while let Some((query_match, capture_index)) = captures.next() {
        let capture = query_match.captures[*capture_index];
        let Some(name) = capture_names.get(capture.index as usize) else {
            continue;
        };
        let Some((mut token, priority)) = capture_token(name) else {
            continue;
        };
        let node_kind = capture.node.kind().to_ascii_lowercase();
        if node_kind.contains("integer")
            || node_kind.contains("float")
            || node_kind.contains("number")
        {
            token = "number";
        } else if node_kind.contains("boolean") {
            token = "constant";
        }
        push_node_highlights(capture.node, token, priority, &line_lengths, &mut raw);
    }
    normalize_highlights(raw)
}

fn push_node_highlights(
    node: Node<'_>,
    token: &'static str,
    priority: u8,
    line_lengths: &[usize],
    output: &mut Vec<RawHighlight>,
) {
    let start = node.start_position();
    let end = node.end_position();
    for line in start.row..=end.row {
        let Some(line_length) = line_lengths.get(line).copied() else {
            continue;
        };
        let segment_start = if line == start.row { start.column } else { 0 }.min(line_length);
        let segment_end = if line == end.row {
            end.column
        } else {
            line_length
        }
        .min(line_length);
        if segment_start < segment_end {
            output.push(RawHighlight {
                line,
                start: segment_start,
                end: segment_end,
                token,
                priority,
            });
        }
    }
}

fn normalize_highlights(raw: Vec<RawHighlight>) -> Vec<HighlightSpan> {
    let mut lines = BTreeMap::<usize, Vec<RawHighlight>>::new();
    for span in raw {
        lines.entry(span.line).or_default().push(span);
    }
    let mut output: Vec<HighlightSpan> = Vec::new();
    for (line, spans) in lines {
        let mut boundaries = spans
            .iter()
            .flat_map(|span| [span.start, span.end])
            .collect::<Vec<_>>();
        boundaries.sort_unstable();
        boundaries.dedup();
        for pair in boundaries.windows(2) {
            let (start, end) = (pair[0], pair[1]);
            let winner = spans
                .iter()
                .filter(|span| span.start <= start && span.end >= end)
                .max_by(|left, right| {
                    left.priority
                        .cmp(&right.priority)
                        .then_with(|| (right.end - right.start).cmp(&(left.end - left.start)))
                });
            let Some(winner) = winner else { continue };
            if let Some(previous) = output.last_mut()
                && previous.start_line == line
                && previous.end_column == start
                && previous.token == winner.token
            {
                previous.end_column = end;
            } else {
                output.push(HighlightSpan {
                    start_line: line,
                    start_column: start,
                    end_line: line,
                    end_column: end,
                    token: winner.token.into(),
                });
            }
        }
    }
    output
}

fn capture_token(name: &str) -> Option<(&'static str, u8)> {
    let name = name.to_ascii_lowercase();
    let token = if name.starts_with("comment") {
        ("comment", 100)
    } else if name.contains("escape") || name.starts_with("character.special") {
        ("escape", 110)
    } else if name.starts_with("string") {
        ("string", 100)
    } else if name.starts_with("function.macro")
        || name.starts_with("macro")
        || name.starts_with("constant.macro")
    {
        ("macro", 92)
    } else if name.starts_with("function.builtin") {
        ("builtin", 90)
    } else if name.starts_with("function") || name.starts_with("method") {
        ("function", 88)
    } else if name.starts_with("constructor") || name.starts_with("type") {
        ("type", 86)
    } else if name.starts_with("property")
        || name.starts_with("field")
        || name.starts_with("variable.member")
    {
        ("property", 84)
    } else if name.starts_with("variable.parameter") || name.starts_with("parameter") {
        ("parameter", 82)
    } else if name.starts_with("variable.builtin") {
        ("builtin", 82)
    } else if name.starts_with("constant.builtin") || name.starts_with("module.builtin") {
        ("builtin", 80)
    } else if name.starts_with("number") || name.contains("numeric") || name.starts_with("float") {
        ("number", 80)
    } else if name.starts_with("boolean") {
        ("constant", 80)
    } else if name.starts_with("constant") {
        ("constant", 79)
    } else if name.starts_with("keyword")
        || name.starts_with("conditional")
        || name.starts_with("repeat")
        || matches!(
            name.as_str(),
            "charset" | "import" | "keyframes" | "media" | "supports"
        )
    {
        ("keyword", 76)
    } else if name.starts_with("attribute") || name.starts_with("decorator") {
        ("attribute", 74)
    } else if name.starts_with("tag") {
        ("tag", 72)
    } else if name.starts_with("namespace") || name.starts_with("module") {
        ("namespace", 70)
    } else if name.starts_with("label") {
        ("label", 68)
    } else if name.starts_with("embedded") {
        ("embedded", 66)
    } else if name.starts_with("variable") {
        ("variable", 60)
    } else if name.starts_with("operator") {
        ("operator", 50)
    } else if name.starts_with("punctuation") || name.starts_with("delimiter") {
        ("punctuation", 40)
    } else {
        return None;
    };
    Some(token)
}

fn highlight_markdown(source: &str) -> Vec<HighlightSpan> {
    let mut spans = Vec::new();
    for (line, text) in source.lines().enumerate() {
        let indentation = text.len().saturating_sub(text.trim_start().len());
        let trimmed = text.trim_start();
        if trimmed.starts_with('#') {
            spans.push(HighlightSpan {
                start_line: line,
                start_column: indentation,
                end_line: line,
                end_column: text.len(),
                token: "heading".into(),
            });
        } else if trimmed.starts_with('>') {
            spans.push(HighlightSpan {
                start_line: line,
                start_column: indentation,
                end_line: line,
                end_column: text.len(),
                token: "comment".into(),
            });
        }
        let mut offset = 0;
        while let Some(start) = text[offset..].find('`') {
            let start = offset + start;
            let Some(end) = text[start + 1..].find('`') else {
                break;
            };
            let end = start + end + 2;
            spans.push(HighlightSpan {
                start_line: line,
                start_column: start,
                end_line: line,
                end_column: end,
                token: "string".into(),
            });
            offset = end;
        }
    }
    spans
}

pub fn analyze(document: &ReviewDocument<'_>) -> Result<AnalysisResult> {
    let language = SourceLanguage::from_path(&document.path);
    match language {
        SourceLanguage::Markdown => Ok(analyze_markdown(document)),
        SourceLanguage::Text => Ok(analyze_text(document)),
        language => analyze_syntax(document, language),
    }
}

fn analyze_syntax(
    document: &ReviewDocument<'_>,
    language: SourceLanguage,
) -> Result<AnalysisResult> {
    let grammar = grammar(language)?;
    let mut parser = Parser::new();
    parser
        .set_language(&grammar)
        .context("failed to install Tree-sitter grammar")?;
    let tree = parser
        .parse(document.content, None)
        .context("Tree-sitter returned no syntax tree")?;
    let mut units = Vec::new();
    let mut symbols = Vec::new();
    let mut calls = Vec::new();
    collect_syntax(
        document,
        &tree,
        tree.root_node(),
        &mut Vec::new(),
        &mut units,
        &mut symbols,
        &mut calls,
    );
    if units.is_empty() {
        units.push(file_unit(
            document,
            semantic_tree_content(&tree, document.content),
        ));
    }
    let diagnostics = if tree.root_node().has_error() {
        vec!["syntax tree contains parser recovery nodes".to_string()]
    } else {
        Vec::new()
    };
    Ok(AnalysisResult {
        language,
        units,
        symbols,
        calls,
        canonical_tree: Some(canonical_node(tree.root_node(), None)),
        diagnostics,
    })
}

fn collect_syntax(
    document: &ReviewDocument<'_>,
    tree: &Tree,
    node: Node<'_>,
    parents: &mut Vec<String>,
    units: &mut Vec<AnalyzedUnit>,
    symbols: &mut Vec<SymbolNode>,
    calls: &mut Vec<CallEdge>,
) {
    let symbol = is_symbol_kind(node.kind());
    let mut pushed = false;
    if symbol {
        let local_name = node_name(node, document.content).unwrap_or_else(|| {
            format!(
                "{}@{}",
                node.kind(),
                node.start_position().row.saturating_add(1)
            )
        });
        let qualified_name = parents
            .iter()
            .chain(std::iter::once(&local_name))
            .cloned()
            .collect::<Vec<_>>()
            .join("::");
        let content = slice(document.content, node.byte_range()).to_string();
        units.push(AnalyzedUnit {
            logical_key: logical_key(document, &qualified_name, node.kind()),
            repository_id: document.repository_id.clone(),
            path: document.path.clone(),
            qualified_name: Some(qualified_name.clone()),
            kind: ReviewUnitKind::Symbol,
            title: format!("{} {qualified_name}", node.kind()),
            semantic_content: semantic_node_content(tree, node, document.content),
            content,
            start_byte: node.start_byte() as u64,
            end_byte: node.end_byte() as u64,
            start_line: line(node.start_position()),
            end_line: line(node.end_position()),
            provenance: "tree-sitter".to_string(),
            confidence: AnalysisConfidence::SyntaxInferred,
        });
        symbols.push(SymbolNode {
            qualified_name: qualified_name.clone(),
            kind: node.kind().to_string(),
            start_line: line(node.start_position()),
            end_line: line(node.end_position()),
            children: Vec::new(),
        });
        parents.push(qualified_name);
        pushed = true;
    }
    if is_call_kind(node.kind()) {
        let callee = node
            .child_by_field_name("function")
            .or_else(|| node.child_by_field_name("name"))
            .or_else(|| node.child_by_field_name("callee"))
            .or_else(|| node.named_child(0))
            .map(|child| {
                slice(document.content, child.byte_range())
                    .trim()
                    .to_string()
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "<dynamic>".to_string());
        calls.push(CallEdge {
            caller: parents
                .last()
                .cloned()
                .unwrap_or_else(|| document.path.display().to_string()),
            callee,
            line: line(node.start_position()),
            confidence: AnalysisConfidence::SyntaxInferred,
        });
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_syntax(document, tree, child, parents, units, symbols, calls);
    }
    if pushed {
        parents.pop();
    }
}

fn analyze_markdown(document: &ReviewDocument<'_>) -> AnalysisResult {
    let lines: Vec<&str> = document.content.lines().collect();
    let mut headings: Vec<(usize, usize, String)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
        if (1..=6).contains(&hashes) && line.as_bytes().get(hashes) == Some(&b' ') {
            headings.push((index, hashes, line[hashes + 1..].trim().to_string()));
        }
    }
    let mut units = Vec::new();
    for (position, (start, level, title)) in headings.iter().enumerate() {
        let end = headings[position + 1..]
            .iter()
            .find(|(_, next_level, _)| next_level <= level)
            .map(|(line, _, _)| *line)
            .unwrap_or(lines.len());
        let content = lines[*start..end].join("\n");
        let qualified_name = title.to_ascii_lowercase().replace(' ', "-");
        units.push(AnalyzedUnit {
            logical_key: logical_key(document, &qualified_name, "markdown_section"),
            repository_id: document.repository_id.clone(),
            path: document.path.clone(),
            qualified_name: Some(qualified_name),
            kind: ReviewUnitKind::MarkdownSection,
            title: title.clone(),
            semantic_content: normalize_markdown(&content),
            content,
            start_byte: byte_offset(&lines, *start) as u64,
            end_byte: byte_offset(&lines, end) as u64,
            start_line: (*start + 1) as u32,
            end_line: end.max(*start + 1) as u32,
            provenance: "markdown-outline".to_string(),
            confidence: AnalysisConfidence::Observed,
        });
    }
    if units.is_empty() {
        units.push(file_unit(document, normalize_markdown(document.content)));
    }
    AnalysisResult {
        language: SourceLanguage::Markdown,
        units,
        symbols: headings
            .into_iter()
            .map(|(line_index, level, title)| SymbolNode {
                qualified_name: title,
                kind: format!("heading_{level}"),
                start_line: (line_index + 1) as u32,
                end_line: (line_index + 1) as u32,
                children: Vec::new(),
            })
            .collect(),
        calls: Vec::new(),
        canonical_tree: None,
        diagnostics: Vec::new(),
    }
}

fn analyze_text(document: &ReviewDocument<'_>) -> AnalysisResult {
    AnalysisResult {
        language: SourceLanguage::Text,
        units: vec![file_unit(document, normalize_text(document.content))],
        symbols: Vec::new(),
        calls: Vec::new(),
        canonical_tree: None,
        diagnostics: Vec::new(),
    }
}

fn file_unit(document: &ReviewDocument<'_>, semantic_content: String) -> AnalyzedUnit {
    AnalyzedUnit {
        logical_key: logical_key(document, "<file>", "file"),
        repository_id: document.repository_id.clone(),
        path: document.path.clone(),
        qualified_name: None,
        kind: ReviewUnitKind::File,
        title: document.path.display().to_string(),
        content: document.content.to_string(),
        semantic_content,
        start_byte: 0,
        end_byte: document.content.len() as u64,
        start_line: 1,
        end_line: document.content.lines().count().max(1) as u32,
        provenance: "whole-file".to_string(),
        confidence: AnalysisConfidence::Textual,
    }
}

fn grammar(language: SourceLanguage) -> Result<Language> {
    Ok(match language {
        SourceLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
        SourceLanguage::Swift => tree_sitter_swift::LANGUAGE.into(),
        SourceLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        SourceLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        SourceLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        SourceLanguage::Python => tree_sitter_python::LANGUAGE.into(),
        SourceLanguage::Go => tree_sitter_go::LANGUAGE.into(),
        SourceLanguage::Php => tree_sitter_php::LANGUAGE_PHP.into(),
        SourceLanguage::Bash => tree_sitter_bash::LANGUAGE.into(),
        SourceLanguage::C => tree_sitter_c::LANGUAGE.into(),
        SourceLanguage::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        SourceLanguage::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        SourceLanguage::Css => tree_sitter_css::LANGUAGE.into(),
        SourceLanguage::Html => tree_sitter_html::LANGUAGE.into(),
        SourceLanguage::Java => tree_sitter_java::LANGUAGE.into(),
        SourceLanguage::Json => tree_sitter_json::LANGUAGE.into(),
        SourceLanguage::Ruby => tree_sitter_ruby::LANGUAGE.into(),
        SourceLanguage::Toml => tree_sitter_toml_ng::LANGUAGE.into(),
        SourceLanguage::Yaml => tree_sitter_yaml::LANGUAGE.into(),
        SourceLanguage::Markdown | SourceLanguage::Text => bail!("language has no syntax grammar"),
    })
}

fn is_symbol_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_item"
            | "function_declaration"
            | "function_definition"
            | "method_definition"
            | "method_declaration"
            | "class_declaration"
            | "class_definition"
            | "struct_item"
            | "enum_item"
            | "trait_item"
            | "impl_item"
            | "interface_declaration"
            | "type_alias_declaration"
            | "mod_item"
    )
}

fn is_call_kind(kind: &str) -> bool {
    matches!(
        kind,
        "call_expression" | "macro_invocation" | "await_expression" | "new_expression"
    )
}

fn node_name(node: Node<'_>, source: &str) -> Option<String> {
    node.child_by_field_name("name")
        .or_else(|| node.child_by_field_name("type"))
        .map(|child| slice(source, child.byte_range()).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn semantic_tree_content(tree: &Tree, source: &str) -> String {
    semantic_node_content(tree, tree.root_node(), source)
}

fn semantic_node_content(_tree: &Tree, root: Node<'_>, source: &str) -> String {
    let mut result = String::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind().contains("comment") {
            continue;
        }
        if node.named_child_count() == 0 {
            result.push_str(node.kind());
            result.push(':');
            result.push_str(slice(source, node.byte_range()).trim());
            result.push('|');
            continue;
        }
        let mut cursor = node.walk();
        let children: Vec<_> = node.named_children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    result
}

fn canonical_node(node: Node<'_>, field: Option<String>) -> CanonicalNode {
    let mut cursor = node.walk();
    let children = node
        .children(&mut cursor)
        .enumerate()
        .filter(|(_, child)| child.is_named())
        .map(|(index, child)| {
            let field = node.field_name_for_child(index as u32).map(str::to_string);
            canonical_node(child, field)
        })
        .collect();
    CanonicalNode {
        kind: node.kind().to_string(),
        field,
        start_line: line(node.start_position()),
        end_line: line(node.end_position()),
        error: node.is_error() || node.is_missing(),
        children,
    }
}

fn logical_key(document: &ReviewDocument<'_>, name: &str, kind: &str) -> String {
    format!(
        "{}:{}:{}:{}",
        document
            .repository_id
            .as_ref()
            .map(|id| id.as_str())
            .unwrap_or("external"),
        document.path.display(),
        kind,
        name
    )
}

fn slice(source: &str, range: std::ops::Range<usize>) -> &str {
    source.get(range).unwrap_or_default()
}

fn line(point: Point) -> u32 {
    point.row.saturating_add(1) as u32
}

fn normalize_text(content: &str) -> String {
    content.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_markdown(content: &str) -> String {
    content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn byte_offset(lines: &[&str], line: usize) -> usize {
    lines.iter().take(line).map(|value| value.len() + 1).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document<'a>(path: &str, content: &'a str) -> ReviewDocument<'a> {
        ReviewDocument {
            repository_id: Some(RepositoryId::from("repo_test")),
            path: path.into(),
            content,
        }
    }

    #[test]
    fn language_detection_is_case_insensitive_and_handles_tooling_files() {
        assert_eq!(
            SourceLanguage::from_path(Path::new("Sources/View.SWIFT")),
            SourceLanguage::Swift
        );
        assert_eq!(
            SourceLanguage::from_path(Path::new("Dockerfile")),
            SourceLanguage::Bash
        );
        assert_eq!(
            SourceLanguage::from_path(Path::new("Gemfile")),
            SourceLanguage::Ruby
        );
        assert_eq!(
            SourceLanguage::from_path(Path::new("src/types.mts")),
            SourceLanguage::TypeScript
        );
        assert_eq!(
            SourceLanguage::from_path(Path::new("components/App.vue")),
            SourceLanguage::Html
        );
    }

    #[test]
    fn rust_symbols_and_calls_are_review_units() {
        let result = analyze(&document(
            "src/lib.rs",
            "pub fn run() { helper(); }\nfn helper() {}\n",
        ))
        .unwrap();
        assert_eq!(result.language, SourceLanguage::Rust);
        assert_eq!(result.units.len(), 2);
        assert!(result.calls.iter().any(|edge| edge.callee == "helper"));
        assert!(result.canonical_tree.is_some());
    }

    #[test]
    fn markdown_is_split_into_stable_sections() {
        let result = analyze(&document(
            "PLAN.md",
            "# Goal\nShip it\n## Checks\n- [ ] test\n# Later\nWait\n",
        ))
        .unwrap();
        assert_eq!(result.units.len(), 3);
        assert_eq!(result.units[1].title, "Checks");
        assert_eq!(result.units[1].start_line, 3);
    }

    #[test]
    fn formatting_does_not_change_rust_semantics() {
        let compact = analyze(&document("src/lib.rs", "fn run(){helper();}"))
            .unwrap()
            .units
            .remove(0);
        let spaced = analyze(&document("src/lib.rs", "fn run() { helper(); }"))
            .unwrap()
            .units
            .remove(0);
        assert_eq!(compact.semantic_content, spaced.semantic_content);
        assert_ne!(compact.content, spaced.content);
    }

    #[test]
    fn canonical_tree_attributes_fields_to_parent_edges() {
        fn collect_fields<'a>(node: &'a CanonicalNode, output: &mut Vec<&'a str>) {
            if let Some(field) = node.field.as_deref() {
                output.push(field);
            }
            for child in &node.children {
                collect_fields(child, output);
            }
        }

        let result = analyze(&document(
            "src/lib.rs",
            "pub fn render(value: usize) -> usize { value + 1 }\n",
        ))
        .unwrap();
        let mut fields = Vec::new();
        collect_fields(result.canonical_tree.as_ref().unwrap(), &mut fields);
        assert!(fields.contains(&"name"), "canonical fields were {fields:?}");
        assert!(fields.contains(&"body"), "canonical fields were {fields:?}");
    }

    #[test]
    fn highlighting_is_line_local_and_classified() {
        let spans = highlight(
            Path::new("src/lib.rs"),
            "pub fn run(value: usize) { // note\n  let answer = 42;\n}\n",
        )
        .unwrap();
        assert!(spans.iter().any(|span| span.token == "keyword"));
        assert!(spans.iter().any(|span| span.token == "type"));
        assert!(spans.iter().any(|span| span.token == "comment"));
        assert!(spans.iter().any(|span| span.token == "number"), "{spans:?}");
        assert!(spans.iter().all(|span| span.start_line == span.end_line));
    }

    #[test]
    fn every_grammar_query_compiles_and_maps_its_visual_captures() {
        let languages = [
            SourceLanguage::Rust,
            SourceLanguage::Swift,
            SourceLanguage::JavaScript,
            SourceLanguage::TypeScript,
            SourceLanguage::Tsx,
            SourceLanguage::Python,
            SourceLanguage::Go,
            SourceLanguage::Php,
            SourceLanguage::Bash,
            SourceLanguage::C,
            SourceLanguage::Cpp,
            SourceLanguage::CSharp,
            SourceLanguage::Css,
            SourceLanguage::Html,
            SourceLanguage::Java,
            SourceLanguage::Json,
            SourceLanguage::Ruby,
            SourceLanguage::Toml,
            SourceLanguage::Yaml,
        ];
        for language in languages {
            let grammar = grammar(language).unwrap();
            let query = Query::new(&grammar, &highlight_query_source(language))
                .unwrap_or_else(|error| panic!("{language:?}: {error}"));
            let unmapped = query
                .capture_names()
                .iter()
                .filter(|name| capture_token(name).is_none() && **name != "spell")
                .collect::<Vec<_>>();
            assert!(unmapped.is_empty(), "{language:?}: {unmapped:?}");
        }
    }

    #[test]
    fn polyglot_highlighting_emits_semantic_tokens() {
        let examples = [
            (
                "src/lib.rs",
                "pub fn run(value: usize) -> usize { value + 1 }",
            ),
            (
                "Sources/App.swift",
                "func run(value: Int) -> Int { value + 1 }",
            ),
            (
                "src/app.tsx",
                "export const App = ({ name }: Props) => <main>{name}</main>;",
            ),
            (
                "src/app.py",
                "def run(value: int) -> int:\n    return value + 1",
            ),
            ("main.go", "func run(value int) int { return value + 1 }"),
            (
                "app/Service.php",
                "<?php function run(int $value): int { return $value + 1; }",
            ),
            ("scripts/check.sh", "function run() { printf '%s\\n' ok; }"),
            ("src/main.cpp", "int run(int value) { return value + 1; }"),
            (
                "src/App.cs",
                "public int Run(int value) { return value + 1; }",
            ),
            ("styles/app.css", ".card { color: #fff; }"),
            ("templates/app.html", "<main class=\"card\">Hello</main>"),
            (
                "src/App.java",
                "public int run(int value) { return value + 1; }",
            ),
            ("config/app.json", "{\"enabled\": true, \"count\": 2}"),
            ("lib/app.rb", "def run(value)\n  value + 1\nend"),
            ("Cargo.toml", "[package]\nname = \"workdeck\""),
            ("config/app.yaml", "enabled: true\ncount: 2"),
        ];
        for (path, source) in examples {
            let spans = highlight(Path::new(path), source)
                .unwrap_or_else(|error| panic!("{path}: {error}"));
            assert!(!spans.is_empty(), "{path} produced no syntax spans");
            assert!(
                spans.iter().all(|span| span.start_line == span.end_line),
                "{path} emitted a cross-line span"
            );
        }
    }

    #[test]
    fn multiline_comments_are_split_into_safe_line_local_spans() {
        let source = "fn run() { /* first\nsecond */ }";
        let spans = highlight(Path::new("src/lib.rs"), source).unwrap();
        let comments = spans
            .iter()
            .filter(|span| span.token == "comment")
            .collect::<Vec<_>>();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].start_line, 0);
        assert_eq!(comments[1].start_line, 1);
        assert!(comments.iter().all(|span| span.start_line == span.end_line));
    }

    #[test]
    fn swift_produces_semantic_units_calls_and_canonical_structure() {
        let result = analyze(&document(
            "Sources/App/App.swift",
            "func render() -> String { helper() }\nfunc helper() -> String { \"ok\" }\n",
        ))
        .unwrap();
        assert_eq!(result.language, SourceLanguage::Swift);
        assert!(
            result
                .units
                .iter()
                .any(|unit| unit.title.contains("render"))
        );
        assert!(result.calls.iter().any(|edge| edge.callee == "helper"));
        assert!(result.canonical_tree.is_some());
    }
}
