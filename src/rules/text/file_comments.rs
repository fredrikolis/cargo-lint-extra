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
#[allow(clippy::unwrap_used)]
#[path = "file_comments_tests.rs"]
mod tests;
