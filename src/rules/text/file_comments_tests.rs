use super::*;

fn rule(mut cfg: Config) -> Rule {
    cfg.level = RuleLevel::Deny;
    Rule::new(&cfg)
}

fn check(r: &Rule, content: &str) -> Vec<Diagnostic> {
    r.check_file(content, Path::new("test.rs"))
}

/// A file with an explicit header line, so `skip_header` has something to consume and the
/// `comments`/`code` counts below are exactly what the budget sees.
fn body(comments: usize, code: usize) -> String {
    String::from("// header\n") + &"// c\n".repeat(comments) + &"let x = 1;\n".repeat(code)
}

#[test]
fn a_file_under_the_ratio_passes() {
    let r = rule(Config::default());
    assert!(check(&r, &body(2, 18)).is_empty());
}

#[test]
fn a_file_over_the_ratio_is_reported_with_its_counts() {
    let r = rule(Config::default());
    let d = check(&r, &body(8, 12));
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].rule, "file-comments");
    assert!(d[0].message.contains("40%"), "{}", d[0].message);
    assert!(d[0].message.contains("8/20"), "{}", d[0].message);
    assert_eq!(d[0].line, None, "the ratio is a whole-file fact");
}

#[test]
fn a_short_file_is_skipped() {
    let r = rule(Config::default());
    assert!(check(&r, &body(5, 1)).is_empty());
}

#[test]
fn min_lines_counts_only_non_blank_lines() {
    let r = rule(Config::default());
    let padded = body(5, 5).replace('\n', "\n\n");
    assert_eq!(
        check(&r, &padded).len(),
        1,
        "10 non-blank lines still measure"
    );
}

#[test]
fn doc_comments_count_toward_the_ratio_by_default() {
    let r = rule(Config::default());
    let text = format!(
        "// header\n{}{}",
        "/// d\n".repeat(8),
        "let x = 1;\n".repeat(12)
    );
    assert_eq!(check(&r, &text).len(), 1);
}

#[test]
fn doc_comments_can_be_excluded_from_the_ratio() {
    let r = rule(Config {
        count_doc_comments: false,
        ..Config::default()
    });
    let text = format!(
        "// header\n{}{}",
        "/// d\n".repeat(8),
        "let x = 1;\n".repeat(12)
    );
    assert!(check(&r, &text).is_empty());
}

#[test]
fn a_plain_run_outside_a_function_body_is_caught() {
    let r = rule(Config {
        max_ratio: 0.0,
        max_consecutive: 1,
        ..Config::default()
    });
    let text = format!(
        "// header\n{}{}",
        "// m\n".repeat(4),
        "let x = 1;\n".repeat(20)
    );
    let d = check(&r, &text);
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("4 consecutive"), "{}", d[0].message);
}

#[test]
fn plain_and_doc_runs_have_independent_caps() {
    let r = rule(Config {
        max_ratio: 0.0,
        max_consecutive: 1,
        max_doc_consecutive: 4,
        ..Config::default()
    });
    let ok = format!(
        "// header\n{}{}",
        "/// d\n".repeat(4),
        "let x = 1;\n".repeat(20)
    );
    assert!(
        check(&r, &ok).is_empty(),
        "a 4-line doc block is legitimate"
    );
    let bad = format!(
        "// header\n{}{}",
        "/// d\n".repeat(5),
        "let x = 1;\n".repeat(20)
    );
    let d = check(&r, &bad);
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("doc-comment"), "{}", d[0].message);
}

#[test]
fn a_blank_line_breaks_a_run() {
    let r = rule(Config {
        max_ratio: 0.0,
        max_consecutive: 2,
        ..Config::default()
    });
    let text = format!(
        "// header\n{}{}",
        "// a\n// b\n\n// c\n// d\n",
        "let x = 1;\n".repeat(20)
    );
    assert!(check(&r, &text).is_empty());
}

#[test]
fn the_header_is_not_charged_to_the_body() {
    let text = format!(
        "// Copyright\n{}{}",
        "// c\n".repeat(6),
        "let x = 1;\n".repeat(14)
    );
    let skipped = rule(Config::default());
    assert!(check(&skipped, &text).is_empty(), "6/20 = 30%, not over");
    let counted = rule(Config {
        skip_header: false,
        ..Config::default()
    });
    let d = check(&counted, &text);
    assert_eq!(d.len(), 1, "7/21 = 33% once the header is charged");
}

#[test]
fn a_shebang_does_not_hide_the_header() {
    let r = rule(Config::default());
    let text = format!("#!/usr/bin/env rust\n{}", body(8, 12));
    let d = check(&r, &text);
    assert_eq!(
        d.len(),
        1,
        "the header is skipped, the 8 body comments are not"
    );
    assert!(d[0].message.contains("8/20"), "{}", d[0].message);
}

#[test]
fn reported_lines_are_absolute_in_the_file() {
    let r = rule(Config {
        max_ratio: 0.0,
        max_consecutive: 1,
        ..Config::default()
    });
    let text = format!(
        "// header\n\n{}{}",
        "// a\n// b\n",
        "let x = 1;\n".repeat(20)
    );
    let d = check(&r, &text);
    assert_eq!(
        d[0].line,
        Some(4),
        "the run ends on file line 4, not body line 2"
    );
}

#[test]
fn a_fenced_doctest_is_code_not_prose() {
    let r = rule(Config {
        max_ratio: 0.0,
        max_doc_consecutive: 4,
        ..Config::default()
    });
    let documented_fn = [
        "// header",
        "/// Does a thing.",
        "///",
        "/// ```",
        "/// let a = 1;",
        "/// let b = 2;",
        "/// let c = 3;",
        "/// let d = 4;",
        "/// let e = 5;",
        "/// ```",
        "pub fn thing() {}",
        "",
    ]
    .join("\n");
    let doctest = documented_fn + &"let x = 1;\n".repeat(20);
    assert!(
        check(&r, &doctest).is_empty(),
        "3 lines of prose around a 5-line example is not a 10-line essay"
    );
    let essay = "// header\n".to_string() + &"/// prose\n".repeat(9) + &"let x = 1;\n".repeat(20);
    assert_eq!(
        check(&r, &essay).len(),
        1,
        "9 lines of unfenced prose still trips"
    );
}

#[test]
fn a_zero_threshold_disables_its_check() {
    let r = rule(Config {
        max_ratio: 0.0,
        max_consecutive: 0,
        max_doc_consecutive: 0,
        ..Config::default()
    });
    assert!(check(&r, &body(19, 1)).is_empty());
}

#[test]
fn the_rule_is_off_unless_configured() {
    assert_eq!(Config::default().level, RuleLevel::Allow);
}
