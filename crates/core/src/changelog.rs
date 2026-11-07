//! What changed in a version, as `CHANGELOG/v<version>_CHANGELOGS*.md` says it, read into what the
//! main window shows: headings, items of lists (one in another), and paragraphs. Markdown beyond
//! that is shown as its text: emphasis and code without their marks, a link as its words.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// `#` to `######`.
    Heading { level: u8, text: String },
    /// `-`, `*` or `+`, or `1.`; `depth` 0 for an item of the outermost list, 1 for one in it...
    /// (An ordered item keeps its number in its text.)
    Item { depth: u8, text: String },
    Paragraph(String),
}

pub fn blocks(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    // Lines go on the block they continue: an item's, or a paragraph's.
    let mut open: Option<Block> = None;
    let close = |open: &mut Option<Block>, blocks: &mut Vec<Block>| {
        if let Some(block) = open.take() {
            blocks.push(block);
        }
    };
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            close(&mut open, &mut blocks);
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 && let Some((level, text)) = heading(trimmed) {
            close(&mut open, &mut blocks);
            blocks.push(Block::Heading {
                level,
                text: inline(text),
            });
        } else if let Some(text) = item(trimmed) {
            close(&mut open, &mut blocks);
            open = Some(Block::Item {
                // (Two spaces or a tab a level, as editors indent.)
                depth: (line[..indent].replace('\t', "  ").len() / 2).min(8) as u8,
                text: inline(text),
            });
        } else {
            match &mut open {
                Some(Block::Item { text, .. }) | Some(Block::Paragraph(text)) => {
                    text.push(' ');
                    text.push_str(&inline(trimmed));
                }
                _ => open = Some(Block::Paragraph(inline(trimmed))),
            }
        }
    }
    close(&mut open, &mut blocks);
    blocks
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let level = line.bytes().take_while(|byte| *byte == b'#').count();
    let text = line[level..].strip_prefix(' ')?;
    (1..=6).contains(&level).then(|| (level as u8, text.trim()))
}

/// The text of a line that is an item of a list (`- `, `* `, `+ `), or of an ordered one (`1. `,
/// which keeps its number).
fn item(line: &str) -> Option<&str> {
    for mark in ["- ", "* ", "+ "] {
        if let Some(text) = line.strip_prefix(mark) {
            return Some(text.trim());
        }
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    (digits > 0 && line[digits..].starts_with(". ")).then_some(line)
}

/// A line's words without Markdown's marks: `**`, `__`, `` ` ``, and `[words](address)` as its words.
fn inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find("](").and_then(|middle| {
            after[middle + 2..]
                .find(')')
                .map(|end| (middle, middle + 2 + end))
        }) {
            Some((middle, end)) => {
                out.push_str(&after[..middle]);
                rest = &after[end + 1..];
            }
            None => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.replace("**", "").replace("__", "").replace('`', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_items_and_paragraphs_are_read() {
        let read = blocks(
            "## v0.2.0 — 2026-11-08\n\n\
             Perora を **GPUI** で書き直しました。\n\
             軽くなりました。\n\n\
             ### 変わったこと\n\
             - 待機中のメモリーが減った\n\
             \x20 - 622 MB から 38 MB\n\
             - `Ctrl + Enter` でアップロード\n\
             \x20 続きの行\n\
             1. 番号つき\n",
        );
        assert_eq!(
            read,
            vec![
                Block::Heading {
                    level: 2,
                    text: "v0.2.0 — 2026-11-08".into()
                },
                Block::Paragraph("Perora を GPUI で書き直しました。 軽くなりました。".into()),
                Block::Heading {
                    level: 3,
                    text: "変わったこと".into()
                },
                Block::Item {
                    depth: 0,
                    text: "待機中のメモリーが減った".into()
                },
                Block::Item {
                    depth: 1,
                    text: "622 MB から 38 MB".into()
                },
                Block::Item {
                    depth: 0,
                    text: "Ctrl + Enter でアップロード 続きの行".into()
                },
                Block::Item {
                    depth: 0,
                    text: "1. 番号つき".into()
                },
            ]
        );
    }

    #[test]
    fn links_are_their_words_and_other_marks_their_text() {
        assert_eq!(
            inline("see [the release](https://example.com/a) and [this]"),
            "see the release and [this]"
        );
        assert_eq!(inline("#hashtag"), "#hashtag");
        assert_eq!(blocks("#not a heading")[0], Block::Paragraph("#not a heading".into()));
        assert!(blocks("").is_empty());
    }
}
