use crate::git::ChangeEntry;
use crate::store::{AgentSession, Issue, ReferenceData};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchTarget {
    File(PathBuf),
    Change(PathBuf),
    Issue(String),
    AgentSession(String),
    Project(String),
    Cycle(String),
    Label(String),
    Symbol {
        path: PathBuf,
        line: usize,
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRecord {
    pub label: String,
    pub detail: String,
    pub haystack: String,
    pub target: SearchTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolRecord {
    pub path: PathBuf,
    pub line: usize,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub score: i64,
    pub record: SearchRecord,
}

#[derive(Debug, Clone, Default)]
pub struct SearchIndex {
    records: Vec<SearchRecord>,
}

impl SearchIndex {
    pub fn rebuild(
        files: &[PathBuf],
        changes: &[ChangeEntry],
        issues: &[Issue],
        sessions: &[AgentSession],
        references: &ReferenceData,
        symbols: &[SymbolRecord],
    ) -> Self {
        let mut records = Vec::new();
        for file in files {
            let label = file.to_string_lossy().to_string();
            records.push(SearchRecord {
                haystack: label.clone(),
                label,
                detail: "file".to_string(),
                target: SearchTarget::File(file.clone()),
            });
        }

        for change in changes {
            let label = change.path_display();
            let detail = format!("{} {} change", change.kind.label(), change.stage_label());
            records.push(SearchRecord {
                haystack: format!("{label} {detail}"),
                label,
                detail,
                target: SearchTarget::Change(change.path.clone()),
            });
        }

        for issue in issues {
            let label = format!("{} {}", issue.key, issue.title);
            let detail = format!(
                "issue {} {} {} {}",
                issue.status.label(),
                issue.priority.label(),
                issue.project,
                issue.cycle
            );
            records.push(SearchRecord {
                haystack: [
                    label.as_str(),
                    detail.as_str(),
                    issue.description.as_str(),
                    issue.assignee.as_str(),
                    &issue.labels.join(" "),
                    &issue.linked_files.join(" "),
                    &issue.linked_commits.join(" "),
                ]
                .join(" "),
                label,
                detail,
                target: SearchTarget::Issue(issue.key.clone()),
            });
        }

        for session in sessions {
            let label = format!("{} {}", session.id, session.title);
            let touched_files = session
                .touched_files
                .iter()
                .map(|file| format!("{} {}", file.path, file.change_type))
                .collect::<Vec<_>>()
                .join(" ");
            let detail = format!("agent {} {}", session.agent, session.status);
            records.push(SearchRecord {
                haystack: [
                    label.as_str(),
                    detail.as_str(),
                    session.cwd.as_str(),
                    session.goal.as_str(),
                    session.summary.as_str(),
                    &session.plan.join(" "),
                    &session.commands_run.join(" "),
                    &session.tests_run.join(" "),
                    &session.handoff_notes.join(" "),
                    &touched_files,
                ]
                .join(" "),
                label,
                detail,
                target: SearchTarget::AgentSession(session.id.clone()),
            });
        }

        for project in &references.projects {
            let label = format!("{} {}", project.id, project.name);
            let detail = format!("project {}", project.status);
            records.push(SearchRecord {
                haystack: [
                    label.as_str(),
                    detail.as_str(),
                    project.description.as_str(),
                    project.created_at.as_str(),
                    project.updated_at.as_str(),
                ]
                .join(" "),
                label,
                detail,
                target: SearchTarget::Project(project.id.clone()),
            });
        }

        for cycle in &references.cycles {
            let label = format!("{} {}", cycle.id, cycle.name);
            let detail = format!("cycle {}", cycle.status);
            records.push(SearchRecord {
                haystack: [
                    label.as_str(),
                    detail.as_str(),
                    cycle.starts_at.as_str(),
                    cycle.ends_at.as_str(),
                ]
                .join(" "),
                label,
                detail,
                target: SearchTarget::Cycle(cycle.id.clone()),
            });
        }

        for label_ref in &references.labels {
            let label = format!("{} {}", label_ref.id, label_ref.name);
            let detail = format!("label {}", label_ref.color);
            records.push(SearchRecord {
                haystack: [label.as_str(), detail.as_str()].join(" "),
                label,
                detail,
                target: SearchTarget::Label(label_ref.id.clone()),
            });
        }

        for symbol in symbols {
            let label = format!("{}:{} {}", symbol.path.display(), symbol.line, symbol.name);
            let detail = format!("symbol {}", symbol.kind);
            let normalized_name = symbol.name.replace(['_', '-'], " ");
            records.push(SearchRecord {
                haystack: [label.as_str(), detail.as_str(), normalized_name.as_str()].join(" "),
                label,
                detail,
                target: SearchTarget::Symbol {
                    path: symbol.path.clone(),
                    line: symbol.line,
                    name: symbol.name.clone(),
                },
            });
        }

        Self { records }
    }

    pub fn query(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        if query.trim().is_empty() {
            return self
                .records
                .iter()
                .take(limit)
                .cloned()
                .map(|record| SearchResult { score: 0, record })
                .collect();
        }

        let matcher = SkimMatcherV2::default();
        let mut results = self
            .records
            .iter()
            .filter_map(|record| {
                matcher
                    .fuzzy_match(&record.haystack, query)
                    .map(|score| SearchResult {
                        score,
                        record: record.clone(),
                    })
            })
            .collect::<Vec<_>>();
        results.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then(a.record.label.cmp(&b.record.label))
        });
        results.truncate(limit);
        results
    }
}

const MAX_SYMBOL_FILES: usize = 1_000;
const MAX_SYMBOL_BYTES: usize = 64 * 1024;
const MAX_SYMBOLS_PER_FILE: usize = 32;

pub fn extract_symbols(repo_root: &Path, files: &[PathBuf]) -> Vec<SymbolRecord> {
    let mut symbols = Vec::new();
    for path in files
        .iter()
        .filter(|path| is_symbol_source(path))
        .take(MAX_SYMBOL_FILES)
    {
        let Ok(bytes) = fs::read(repo_root.join(path)) else {
            continue;
        };
        let bytes = if bytes.len() > MAX_SYMBOL_BYTES {
            &bytes[..MAX_SYMBOL_BYTES]
        } else {
            &bytes
        };
        if bytes.contains(&0) {
            continue;
        }
        let Ok(content) = std::str::from_utf8(bytes) else {
            continue;
        };
        let mut file_count = 0;
        for (line_index, line) in content.lines().enumerate() {
            if let Some((kind, name)) = extract_symbol_from_line(path, line) {
                symbols.push(SymbolRecord {
                    path: path.clone(),
                    line: line_index + 1,
                    name,
                    kind,
                });
                file_count += 1;
                if file_count >= MAX_SYMBOLS_PER_FILE {
                    break;
                }
            }
        }
    }
    symbols
}

fn is_symbol_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some(
            "rs" | "php"
                | "js"
                | "jsx"
                | "ts"
                | "tsx"
                | "vue"
                | "py"
                | "rb"
                | "go"
                | "java"
                | "kt"
                | "swift"
        )
    )
}

fn extract_symbol_from_line(path: &Path, line: &str) -> Option<(String, String)> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with('*') {
        return None;
    }

    match path.extension().and_then(|extension| extension.to_str()) {
        Some("rs") => extract_prefixed_symbol(
            trimmed,
            &[
                ("pub async fn ", "fn"),
                ("pub fn ", "fn"),
                ("async fn ", "fn"),
                ("fn ", "fn"),
                ("pub struct ", "struct"),
                ("struct ", "struct"),
                ("pub enum ", "enum"),
                ("enum ", "enum"),
                ("pub trait ", "trait"),
                ("trait ", "trait"),
                ("impl ", "impl"),
            ],
        ),
        Some("php") => extract_prefixed_symbol(
            trimmed,
            &[
                ("public function ", "function"),
                ("protected function ", "function"),
                ("private function ", "function"),
                ("function ", "function"),
                ("final class ", "class"),
                ("abstract class ", "class"),
                ("class ", "class"),
                ("interface ", "interface"),
                ("trait ", "trait"),
            ],
        ),
        Some("py") => extract_prefixed_symbol(
            trimmed,
            &[
                ("async def ", "function"),
                ("def ", "function"),
                ("class ", "class"),
            ],
        ),
        Some("js" | "jsx" | "ts" | "tsx" | "vue") => extract_js_like_symbol(trimmed),
        Some("rb") => extract_prefixed_symbol(
            trimmed,
            &[
                ("def ", "method"),
                ("class ", "class"),
                ("module ", "module"),
            ],
        ),
        Some("go") => extract_prefixed_symbol(trimmed, &[("func ", "function"), ("type ", "type")]),
        Some("java" | "kt" | "swift") => extract_prefixed_symbol(
            trimmed,
            &[
                ("class ", "class"),
                ("struct ", "struct"),
                ("enum ", "enum"),
                ("func ", "function"),
            ],
        ),
        _ => None,
    }
}

fn extract_js_like_symbol(line: &str) -> Option<(String, String)> {
    extract_prefixed_symbol(
        line,
        &[
            ("export async function ", "function"),
            ("export function ", "function"),
            ("async function ", "function"),
            ("function ", "function"),
            ("export default class ", "class"),
            ("export class ", "class"),
            ("class ", "class"),
        ],
    )
    .or_else(|| extract_assigned_symbol(line, "export const ", "const"))
    .or_else(|| extract_assigned_symbol(line, "const ", "const"))
    .or_else(|| extract_assigned_symbol(line, "let ", "let"))
}

fn extract_prefixed_symbol(line: &str, prefixes: &[(&str, &str)]) -> Option<(String, String)> {
    for (prefix, kind) in prefixes {
        if let Some(rest) = line.strip_prefix(prefix) {
            let name = take_symbol_name(rest);
            if !name.is_empty() {
                return Some(((*kind).to_string(), name));
            }
        }
    }
    None
}

fn extract_assigned_symbol(line: &str, prefix: &str, kind: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix(prefix)?;
    let name = take_symbol_name(rest);
    if name.is_empty() || !rest[name.len()..].trim_start().starts_with('=') {
        return None;
    }
    Some((kind.to_string(), name))
}

fn take_symbol_name(value: &str) -> String {
    value
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '$')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_issue_by_key() {
        let issue = Issue::new("WD-42".to_string(), "Render changes".to_string());
        let index = SearchIndex::rebuild(&[], &[], &[issue], &[], &ReferenceData::default(), &[]);

        let results = index.query("42", 5);

        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].record.target, SearchTarget::Issue(_)));
    }

    #[test]
    fn finds_issue_by_metadata_without_polluting_label() {
        let mut issue = Issue::new("WD-7".to_string(), "Fix preview".to_string());
        issue.description = "Binary fallback is wrong".to_string();
        issue.project = "workdeck-mvp".to_string();
        issue.labels = vec!["preview".to_string(), "binary".to_string()];
        issue.linked_files = vec!["crates/workdeck-cli/src/git/mod.rs".to_string()];
        let index = SearchIndex::rebuild(&[], &[], &[issue], &[], &ReferenceData::default(), &[]);

        let results = index.query("binary", 5);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].record.label, "WD-7 Fix preview");
    }

    #[test]
    fn finds_agent_by_work_state_fields() {
        let mut session = AgentSession::new("Preview hardening".to_string());
        session.id = "session-1".to_string();
        session.goal = "Move preview loading off render path".to_string();
        session.plan = vec!["add worker".to_string()];
        session.tests_run = vec!["cargo test".to_string()];
        session.touched_files = vec![crate::store::AgentTouchedFile {
            path: "crates/workdeck-cli/src/app.rs".to_string(),
            change_type: "modified".to_string(),
        }];
        let index = SearchIndex::rebuild(&[], &[], &[], &[session], &ReferenceData::default(), &[]);

        let results = index.query("render path", 5);

        assert_eq!(results.len(), 1);
        assert!(matches!(
            results[0].record.target,
            SearchTarget::AgentSession(_)
        ));
    }

    #[test]
    fn finds_reference_data_and_symbols() {
        let mut references = ReferenceData::default();
        references.projects.push(crate::store::Project::new(
            "workdeck-mvp".to_string(),
            "Workdeck MVP".to_string(),
        ));
        references.labels.push(crate::store::Label::new(
            "preview".to_string(),
            "Preview".to_string(),
        ));
        let symbols = vec![SymbolRecord {
            path: PathBuf::from("src/app.rs"),
            line: 42,
            name: "refresh_workspace".to_string(),
            kind: "fn".to_string(),
        }];
        let index = SearchIndex::rebuild(&[], &[], &[], &[], &references, &symbols);

        assert!(matches!(
            index.query("mvp", 5)[0].record.target,
            SearchTarget::Project(_)
        ));
        assert!(matches!(
            index.query("refresh workspace", 5)[0].record.target,
            SearchTarget::Symbol { .. }
        ));
    }

    #[test]
    fn extracts_symbols_from_common_source_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/app.rs"),
            "pub fn run_app() {}\nstruct Workdeck {}\n",
        )
        .unwrap();

        let symbols = extract_symbols(dir.path(), &[PathBuf::from("src/app.rs")]);

        assert_eq!(symbols[0].name, "run_app");
        assert_eq!(symbols[1].name, "Workdeck");
    }
}
