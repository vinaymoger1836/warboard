//! Real names that must not appear in content, scenarios, code or docs (doc 11
//! §3.1, CLAUDE.md rule 6). The list lives in `content/denylist.txt`: one term
//! per line, `#` comments. A term matches without regard to case and only as a
//! whole word: the characters on either side are not letters or digits.
//!
//! The content check applies it to every entry file; `wb content denylist`
//! applies it to whole trees in CI.

/// File name of the list at the root of a content folder.
pub const FILE: &str = "denylist.txt";

#[derive(Debug, Clone, Default)]
pub struct Denylist {
    /// (term as written, lower-case form)
    terms: Vec<(String, String)>,
}

/// One occurrence of a denied term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// 1-based line number.
    pub line: usize,
    /// The term as written in the list.
    pub term: String,
}

impl Denylist {
    pub fn parse(text: &str) -> Self {
        let mut terms: Vec<(String, String)> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| (l.to_owned(), l.to_lowercase()))
            .collect();
        terms.sort_by(|a, b| a.1.cmp(&b.1));
        terms.dedup_by(|a, b| a.1 == b.1);
        Self { terms }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn len(&self) -> usize {
        self.terms.len()
    }

    /// Every denied term in `text`, by line, in line then list order.
    pub fn find(&self, text: &str) -> Vec<Hit> {
        let mut hits = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let lower = line.to_lowercase();
            for (term, low) in &self.terms {
                if contains_word(&lower, low) {
                    hits.push(Hit {
                        line: i + 1,
                        term: term.clone(),
                    });
                }
            }
        }
        hits
    }
}

fn is_word_char(c: Option<char>) -> bool {
    c.is_some_and(char::is_alphanumeric)
}

fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(pos) = haystack[from..].find(needle) {
        let start = from + pos;
        let end = start + needle.len();
        let before = haystack[..start].chars().next_back();
        let after = haystack[end..].chars().next();
        if !is_word_char(before) && !is_word_char(after) {
            return true;
        }
        from = start + needle.chars().next().map_or(1, char::len_utf8);
    }
    false
}
