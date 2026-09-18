use super::*;
use tempfile::tempdir;
use workdeck_core::CommonOptions;

fn diff_input() -> VcsDiffCommandInput {
    VcsDiffCommandInput {
        range: None,
        range_endpoints: None,
        staged: false,
        pathspecs: Vec::new(),
        options: CommonOptions::default(),
    }
}

fn missing_context() -> JujutsuCommandContext {
    JujutsuCommandContext {
        cwd: std::env::temp_dir(),
        jj_executable: "definitely-not-a-real-jj-binary".into(),
    }
}

#[cfg(unix)]
fn fake_jj(command_log: &std::path::Path) -> tempfile::TempDir {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn quote(path: &std::path::Path) -> String {
        format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
    }

    let directory = tempdir().unwrap();
    let executable = directory.path().join("jj");
    let script = format!(
        concat!(
            "#!/bin/sh\n",
            "printf '%s\\n' \"$*\" >> {}\n",
            "case \"$*\" in\n",
            "  *\" -r @ -T \"*) printf 'bbbb\\n' ;;\n",
            "  *\" -r bbbb- -T \"*) printf 'aaaa\\n' ;;\n",
            "  *\" -r main -T \"*) printf '1111\\n' ;;\n",
            "  *\" -r feature -T \"*) printf '2222\\n' ;;\n",
            "  *\" -r multi -T \"*) printf '1111\\n2222\\n' ;;\n",
            "  *\" -r merge -T \"*) printf 'dddd\\n' ;;\n",
            "  *\" -r dddd- -T \"*) printf 'cccc\\naaaa\\n' ;;\n",
            "esac\n"
        ),
        quote(command_log)
    );
    fs::write(&executable, script).unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions).unwrap();
    directory
}

#[test]
fn compares_named_revisions_with_from_to_instead_of_a_range_revset() {
    let mut input = diff_input();
    input.range_endpoints = Some(VcsRangeEndpoints {
        from: "main".into(),
        to: "feature".into(),
    });
    assert_eq!(
        build_jj_diff_args(&input, None, false).unwrap(),
        [
            "diff",
            "--git",
            "--ignore-working-copy",
            "--from",
            "main",
            "--to",
            "feature"
        ]
    );
    assert_eq!(
        build_jj_diff_args(&input, None, true).unwrap(),
        ["diff", "--git", "--from", "main", "--to", "feature"]
    );
}

#[test]
fn rejects_option_like_or_empty_endpoints_before_commands_or_revision_probes() {
    for (from, to, fragment) in [
        ("--from-file", "feature", "looks like a Jujutsu option"),
        ("main", "--to-file", "looks like a Jujutsu option"),
        ("", "feature", "empty revision"),
        ("main", "", "empty revision"),
    ] {
        let mut input = diff_input();
        let endpoints = VcsRangeEndpoints {
            from: from.into(),
            to: to.into(),
        };
        input.range_endpoints = Some(endpoints.clone());
        assert!(
            build_jj_diff_args(&input, None, false)
                .unwrap_err()
                .to_string()
                .contains(fragment)
        );
        assert!(
            resolve_jj_range_endpoints(&input, &endpoints, &missing_context())
                .unwrap_err()
                .to_string()
                .contains(fragment)
        );
    }
}

#[test]
fn passes_an_explicit_revset_through_to_revision_argument() {
    let mut input = diff_input();
    input.range = Some("trunk()..@".into());
    assert_eq!(
        build_jj_diff_args(&input, None, false).unwrap(),
        ["diff", "--git", "-r", "trunk()..@"]
    );
}

#[test]
fn uses_immutable_overrides_without_changing_fileset_arguments() {
    let mut diff = diff_input();
    diff.pathspecs = vec!["src/a b.ts".into()];
    assert_eq!(
        build_jj_diff_args(
            &diff,
            Some(&JujutsuPinnedDiff::Revision("abc123".into())),
            false
        )
        .unwrap(),
        ["diff", "--git", "-r", "abc123", "--", "src/a b.ts"]
    );
    let show = VcsShowCommandInput {
        reference: Some("moving".into()),
        pathspecs: vec!["-odd.ts".into()],
        options: CommonOptions::default(),
    };
    assert_eq!(
        build_jj_show_args(&show, Some("abc123")),
        ["diff", "--git", "-r", "abc123", "--", "-odd.ts"]
    );
}

#[test]
fn reports_a_friendly_error_when_jj_is_not_installed_or_on_path() {
    let error = run_jj_text(
        &JujutsuBackedInput::Diff(diff_input()),
        &["root".into()],
        &missing_context(),
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Jujutsu is required for `workdeck diff` when `vcs = \"jj\"`, but `definitely-not-a-real-jj-binary` was not found in PATH."
    );
}

#[test]
fn reports_a_friendly_error_outside_a_jj_repository() {
    let translated = translate_jj_exit_failure(
        &JujutsuBackedInput::Diff(diff_input()),
        "Error: There is no jj repo in /tmp",
    );
    assert_eq!(
        translated.to_string(),
        "`workdeck diff` must be run inside a Jujutsu repository when `vcs = \"jj\"`."
    );
}

#[test]
fn reports_a_friendly_error_for_invalid_revsets() {
    let mut diff = diff_input();
    diff.range = Some("missing_revision".into());
    let translated = translate_jj_exit_failure(
        &JujutsuBackedInput::Diff(diff),
        "Error: Revision not found: missing_revision",
    );
    assert_eq!(
        translated.to_string(),
        "`workdeck diff missing_revision` could not resolve Jujutsu revset `missing_revision`."
    );
}

#[cfg(unix)]
#[test]
fn resolves_a_single_revision_to_immutable_commit_and_parent_endpoints() {
    let temp = tempdir().unwrap();
    let log = temp.path().join("commands.log");
    let binary = fake_jj(&log);
    let context = JujutsuCommandContext {
        cwd: temp.path().into(),
        jj_executable: binary.path().join("jj"),
    };
    assert_eq!(
        resolve_jj_diff_endpoints(&JujutsuBackedInput::Diff(diff_input()), "@", &context).unwrap(),
        Some(JujutsuDiffEndpoints {
            new_commit_id: "bbbb".into(),
            old_commit_ids: vec!["aaaa".into()],
        })
    );
}

#[cfg(unix)]
#[test]
fn bypasses_template_aliases_when_resolving_immutable_endpoints() {
    let temp = tempdir().unwrap();
    let log = temp.path().join("commands.log");
    let binary = fake_jj(&log);
    let context = JujutsuCommandContext {
        cwd: temp.path().into(),
        jj_executable: binary.path().join("jj"),
    };
    resolve_jj_diff_endpoints(&JujutsuBackedInput::Diff(diff_input()), "@", &context).unwrap();
    let commands = std::fs::read_to_string(log).unwrap();
    assert!(
        commands
            .lines()
            .all(|command| command.contains(JJ_COMMIT_ID_TEMPLATE))
    );
    assert!(commands.lines().any(|command| command.contains("-r bbbb-")));
    assert!(!commands.contains("parents("));
}

#[cfg(unix)]
#[test]
fn resolves_two_revisions_to_immutable_source_expansion_endpoints() {
    let temp = tempdir().unwrap();
    let log = temp.path().join("commands.log");
    let binary = fake_jj(&log);
    let context = JujutsuCommandContext {
        cwd: temp.path().into(),
        jj_executable: binary.path().join("jj"),
    };
    let mut input = diff_input();
    let endpoints = VcsRangeEndpoints {
        from: "main".into(),
        to: "feature".into(),
    };
    input.range_endpoints = Some(endpoints.clone());
    assert_eq!(
        resolve_jj_range_endpoints(&input, &endpoints, &context).unwrap(),
        Some(JujutsuDiffEndpoints {
            new_commit_id: "2222".into(),
            old_commit_ids: vec!["1111".into()],
        })
    );
}

#[cfg(unix)]
#[test]
fn omits_endpoints_when_a_revset_resolves_to_multiple_revisions() {
    let temp = tempdir().unwrap();
    let log = temp.path().join("commands.log");
    let binary = fake_jj(&log);
    let context = JujutsuCommandContext {
        cwd: temp.path().into(),
        jj_executable: binary.path().join("jj"),
    };
    assert_eq!(
        resolve_jj_diff_endpoints(&JujutsuBackedInput::Diff(diff_input()), "multi", &context)
            .unwrap(),
        None
    );
}

#[cfg(unix)]
#[test]
fn retains_every_merge_parent_instead_of_choosing_one() {
    let temp = tempdir().unwrap();
    let log = temp.path().join("commands.log");
    let binary = fake_jj(&log);
    let context = JujutsuCommandContext {
        cwd: temp.path().into(),
        jj_executable: binary.path().join("jj"),
    };
    assert_eq!(
        resolve_jj_diff_endpoints(&JujutsuBackedInput::Diff(diff_input()), "merge", &context)
            .unwrap(),
        Some(JujutsuDiffEndpoints {
            new_commit_id: "dddd".into(),
            old_commit_ids: vec!["aaaa".into(), "cccc".into()],
        })
    );
}

#[test]
fn reports_a_friendly_error_for_ambiguous_change_id_prefixes() {
    let mut diff = diff_input();
    diff.range = Some("a".into());
    let translated = translate_jj_exit_failure(
        &JujutsuBackedInput::Diff(diff),
        "Error: change ID prefix is ambiguous",
    );
    assert_eq!(
        translated.to_string(),
        "`workdeck diff a` could not resolve Jujutsu revset `a`."
    );
}

#[test]
fn parser_accepts_only_nonempty_full_hexadecimal_commit_id_lines() {
    assert_eq!(
        parse_jj_commit_ids("abcd\nnot-an-id\n  0123EF  \n\n"),
        ["abcd", "0123EF"]
    );
}
