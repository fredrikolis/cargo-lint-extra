#![allow(clippy::unwrap_used)]

mod test_helpers;

use cargo_lint_extra::config::{Config, FileCommentsConfig};
use cargo_lint_extra::diagnostic::RuleLevel;

fn config_with(file_comments: FileCommentsConfig) -> Config {
    let mut config = Config::default();
    config.rules.file_comments = file_comments;
    config
}

fn file_comments_diags(config: &Config) -> Vec<cargo_lint_extra::diagnostic::Diagnostic> {
    test_helpers::run_on_fixture("file_comments.rs", config)
        .into_iter()
        .filter(|d| d.rule == "file-comments")
        .collect()
}

#[test]
fn test_file_comments_disabled_by_default() {
    let diags = file_comments_diags(&Config::default());
    assert!(
        diags.is_empty(),
        "file-comments should be Allow by default, got: {diags:?}"
    );
}

#[test]
fn test_file_comments_ratio_detected() {
    let config = config_with(FileCommentsConfig {
        level: RuleLevel::Warn,
        ..FileCommentsConfig::default()
    });
    let diags = file_comments_diags(&config);
    assert_eq!(
        diags.len(),
        1,
        "only the ratio check is enabled by default, got: {diags:?}"
    );
    assert!(
        diags[0].message.contains("41%") && diags[0].message.contains("11/27"),
        "expected the measured counts in the message, got: {}",
        diags[0].message
    );
    assert_eq!(
        diags[0].line, None,
        "the ratio is a whole-file fact and carries no line"
    );
}

#[test]
fn test_file_comments_consecutive_runs_detected() {
    let config = config_with(FileCommentsConfig {
        level: RuleLevel::Warn,
        max_ratio: 0.0,
        max_consecutive: 3,
        max_doc_consecutive: 4,
        ..FileCommentsConfig::default()
    });
    let diags = file_comments_diags(&config);
    assert_eq!(diags.len(), 2, "one plain run, one doc run: {diags:?}");
    let plain = diags
        .iter()
        .find(|d| !d.message.contains("doc-comment"))
        .unwrap();
    assert_eq!(plain.line, Some(16), "the plain run ends on line 16");
    assert!(
        plain.message.contains("4 consecutive"),
        "got: {}",
        plain.message
    );
    let doc = diags
        .iter()
        .find(|d| d.message.contains("doc-comment"))
        .unwrap();
    assert_eq!(doc.line, Some(25), "the doc run ends on line 25");
    assert!(
        doc.message.contains("5 consecutive"),
        "got: {}",
        doc.message
    );
}

#[test]
fn test_file_comments_fenced_doctest_is_not_prose() {
    let config = config_with(FileCommentsConfig {
        level: RuleLevel::Warn,
        max_ratio: 0.0,
        max_doc_consecutive: 5,
        ..FileCommentsConfig::default()
    });
    let diags = file_comments_diags(&config);
    assert!(
        diags.is_empty(),
        "add's 8-line doc block is 2 lines of prose around a fenced example, got: {diags:?}"
    );
}
