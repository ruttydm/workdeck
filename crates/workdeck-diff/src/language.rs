use globset::{Glob, GlobMatcher};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LanguageMatcher {
    Extension(String),
    Filename(String),
    Glob { value: String, target_path: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageRegistration {
    pub matcher: LanguageMatcher,
    pub language: String,
    pub reserved: bool,
}

#[derive(Debug)]
struct AppliedRegistration {
    registration: LanguageRegistration,
    glob: Option<GlobMatcher>,
}

#[derive(Debug)]
pub struct LanguageRegistry {
    registrations: Vec<AppliedRegistration>,
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        let mut registry = Self {
            registrations: Vec::new(),
        };
        registry.replace_extensions(Vec::new());
        registry
    }
}

impl LanguageRegistry {
    pub fn replace_extensions(&mut self, extensions: Vec<LanguageRegistration>) {
        let built_in = ["mts", "cts"]
            .into_iter()
            .map(|extension| LanguageRegistration {
                matcher: LanguageMatcher::Extension(extension.into()),
                language: "typescript".into(),
                reserved: true,
            });
        self.registrations = built_in
            .chain(extensions)
            .map(|registration| {
                let glob = match &registration.matcher {
                    LanguageMatcher::Glob { value, .. } if !value.contains('\0') => {
                        Glob::new(&encode_backslashes(value))
                            .ok()
                            .map(|glob| glob.compile_matcher())
                    }
                    _ => None,
                };
                AppliedRegistration { registration, glob }
            })
            .collect();
    }

    pub fn language_for_path(&self, path: &str) -> String {
        let basename = path.rsplit('/').next().unwrap_or(path);
        self.extension_language(basename, true)
            .or_else(|| self.filename_language(basename))
            .or_else(|| self.glob_language(path, basename))
            .or_else(|| self.extension_language(basename, false))
            .or_else(|| built_in_language(path))
            .unwrap_or("text")
            .to_owned()
    }

    fn filename_language<'a>(&'a self, basename: &str) -> Option<&'a str> {
        self.registrations.iter().rev().find_map(|applied| {
            matches!(
                &applied.registration.matcher,
                LanguageMatcher::Filename(value) if value == basename
            )
            .then_some(applied.registration.language.as_str())
        })
    }

    fn glob_language<'a>(&'a self, path: &str, basename: &str) -> Option<&'a str> {
        if path.contains('\0') {
            return None;
        }
        self.registrations.iter().rev().find_map(|applied| {
            let LanguageMatcher::Glob { target_path, .. } = &applied.registration.matcher else {
                return None;
            };
            let candidate = if *target_path { path } else { basename };
            applied
                .glob
                .as_ref()
                .is_some_and(|glob| glob.is_match(encode_backslashes(candidate)))
                .then_some(applied.registration.language.as_str())
        })
    }

    fn extension_language<'a>(&'a self, basename: &str, reserved: bool) -> Option<&'a str> {
        let lower = basename.to_ascii_lowercase();
        let mut best: Option<(usize, &str)> = None;
        for applied in self.registrations.iter().rev() {
            let LanguageMatcher::Extension(extension) = &applied.registration.matcher else {
                continue;
            };
            if applied.registration.reserved != reserved
                || !lower.ends_with(&format!(".{extension}"))
            {
                continue;
            }
            if best.is_none_or(|(length, _)| extension.len() > length) {
                best = Some((extension.len(), applied.registration.language.as_str()));
            }
        }
        best.map(|(_, language)| language)
    }
}

/// Validate one extension-authored glob with the exact parser used by the live registry.
pub fn validate_language_glob(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("glob matcher value must be non-empty".into());
    }
    if value.contains('\0') {
        return Err("glob matchers cannot contain NUL".into());
    }
    Glob::new(&encode_backslashes(value))
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn encode_backslashes(value: &str) -> String {
    value.replace('\\', "\0")
}

fn built_in_language(path: &str) -> Option<&'static str> {
    let basename = Path::new(path).file_name()?.to_str()?;
    match basename {
        "Dockerfile" => return Some("dockerfile"),
        "Makefile" => return Some("makefile"),
        _ => {}
    }
    let extension = basename.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match extension.as_str() {
        "rs" => "rust",
        "ts" => "typescript",
        "tsx" => "tsx",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "py" => "python",
        "rb" => "ruby",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "swift" => "swift",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" => "cpp",
        "cs" => "csharp",
        "php" => "php",
        "vue" => "vue",
        "svelte" => "svelte",
        "html" | "htm" => "html",
        "css" | "scss" | "sass" => "css",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "md" | "mdx" => "markdown",
        "sh" | "bash" | "zsh" => "shell",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registration(matcher: LanguageMatcher, language: &str) -> LanguageRegistration {
        LanguageRegistration {
            matcher,
            language: language.into(),
            reserved: false,
        }
    }

    #[test]
    fn preserves_reserved_and_core_languages() {
        let mut registry = LanguageRegistry::default();
        registry.replace_extensions(vec![
            registration(LanguageMatcher::Filename("special.mts".into()), "python"),
            registration(
                LanguageMatcher::Glob {
                    value: "*.cts".into(),
                    target_path: false,
                },
                "ruby",
            ),
        ]);
        assert_eq!(registry.language_for_path("special.mts"), "typescript");
        assert_eq!(
            registry.language_for_path("nested/example.cts"),
            "typescript"
        );
        assert_eq!(registry.language_for_path("foo.tsx"), "tsx");
        assert_eq!(
            registry.language_for_path("docker/Dockerfile"),
            "dockerfile"
        );
        assert_eq!(
            registry.language_for_path("build/tools/Makefile"),
            "makefile"
        );
    }

    #[test]
    fn selectors_follow_filename_glob_and_longest_extension_priority() {
        let mut registry = LanguageRegistry::default();
        registry.replace_extensions(vec![
            registration(LanguageMatcher::Extension("priority".into()), "ruby"),
            registration(
                LanguageMatcher::Extension("spec.priority".into()),
                "typescript",
            ),
            registration(
                LanguageMatcher::Glob {
                    value: "*.priority".into(),
                    target_path: false,
                },
                "javascript",
            ),
            registration(LanguageMatcher::Filename("exact.priority".into()), "python"),
        ]);
        assert_eq!(registry.language_for_path("exact.priority"), "python");
        assert_eq!(registry.language_for_path("other.priority"), "javascript");
        registry.replace_extensions(vec![
            registration(LanguageMatcher::Extension("longest".into()), "ruby"),
            registration(
                LanguageMatcher::Extension("spec.longest".into()),
                "typescript",
            ),
        ]);
        assert_eq!(
            registry.language_for_path("other.spec.longest"),
            "typescript"
        );
    }

    #[test]
    fn extensions_filenames_and_globs_match_review_paths_exactly() {
        let mut registry = LanguageRegistry::default();
        registry.replace_extensions(vec![
            registration(LanguageMatcher::Extension("hunklegacy".into()), "python"),
            registration(LanguageMatcher::Filename("Hunkfile".into()), "ruby"),
            registration(LanguageMatcher::Filename(" Tool\\Hunkfile ".into()), "ruby"),
            registration(
                LanguageMatcher::Glob {
                    value: "*.hunkbasename".into(),
                    target_path: false,
                },
                "ruby",
            ),
            registration(
                LanguageMatcher::Glob {
                    value: "generated/**/*.hunkpath".into(),
                    target_path: true,
                },
                "python",
            ),
        ]);
        assert_eq!(registry.language_for_path("x.hunklegacy"), "python");
        assert_eq!(registry.language_for_path("hunklegacy"), "text");
        assert_eq!(registry.language_for_path("tools/Hunkfile"), "ruby");
        assert_eq!(registry.language_for_path("tools/hunkfile"), "text");
        assert_eq!(
            registry.language_for_path("nested/ Tool\\Hunkfile "),
            "ruby"
        );
        assert_eq!(
            registry.language_for_path("nested/example.hunkbasename"),
            "ruby"
        );
        assert_eq!(
            registry.language_for_path("generated/example.hunkpath"),
            "python"
        );
        assert_eq!(
            registry.language_for_path("generated/nested/example.hunkpath"),
            "python"
        );
        assert_eq!(
            registry.language_for_path("generated\\nested\\example.hunkpath"),
            "text"
        );
    }

    #[test]
    fn glob_question_marks_treat_literal_backslashes_as_one_character() {
        let mut registry = LanguageRegistry::default();
        registry.replace_extensions(vec![
            registration(
                LanguageMatcher::Glob {
                    value: "foo?bar".into(),
                    target_path: false,
                },
                "python",
            ),
            registration(
                LanguageMatcher::Glob {
                    value: "foo??bar".into(),
                    target_path: false,
                },
                "ruby",
            ),
        ]);
        assert_eq!(registry.language_for_path("foo\\bar"), "python");
        assert_eq!(registry.language_for_path("fooXYbar"), "ruby");
        assert_eq!(registry.language_for_path("foo\0bar"), "text");
        registry.replace_extensions(vec![registration(
            LanguageMatcher::Glob {
                value: "foo[\\]bar".into(),
                target_path: false,
            },
            "ruby",
        )]);
        assert_eq!(registry.language_for_path("foo\\bar"), "ruby");
        assert_eq!(registry.language_for_path("fooXbar"), "text");
    }

    #[test]
    fn replacement_removes_stale_rules_and_keeps_latest_tie() {
        let mut registry = LanguageRegistry::default();
        registry.replace_extensions(vec![registration(
            LanguageMatcher::Filename("Reloadfile".into()),
            "python",
        )]);
        assert_eq!(registry.language_for_path("nested/Reloadfile"), "python");
        registry.replace_extensions(vec![]);
        assert_eq!(registry.language_for_path("nested/Reloadfile"), "text");
        registry.replace_extensions(vec![
            registration(LanguageMatcher::Extension("repeat".into()), "python"),
            registration(LanguageMatcher::Extension("repeat".into()), "ruby"),
        ]);
        assert_eq!(registry.language_for_path("foo.repeat"), "ruby");
    }

    #[test]
    fn extension_glob_validation_uses_the_live_registry_parser() {
        assert!(validate_language_glob("generated/**/*.rs").is_ok());
        assert!(validate_language_glob("foo[\\]bar").is_ok());
        assert!(validate_language_glob("").is_err());
        assert!(validate_language_glob("bad\0glob").is_err());
        assert!(validate_language_glob("[").is_err());
    }
}
