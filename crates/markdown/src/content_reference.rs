//! Compatibility with content references emitted by agent skills.
//!
//! Keep transport-specific spelling here, not in a frontend or a harness:
//! skills can emit the same text through different providers. This module
//! never touches the filesystem. The original message remains authoritative.
use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag};

const CITATION: &str = ":codex-file-citation{";
const PLACEHOLDER: &str = "`\u{fffc}`";
const MAX_REFERENCE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReference {
    pub path: String,
}

impl FileReference {
    pub fn label(&self) -> &str {
        self.path.rsplit(['/', '\\']).next().unwrap_or(&self.path)
    }
}

/// Decode one complete directive. Unknown attributes are preserved in the
/// original message but do not grant extra behavior (e.g. page navigation).
pub fn file_reference(text: &str) -> Option<FileReference> {
    let mut rest = text.strip_prefix(CITATION)?.strip_suffix('}')?.trim();
    let mut path = None;
    let mut purpose = None;
    while !rest.is_empty() {
        let end = rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_')?;
        let key = &rest[..end];
        if key.is_empty() {
            return None;
        }
        rest = rest[end..].trim_start().strip_prefix('=')?.trim_start();
        let quote = rest.chars().next()?;
        if !matches!(quote, '"' | '\'') {
            return None;
        }
        rest = &rest[1..];
        let mut value = String::new();
        let mut chars = rest.char_indices();
        let consumed = loop {
            let (at, c) = chars.next()?;
            if c == quote {
                break at + 1;
            }
            if c == '\\' {
                let (_, escaped) = chars.next()?;
                if escaped != quote && escaped != '\\' {
                    return None;
                }
                value.push(escaped);
            } else if c.is_control() {
                return None;
            } else {
                value.push(c);
            }
        };
        match key {
            "path" if path.is_none() => path = Some(value),
            "purpose" if purpose.is_none() && matches!(value.as_str(), "source" | "output") => {
                purpose = Some(value);
            }
            "path" | "purpose" => return None,
            _ => {}
        }
        rest = &rest[consumed..];
        if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
            return None;
        }
        rest = rest.trim_start();
    }
    let path = path?;
    let absolute = path.starts_with('/')
        || (path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && path.as_bytes().get(1) == Some(&b':')
            && path
                .as_bytes()
                .get(2)
                .is_some_and(|c| matches!(c, b'/' | b'\\')));
    (absolute && !path.ends_with(['/', '\\']) && path.len() > 1).then_some(FileReference { path })
}

struct Replacement {
    source: Range<usize>,
    shown: Range<usize>,
}

/// Opaque code spans protect reference payloads from CommonMark punctuation.
/// Only spans introduced by this rewrite are decoded again by the parser.
pub(crate) struct Rewritten<'a> {
    pub text: String,
    source: &'a str,
    replacements: Vec<Replacement>,
}

impl Rewritten<'_> {
    pub fn original_range(&self, range: Range<usize>) -> Range<usize> {
        self.original(range.start, false)..self.original(range.end, true)
    }

    fn original(&self, at: usize, end: bool) -> usize {
        let mut delta = 0isize;
        for replacement in &self.replacements {
            if at <= replacement.shown.start {
                break;
            }
            if at < replacement.shown.end {
                return if end {
                    replacement.source.end
                } else {
                    replacement.source.start
                };
            }
            delta = replacement.source.end as isize - replacement.shown.end as isize;
        }
        at.saturating_add_signed(delta)
    }

    pub fn reference(&self, range: &Range<usize>) -> Option<&str> {
        let ix = self
            .replacements
            .partition_point(|r| r.shown.start < range.start);
        let replacement = self.replacements.get(ix)?;
        (replacement.shown == *range).then(|| &self.source[replacement.source.clone()])
    }
}

pub(crate) fn rewrite(source: &str) -> Option<Rewritten<'_>> {
    if !source.contains(CITATION) {
        return None;
    }
    // Ask the same CommonMark parser which regions are literal. In particular,
    // an unclosed code fence must stay literal throughout streaming.
    let protected: Vec<_> = Parser::new_ext(source, super::parser::options())
        .into_offset_iter()
        .filter_map(|(event, range)| match event {
            Event::Code(_)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::Start(
                Tag::CodeBlock(_) | Tag::HtmlBlock | Tag::Link { .. } | Tag::Image { .. },
            ) => Some(range),
            _ => None,
        })
        .collect();
    let mut text = String::new();
    let mut replacements = Vec::new();
    let mut copied = 0;
    let mut at = 0;
    while let Some(relative) = source[at..].find(CITATION) {
        let start = at + relative;
        at = start + CITATION.len();
        if protected.iter().any(|r| r.contains(&start))
            || source[..start]
                .bytes()
                .rev()
                .take_while(|b| *b == b'\\')
                .count()
                % 2
                != 0
        {
            continue;
        }
        let mut quoted = None;
        let mut escaped = false;
        let end = source[at..]
            .char_indices()
            .take_while(|(i, _)| *i < MAX_REFERENCE_BYTES)
            .find_map(|(i, c)| {
                if escaped {
                    escaped = false;
                    return None;
                }
                if c == '\\' && quoted.is_some() {
                    escaped = true;
                    return None;
                }
                if quoted == Some(c) {
                    quoted = None;
                } else if quoted.is_none() && matches!(c, '"' | '\'') {
                    quoted = Some(c);
                } else if quoted.is_none() && c == '}' {
                    return Some(at + i + 1);
                }
                None
            });
        let Some(end) = end else { continue };
        if file_reference(&source[start..end]).is_none() {
            continue;
        }
        text.push_str(&source[copied..start]);
        let shown_start = text.len();
        text.push_str(PLACEHOLDER);
        replacements.push(Replacement {
            source: start..end,
            shown: shown_start..text.len(),
        });
        copied = end;
        at = end;
    }
    if replacements.is_empty() {
        return None;
    }
    text.push_str(&source[copied..]);
    Some(Rewritten {
        text,
        source,
        replacements,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{Block, IncrementalParser, parse_full};

    fn links(source: &str) -> Vec<String> {
        let tree = parse_full(source);
        let Block::Paragraph { runs } = &tree.blocks[0].block else {
            panic!("paragraph")
        };
        runs.iter().filter_map(|r| r.style.link.clone()).collect()
    }

    #[test]
    fn citations_keep_literal_paths_and_readable_labels() {
        let source = r#"看过 :codex-file-citation{path="/tmp/空 格/a_*[x]`{y}.pdf" purpose="source"} 和 :codex-file-citation{purpose="output" path="/tmp/b.pdf"}。"#;
        assert_eq!(links(source), ["/tmp/空 格/a_*[x]`{y}.pdf", "/tmp/b.pdf"]);
        let tree = parse_full(source);
        let Block::Paragraph { runs } = &tree.blocks[0].block else {
            panic!()
        };
        assert_eq!(runs[1].style.file_label.as_deref(), Some("a_*[x]`{y}.pdf"));
        assert_eq!(runs.last().unwrap().text, "。");
    }

    #[test]
    fn examples_and_invalid_directives_are_not_links() {
        let citation = r#":codex-file-citation{path="/tmp/a.pdf" purpose="source"}"#;
        for source in [
            format!("`{citation}`"),
            format!("\\{citation}"),
            format!("[{citation}](https://example.com)"),
            citation.replace("/tmp/a.pdf", "javascript:alert(1)"),
            citation.replace("source", "bad"),
        ] {
            assert!(!links(&source).contains(&"/tmp/a.pdf".into()), "{source}");
        }
        assert!(rewrite(&format!("```text\n{citation}")).is_none());
        assert!(rewrite(&format!("    {citation}")).is_none());
    }

    #[test]
    fn streaming_matches_full_parse_at_every_character_boundary() {
        let source = "**资料** :codex-file-citation{path=\"/tmp/资料.pdf\" purpose=\"source\"}\n\n- [ ] 后续\n";
        let mut parser = IncrementalParser::new();
        for (i, c) in source.char_indices() {
            let end = i + c.len_utf8();
            parser.set_text(&source[..end]);
            assert_eq!(parser.tree(), &parse_full(&source[..end]), "{end}");
        }
        let tree = parse_full(source);
        let Block::List { items, .. } = &tree.blocks[1].block else {
            panic!()
        };
        let Block::Paragraph { runs } = &items[0][0] else {
            panic!()
        };
        let task = runs[0].style.task.as_ref().unwrap();
        assert_eq!(&source[task.range.clone()], "[ ]");
    }
}
