use crate::diagnostic::{Diagnostic, RuleLevel};
use crate::rules::TextRule;
use serde::Deserialize;
use std::path::Path;

// --- Config ---
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub level: RuleLevel,
    /// Comment lines as a fraction of all non-blank lines. `0.0` disables the ratio check.
    pub max_ratio: f64,
    /// Whether `///` and `//!` count toward the ratio. `inline-comments` always excludes
    /// them; at file scope they are usually the larger share, so they are counted here.
    pub count_doc_comments: bool,
    /// Longest run of adjacent `//` lines allowed anywhere in the file, including outside
    /// function bodies where `inline-comments` cannot see. `0` disables.
    pub max_consecutive: usize,
    /// Longest run of adjacent `///` / `//!` lines allowed. Separate from `max_consecutive`
    /// because a doc block on a public item is legitimate where a `//` block is not. `0` disables.
    pub max_doc_consecutive: usize,
    /// Files with fewer than this many non-blank lines are skipped, so a stub or a
    /// single-line header file is not trivially "100% comments".
    pub min_lines: usize,
    /// Exclude a leading comment line (licence banner, ownership header) from the counts.
    /// It is a per-file fixed cost the body should not be charged for.
    pub skip_header: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            level: RuleLevel::Allow,
            max_ratio: 0.30,
            count_doc_comments: true,
            max_consecutive: 0,
            max_doc_consecutive: 0,
            min_lines: 10,
            skip_header: true,
        }
    }
}

// --- Test Override ---
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Override {
    pub level: Option<RuleLevel>,
    pub max_ratio: Option<f64>,
    pub count_doc_comments: Option<bool>,
    pub max_consecutive: Option<usize>,
    pub max_doc_consecutive: Option<usize>,
    pub min_lines: Option<usize>,
    pub skip_header: Option<bool>,
}

pub const fn apply_override(cfg: &mut Config, o: &Override) {
    if let Some(v) = o.level {
        cfg.level = v;
    }
    if let Some(v) = o.max_ratio {
        cfg.max_ratio = v;
    }
    if let Some(v) = o.count_doc_comments {
        cfg.count_doc_comments = v;
    }
    if let Some(v) = o.max_consecutive {
        cfg.max_consecutive = v;
    }
    if let Some(v) = o.max_doc_consecutive {
        cfg.max_doc_consecutive = v;
    }
    if let Some(v) = o.min_lines {
        cfg.min_lines = v;
    }
    if let Some(v) = o.skip_header {
        cfg.skip_header = v;
    }
}

// --- Rule ---
pub struct Rule {
    level: RuleLevel,
    max_ratio: f64,
    count_doc_comments: bool,
    max_consecutive: usize,
    max_doc_consecutive: usize,
    min_lines: usize,
    skip_header: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Blank,
    Doc,
    Plain,
    Code,
}

const fn classify(trimmed: &str) -> Kind {
    if trimmed.is_empty() {
        Kind::Blank
    } else if starts_with(trimmed, b"///") || starts_with(trimmed, b"//!") {
        Kind::Doc
    } else if starts_with(trimmed, b"//") {
        Kind::Plain
    } else {
        Kind::Code
    }
}

const fn starts_with(s: &str, prefix: &[u8]) -> bool {
    let b = s.as_bytes();
    if b.len() < prefix.len() {
        return false;
    }
    let mut i = 0;
    while i < prefix.len() {
        if b[i] != prefix[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Does this doc line open or close a fenced example block?
fn doc_fence(trimmed: &str) -> bool {
    let body = trimmed
        .trim_start_matches("///")
        .trim_start_matches("//!")
        .trim();
    body.starts_with("```")
}

/// A run of adjacent comment lines: its length and the line it ends on.
#[derive(Default, Clone, Copy)]
struct Run {
    len: usize,
    end: usize,
}

#[derive(Default)]
struct Counts {
    comments: usize,
    code: usize,
    worst_plain: Run,
    worst_doc: Run,
}

impl Rule {
    pub const fn new(config: &Config) -> Self {
        Self {
            level: config.level,
            max_ratio: config.max_ratio,
            count_doc_comments: config.count_doc_comments,
            max_consecutive: config.max_consecutive,
            max_doc_consecutive: config.max_doc_consecutive,
            min_lines: config.min_lines,
            skip_header: config.skip_header,
        }
    }

    /// Index of the first line to charge against the budget: past a `#!` shebang, past any
    /// leading blanks, and past one header comment when `skip_header` is set.
    fn body_start(&self, lines: &[&str]) -> usize {
        let mut i = 0;
        if lines.first().is_some_and(|l| l.starts_with("#!")) {
            i += 1;
        }
        while lines.get(i).is_some_and(|l| l.trim().is_empty()) {
            i += 1;
        }
        if self.skip_header
            && lines
                .get(i)
                .is_some_and(|l| classify(l.trim()) != Kind::Code)
        {
            i += 1;
        }
        i
    }

    fn measure(&self, lines: &[&str]) -> Counts {
        let mut c = Counts::default();
        let (mut plain, mut doc) = (0usize, 0usize);
        let mut in_doctest = false;
        for (offset, raw) in lines.iter().enumerate() {
            let mut kind = classify(raw.trim());
            let line_no = offset + 1;
            // A fenced block inside a doc comment is example CODE, not prose. Counting it as
            // prose would price a doctest — the thing rustdoc exists to encourage — as if it
            // were an essay, and no run cap could then tell the two apart.
            if kind == Kind::Doc && doc_fence(raw.trim()) {
                in_doctest = !in_doctest;
                kind = Kind::Code;
            } else if in_doctest && kind == Kind::Doc {
                kind = Kind::Code;
            }
            match kind {
                Kind::Doc => {
                    doc += 1;
                    plain = 0;
                    if doc > c.worst_doc.len {
                        c.worst_doc = Run {
                            len: doc,
                            end: line_no,
                        };
                    }
                    if self.count_doc_comments {
                        c.comments += 1;
                    } else {
                        c.code += 1;
                    }
                }
                Kind::Plain => {
                    plain += 1;
                    doc = 0;
                    if plain > c.worst_plain.len {
                        c.worst_plain = Run {
                            len: plain,
                            end: line_no,
                        };
                    }
                    c.comments += 1;
                }
                Kind::Code => {
                    plain = 0;
                    doc = 0;
                    c.code += 1;
                }
                Kind::Blank => {
                    plain = 0;
                    doc = 0;
                }
            }
        }
        c
    }
}

impl TextRule for Rule {
    fn name(&self) -> &'static str {
        "file-comments"
    }

    fn check_file(&self, content: &str, file: &Path) -> Vec<Diagnostic> {
        let all: Vec<&str> = content.lines().collect();
        let start = self.body_start(&all);
        let Some(body) = all.get(start..) else {
            return Vec::new();
        };
        let c = self.measure(body);
        let total = c.comments + c.code;
        if total < self.min_lines {
            return Vec::new();
        }

        let mut out = Vec::new();
        let mut report = |message: String, line: Option<usize>| {
            let d = Diagnostic::new(self.name(), self.level, message, file);
            out.push(match line {
                Some(l) => d.with_line(l + start),
                None => d,
            });
        };

        #[expect(
            clippy::cast_precision_loss,
            reason = "line counts are far below f64 precision"
        )]
        let ratio = c.comments as f64 / total as f64;
        if self.max_ratio > 0.0 && ratio > self.max_ratio {
            report(
                format!(
                    "file is {:.0}% comments ({}/{} lines), max allowed is {:.0}%",
                    ratio * 100.0,
                    c.comments,
                    total,
                    self.max_ratio * 100.0
                ),
                None,
            );
        }
        if self.max_consecutive > 0 && c.worst_plain.len > self.max_consecutive {
            report(
                format!(
                    "{} consecutive comment lines (max allowed {})",
                    c.worst_plain.len, self.max_consecutive
                ),
                Some(c.worst_plain.end),
            );
        }
        if self.max_doc_consecutive > 0 && c.worst_doc.len > self.max_doc_consecutive {
            report(
                format!(
                    "{} consecutive doc-comment lines (max allowed {})",
                    c.worst_doc.len, self.max_doc_consecutive
                ),
                Some(c.worst_doc.end),
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
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
        let doctest = "// header\n/// Does a thing.\n///\n/// ```\n/// let a = 1;\n/// let b = 2;\n/// let c = 3;\n/// let d = 4;\n/// let e = 5;\n/// ```\npub fn thing() {}\n".to_string()
            + &"let x = 1;\n".repeat(20);
        assert!(
            check(&r, &doctest).is_empty(),
            "3 lines of prose around a 5-line example is not a 10-line essay"
        );
        let essay =
            "// header\n".to_string() + &"/// prose\n".repeat(9) + &"let x = 1;\n".repeat(20);
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
}
