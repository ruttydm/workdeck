use std::path::Path;

use serde::Deserialize;
use serde_yaml_ng::{Mapping, Value};
use workdeck_pm::documents::{MAX_DOCUMENT_BYTES, MarkdownDocument, YamlDocument};

fn path() -> &'static Path {
    Path::new(".workdeck/issues/WD-1/item.md")
}

fn changes(yaml: &str) -> Mapping {
    serde_yaml_ng::from_str(yaml).unwrap()
}

#[test]
fn unchanged_markdown_preserves_all_bytes_and_unknown_nested_metadata() {
    for newline in ["\n", "\r\n"] {
        let source = "---\n# author comment\ntitle: 'Keep me' # inline\ncustom:\n  unknown: {a: 1, b: [true, 'yes']}\n  prose: |-\n    line one\n    line two\n---\n\n# Acceptance\n\n- [ ] done\n\n".replace('\n', newline);
        let document = MarkdownDocument::parse(path(), &source).unwrap();
        assert_eq!(document.render(), source);
        assert!(
            document
                .body()
                .starts_with(&format!("{newline}# Acceptance"))
        );
        assert_eq!(document.metadata()["custom"]["unknown"]["b"][1], "yes");
    }
}

#[test]
fn field_edit_preserves_unrelated_comments_formatting_and_body() {
    let source = "---\n# Before title\ntitle: 'Before'  # title comment\n\ncustom: {odd: 'yes', enabled: true} # untouched\nprose: >-\n  folded line\n  second line\n---\n\nThe body is **verbatim**.\n";
    let mut document = MarkdownDocument::parse(path(), source).unwrap();
    document.patch(&changes("title: After\n")).unwrap();
    let rendered = document.render();
    assert!(rendered.contains("# Before title\n"), "{rendered}");
    assert!(rendered.contains("# title comment\n"), "{rendered}");
    assert!(rendered.contains("\ncustom: {odd: 'yes', enabled: true} # untouched\nprose: >-\n  folded line\n  second line\n"), "{rendered}");
    assert_eq!(document.body(), "\nThe body is **verbatim**.\n");
    assert_eq!(document.metadata()["title"], "After");
    assert_eq!(
        MarkdownDocument::parse(path(), &rendered)
            .unwrap()
            .metadata(),
        document.metadata()
    );
}

#[test]
fn semantic_noop_is_byte_identical() {
    let source = "---\ntitle: 'Before' # Keep quotes\ncustom: {a: 1}\n---\nbody";
    let mut document = MarkdownDocument::parse(path(), source).unwrap();
    document
        .patch(&changes("title: Before\ncustom: {a: 1}\nabsent: null\n"))
        .unwrap();
    assert_eq!(document.render(), source);
}

#[test]
fn changed_and_inserted_fields_keep_crlf() {
    let source = "---\r\n# hello\r\ntitle: Before\r\ncustom: {a: 1}\r\n---\r\nbody\r\n";
    let mut document = MarkdownDocument::parse(path(), source).unwrap();
    document
        .patch(&changes("title: After\nlabels: [bug, auth]\n"))
        .unwrap();
    let rendered = document.render();
    assert!(!rendered.replace("\r\n", "").contains('\n'), "{rendered:?}");
    assert!(rendered.contains("custom: {a: 1}\r\n"));
    assert_eq!(document.metadata()["labels"][1], "auth");
    assert_eq!(document.body(), "body\r\n");
}

#[test]
fn insertion_serializes_multiline_and_yaml_looking_strings_as_data() {
    let mut document = MarkdownDocument::parse(path(), "---\ntitle: Before\n---\nbody").unwrap();
    for title in [
        "After\nadmin: true\n---\ninjected",
        "true",
        "null",
        "123",
        "!tag",
        "a: b # c",
        "*anchor",
        "\"quoted\"",
        "emoji 🦀",
        "line\u{0085}separator",
        "line\u{2028}separator",
        "control\u{007f}value",
    ] {
        let patch =
            Mapping::from_iter([(Value::String("title".into()), Value::String(title.into()))]);
        document.patch(&patch).unwrap();
        assert_eq!(document.metadata()["title"], title);
        assert_eq!(document.metadata().len(), 1);
        assert_eq!(document.body(), "body");
        assert_eq!(
            MarkdownDocument::parse(path(), &document.render())
                .unwrap()
                .metadata()["title"],
            title
        );
    }
}

#[test]
fn removes_requested_key_and_keeps_following_comment_and_unknown_fields() {
    let source = "---\ntitle: Before\nassignee: someone\n# Keep custom comment\ncustom:\n  setting: [1, 2]\n---\nbody\n";
    let mut document = MarkdownDocument::parse(path(), source).unwrap();
    document.patch(&changes("assignee: null\n")).unwrap();
    assert!(!document.metadata().contains_key("assignee"));
    assert!(
        document
            .render()
            .contains("# Keep custom comment\ncustom:\n  setting: [1, 2]\n")
    );
}

#[test]
fn supports_replacing_collections_scalars_and_flow_roots() {
    for source in [
        "---\ntitle: Before\ncustom:\n  old: value\n---\nbody",
        "---\n{title: Before, custom: {old: value}}\n---\nbody",
    ] {
        let mut document = MarkdownDocument::parse(path(), source).unwrap();
        for patch in [
            "custom: {new: [one, two], count: 3}\n",
            "custom: scalar\n",
            "custom: [one, {two: false}]\n",
            "custom: {}\n",
        ] {
            let patch = changes(patch);
            document.patch(&patch).unwrap();
            assert_eq!(document.metadata()["custom"], patch["custom"]);
            assert_eq!(document.metadata()["title"], "Before");
        }
    }
}

#[test]
fn body_replacement_keeps_header_verbatim() {
    let source = "---\r\n# header\r\ntitle: 'Before'\r\n---\r\nold\n";
    let mut document = MarkdownDocument::parse(path(), source).unwrap();
    document.set_body("new\n\nbody".into()).unwrap();
    assert_eq!(
        document.render(),
        "---\r\n# header\r\ntitle: 'Before'\r\n---\r\nnew\n\nbody"
    );
}

#[test]
fn accepts_empty_mapping_and_closing_fence_at_eof() {
    let mut document = MarkdownDocument::parse(path(), "---\n{}\n---").unwrap();
    assert_eq!(document.render(), "---\n{}\n---");
    document.patch(&changes("title: Created\n")).unwrap();
    assert_eq!(document.metadata()["title"], "Created");
}

#[test]
fn rejects_duplicate_known_and_custom_keys_including_nested_flow_keys() {
    for yaml in [
        "title: one\ntitle: two\n",
        "title: one\n\"title\": two\n",
        "custom:\n  a: one\n  a: two\n",
        "custom: {a: 1, a: 2}\n",
    ] {
        let error = MarkdownDocument::parse(path(), &format!("---\n{yaml}---\nbody")).unwrap_err();
        assert_eq!(error.path, path());
        assert!(error.line >= 2);
        assert!(error.column >= 1);
        assert!(error.message.contains("duplicate"), "{error}");
    }
}

#[test]
fn rejects_unsupported_yaml_features_before_alias_expansion() {
    for yaml in [
        "title: &a value\n",
        "title: *a\n",
        "title: !custom value\n",
        "title: !!str value\n",
        "custom: {<<: {a: 1}}\n",
        "? [a, b]\n: value\n",
        "custom: {1: value}\n",
        "%YAML 1.2\n---\ntitle: Before\n",
        "title: one\n---\ntitle: two\n",
    ] {
        let error = YamlDocument::parse(path(), yaml).unwrap_err();
        assert_eq!(error.path, path());
        assert!(error.line >= 1);
        assert!(error.column >= 1);
    }
}

#[test]
fn preserves_anchor_and_tag_looking_text_inside_strings_and_block_scalars() {
    let source = "---\ntitle: '*this is text'\ncustom: |-\n  !tag &anchor *alias\n  [brackets] {braces}\n---\nbody";
    let document = MarkdownDocument::parse(path(), source).unwrap();
    assert_eq!(document.render(), source);
}

#[test]
fn rejects_missing_frontmatter_invalid_yaml_and_conflicts_with_positions() {
    for source in [
        "title: No fence\n",
        "---\ntitle: Missing end\n",
        "---\ntitle: [broken\n---\n",
        "---\n- list\n---\n",
        "---\ntitle: Before\n---\n<<<<<<< branch\nours\n=======\ntheirs\n>>>>>>> main\n",
    ] {
        let error = MarkdownDocument::parse(path(), source).unwrap_err();
        assert_eq!(error.path, path());
        assert!(error.line >= 1 && error.column >= 1, "{error}");
        assert!(error.to_string().contains("item.md"));
    }
    let error =
        MarkdownDocument::parse(path(), "---\ntitle: ok\ncustom: [broken\n---\n").unwrap_err();
    assert!(error.line >= 3, "{error}");
}

#[test]
fn validates_typed_metadata_with_original_source_locations() {
    #[derive(Debug, Deserialize)]
    struct Metadata {
        title: String,
        priority: u8,
    }
    let document =
        MarkdownDocument::parse(path(), "---\ntitle: Example\npriority: 3\n---\n").unwrap();
    let metadata: Metadata = document.deserialize().unwrap();
    assert_eq!(metadata.title, "Example");
    assert_eq!(metadata.priority, 3);
    let invalid =
        MarkdownDocument::parse(path(), "---\ntitle: Example\npriority: high\n---\n").unwrap();
    let error = invalid.deserialize::<Metadata>().unwrap_err();
    assert_eq!(error.line, 3, "{error}");
}

#[test]
fn oversized_or_deep_input_is_rejected_without_mutating_document() {
    let oversized = format!(
        "---\ntitle: Before\n---\n{}",
        "x".repeat(MAX_DOCUMENT_BYTES)
    );
    assert!(
        MarkdownDocument::parse(path(), &oversized)
            .unwrap_err()
            .message
            .contains("limit")
    );
    let nested = format!("title: {}0{}\n", "[".repeat(1000), "]".repeat(1000));
    assert!(
        YamlDocument::parse(path(), &nested)
            .unwrap_err()
            .message
            .contains("depth")
    );
    let block_nested = (0..1000)
        .map(|n| format!("{}key:\n", " ".repeat(n)))
        .collect::<String>();
    assert!(YamlDocument::parse(path(), &block_nested).is_err());
    let source = "---\ntitle: Before\n---\nbody";
    let mut document = MarkdownDocument::parse(path(), source).unwrap();
    assert!(document.set_body("x".repeat(MAX_DOCUMENT_BYTES)).is_err());
    assert_eq!(document.render(), source);
    let patch = Mapping::from_iter([(
        Value::String("title".into()),
        Value::String("x".repeat(MAX_DOCUMENT_BYTES)),
    )]);
    assert!(document.patch(&patch).is_err());
    assert_eq!(document.render(), source);
}

#[test]
fn failed_patch_does_not_apply_earlier_fields() {
    let source = "---\ntitle: Before\n---\nbody";
    let mut document = MarkdownDocument::parse(path(), source).unwrap();
    let patch = Mapping::from_iter([
        (Value::String("title".into()), Value::String("After".into())),
        (Value::Number(1.into()), Value::String("invalid key".into())),
    ]);
    assert!(document.patch(&patch).is_err());
    assert_eq!(document.render(), source);
}

#[test]
fn yaml_documents_preserve_stream_comments_and_explicit_start() {
    let source = "# file comment\n---\n# item comment\ntitle: 'Before'\ncustom: {a: 1}\n# tail\n";
    let mut document = YamlDocument::parse(Path::new(".workdeck/config.yml"), source).unwrap();
    assert_eq!(document.render(), source);
    document.patch(&changes("title: After\n")).unwrap();
    assert!(
        document
            .render()
            .starts_with("# file comment\n---\n# item comment\n")
    );
    assert!(document.render().ends_with("custom: {a: 1}\n# tail\n"));
}

#[test]
fn removing_last_field_keeps_an_editable_empty_mapping() {
    let mut document = MarkdownDocument::parse(
        path(),
        "---\n# file comment\ntitle: Before\n# tail\n---\nbody",
    )
    .unwrap();
    document.patch(&changes("title: null\n")).unwrap();
    assert!(document.metadata().is_empty());
    assert!(document.render().contains("# file comment\n"));
    assert!(document.render().contains("# tail\n"));
    MarkdownDocument::parse(path(), &document.render()).unwrap();
    document.patch(&changes("title: Again\n")).unwrap();
    assert_eq!(document.metadata()["title"], "Again");
}

#[test]
fn null_patch_removes_a_present_null_field() {
    let mut document = YamlDocument::parse(path(), "title: Before\nassignee: null\n").unwrap();
    document.patch(&changes("assignee: null\n")).unwrap();
    assert!(!document.metadata().contains_key("assignee"));
}

#[test]
fn preserves_unchanged_mixed_line_endings_when_inserting() {
    let source = "---\r\ntitle: Before\r\ncustom: {a: 1}\n# mixed tail\n---\r\nbody\n";
    let mut document = MarkdownDocument::parse(path(), source).unwrap();
    document.patch(&changes("labels: [bug]\n")).unwrap();
    assert!(
        document.render().contains("custom: {a: 1}\n# mixed tail\n"),
        "{}",
        document.render()
    );
    assert_eq!(document.body(), "body\n");
}

#[test]
fn patch_keys_and_nonfinite_numbers_cannot_change_value_types() {
    let mut document = MarkdownDocument::parse(path(), "---\ntitle: Before\n---\nbody").unwrap();
    let patch = Mapping::from_iter([
        (Value::String("null".into()), Value::Bool(false)),
        (
            Value::String("odd:\nkey".into()),
            Value::String("value".into()),
        ),
        (
            Value::String("nan".into()),
            serde_yaml_ng::from_str(".nan").unwrap(),
        ),
        (
            Value::String("infinity".into()),
            serde_yaml_ng::from_str(".inf").unwrap(),
        ),
    ]);
    document.patch(&patch).unwrap();
    for (key, value) in &patch {
        assert_eq!(document.metadata().get(key), Some(value));
    }
    assert_eq!(document.metadata().len(), 5);
}

#[test]
fn nested_alias_bomb_and_compact_deep_sequences_are_rejected() {
    let source = "a: &a [x, x, x]\nb: &b [*a, *a, *a]\nc: [*b, *b, *b]\n";
    assert!(
        YamlDocument::parse(path(), source)
            .unwrap_err()
            .message
            .contains("unsupported")
    );
    let compact = format!("{}value\n", "- ".repeat(1000));
    assert!(YamlDocument::parse(path(), &compact).is_err());
}

#[test]
fn blocks_with_explicit_indentation_and_chomping_remain_unchanged() {
    let source = "---\ntitle: Before\ncustom:\n  description: |2+\n    # This is text\n    &not_an_anchor\n\n  other: done\n---\nbody";
    let mut document = MarkdownDocument::parse(path(), source).unwrap();
    document.patch(&changes("title: After\n")).unwrap();
    assert!(document.render().contains(
        "custom:\n  description: |2+\n    # This is text\n    &not_an_anchor\n\n  other: done\n"
    ));
}
