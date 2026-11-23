//! Light markdown styling painted over the textarea after it renders, plus list helpers.
//! Only foreground colours and modifiers are touched, so the cursor and selection stay visible.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui_textarea::TextArea;
use unicode_width::UnicodeWidthChar;

fn dim() -> Style {
    Style::new().fg(Color::DarkGray)
}

/// Recolour the visible text of `ta`, which was just rendered into `area`.
pub fn paint(buf: &mut Buffer, area: Rect, ta: &TextArea) {
    if ta.is_empty() {
        return; // placeholder text
    }
    let lines = ta.lines();
    let fences = fence_state(lines);
    let (top, _) = ta.scroll_offset();
    let mut styled: Option<(usize, Vec<Style>)> = None;
    let mut prev = None;
    for y in 0..area.height {
        let start = ta.screen_to_data((top + y) as usize, 0);
        let start = (start.0, start.1);
        if prev == Some(start) {
            break; // past the last display line (screen_to_data clamps)
        }
        prev = Some(start);
        let (row, col) = start;
        if styled.as_ref().is_none_or(|(r, _)| *r != row) {
            styled = Some((row, line_styles(&lines[row], fences[row])));
        }
        let styles = &styled.as_ref().unwrap().1;
        // Where the next display row starts, if this line wraps there.
        let next = ta.screen_to_data((top + y + 1) as usize, 0);
        let end = if next.0 == row && next.1 > col {
            next.1
        } else {
            usize::MAX
        };
        let mut x = area.x;
        for (i, ch) in lines[row]
            .chars()
            .enumerate()
            .skip(col)
            .take_while(|(i, _)| *i < end)
        {
            let w = ch.width().unwrap_or(0) as u16;
            if x + w > area.right() {
                break;
            }
            buf[(x, area.y + y)].set_style(styles[i]);
            x += w;
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Fence {
    None,
    Marker,
    Inside,
}

fn fence_state(lines: &[String]) -> Vec<Fence> {
    let mut inside = false;
    lines
        .iter()
        .map(|l| {
            if l.trim_start().starts_with("```") {
                inside = !inside;
                Fence::Marker
            } else if inside {
                Fence::Inside
            } else {
                Fence::None
            }
        })
        .collect()
}

/// One style per char of `line`.
fn line_styles(line: &str, fence: Fence) -> Vec<Style> {
    let chars: Vec<char> = line.chars().collect();
    let mut st = vec![Style::new(); chars.len()];
    let fill = |st: &mut [Style], from: usize, to: usize, s: Style| {
        for c in &mut st[from.min(to)..to] {
            *c = c.patch(s);
        }
    };
    let n = chars.len();
    match fence {
        Fence::Marker => {
            fill(&mut st, 0, n, dim());
            return st;
        }
        Fence::Inside => {
            fill(&mut st, 0, n, Style::new().fg(Color::Yellow));
            return st;
        }
        Fence::None => {}
    }

    let indent = chars.iter().take_while(|c| c.is_whitespace()).count();
    let rest: String = chars[indent..].iter().collect();

    // Headings
    let hashes = rest.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && rest.chars().nth(hashes).is_none_or(|c| c == ' ') {
        let color = if hashes == 1 {
            Color::Cyan
        } else {
            Color::Blue
        };
        fill(
            &mut st,
            0,
            n,
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        );
        fill(&mut st, indent, indent + hashes, dim());
        return st;
    }
    // Horizontal rule
    if ["---", "***", "___"].contains(&rest.trim_end()) {
        fill(&mut st, 0, n, dim());
        return st;
    }
    // Quote
    if rest.starts_with('>') {
        fill(
            &mut st,
            0,
            n,
            Style::new().fg(Color::Gray).add_modifier(Modifier::ITALIC),
        );
        fill(&mut st, indent, indent + 1, dim());
    }
    // List marker and checkbox
    let mut body = indent;
    if let Some(p) = ListPrefix::parse(line) {
        fill(&mut st, indent, p.marker_end, Style::new().fg(Color::Cyan));
        body = p.len;
        if let Some(checked) = p.checked {
            let (from, to) = (p.marker_end + 1, p.marker_end + 4);
            if checked {
                fill(&mut st, from, to, Style::new().fg(Color::Green));
                fill(&mut st, to, n, dim());
            } else {
                fill(&mut st, from, to, Style::new().fg(Color::Cyan));
            }
        }
    }
    inline(&chars, body, &mut st);
    st
}

/// `code`, **bold**, *italic* / _italic_, [link](url).
fn inline(chars: &[char], from: usize, st: &mut [Style]) {
    let find = |start: usize, pat: &[char]| -> Option<usize> {
        (start..chars.len().saturating_sub(pat.len() - 1)).find(|&j| chars[j..].starts_with(pat))
    };
    let mut patch = |a: usize, b: usize, s: Style| {
        for c in &mut st[a..b] {
            *c = c.patch(s);
        }
    };
    let mut i = from;
    while i < chars.len() {
        let c = chars[i];
        if c == '`' {
            if let Some(end) = find(i + 1, &['`']) {
                patch(i, end + 1, Style::new().fg(Color::Yellow));
                i = end + 1;
                continue;
            }
        } else if c == '*' && chars.get(i + 1) == Some(&'*') {
            if let Some(end) = find(i + 2, &['*', '*']).filter(|&e| e > i + 2) {
                patch(i + 2, end, Style::new().add_modifier(Modifier::BOLD));
                patch(i, i + 2, dim());
                patch(end, end + 2, dim());
                i = end + 2;
                continue;
            }
        } else if (c == '*' || c == '_')
            && chars.get(i + 1).is_some_and(|n| !n.is_whitespace())
            && (c == '*' || i == 0 || !chars[i - 1].is_alphanumeric())
        {
            if let Some(end) = find(i + 1, &[c]).filter(|&e| !chars[e - 1].is_whitespace()) {
                patch(i + 1, end, Style::new().add_modifier(Modifier::ITALIC));
                patch(i, i + 1, dim());
                patch(end, end + 1, dim());
                i = end + 1;
                continue;
            }
        } else if c == '['
            && let Some(mid) = find(i + 1, &[']', '('])
            && let Some(end) = find(mid + 2, &[')'])
        {
            patch(
                i + 1,
                mid,
                Style::new()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::UNDERLINED),
            );
            patch(i, i + 1, dim());
            patch(mid, end + 1, dim());
            i = end + 1;
            continue;
        }
        i += 1;
    }
}

/// The `  - [ ] ` / `1. ` prefix of a list line. Positions are char indices.
pub struct ListPrefix {
    pub indent: usize,
    /// End of the bullet / number marker.
    pub marker_end: usize,
    /// Some(checked) when the item has a `[ ]` / `[x]` box right after the marker.
    pub checked: Option<bool>,
    /// Length of the whole prefix, including the trailing space.
    pub len: usize,
    number: Option<u64>,
    marker: char,
}

impl ListPrefix {
    pub fn parse(line: &str) -> Option<ListPrefix> {
        let chars: Vec<char> = line.chars().collect();
        let indent = chars.iter().take_while(|&&c| c == ' ' || c == '\t').count();
        let digits = chars[indent..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        let (marker_end, number, marker) = if digits > 0 {
            let m = *chars.get(indent + digits)?;
            if m != '.' && m != ')' {
                return None;
            }
            let n: String = chars[indent..indent + digits].iter().collect();
            (indent + digits + 1, n.parse().ok(), m)
        } else {
            let m = *chars.get(indent)?;
            if !matches!(m, '-' | '*' | '+') {
                return None;
            }
            (indent + 1, None, m)
        };
        if chars.get(marker_end) != Some(&' ') {
            return None;
        }
        let after = &chars[marker_end + 1..];
        let checked = match after {
            ['[', ' ', ']', ' ', ..] | ['[', ' ', ']'] => Some(false),
            ['[', 'x' | 'X', ']', ' ', ..] | ['[', 'x' | 'X', ']'] => Some(true),
            _ => None,
        };
        let len = match checked {
            Some(_) => (marker_end + 5).min(chars.len()),
            None => marker_end + 1,
        };
        Some(ListPrefix {
            indent,
            marker_end,
            checked,
            len,
            number,
            marker,
        })
    }

    /// Prefix for the next item: same indent and marker, next number, empty checkbox.
    pub fn continuation(&self, line: &str) -> String {
        let indent: String = line.chars().take(self.indent).collect();
        let marker = match self.number {
            Some(n) => format!("{}{}", n + 1, self.marker),
            None => self.marker.to_string(),
        };
        let checkbox = if self.checked.is_some() { "[ ] " } else { "" };
        format!("{indent}{marker} {checkbox}")
    }
}

/// Ticks or unticks the checkbox on `line` (1-based) of `text`, outside code blocks. With
/// `expect`, the line must still read that, or be the only checkbox line that does (the note may
/// have changed since the caller looked). Returns the new text and whether the box is now ticked.
pub fn toggle_checkbox(
    text: &str,
    line: usize,
    expect: Option<&str>,
) -> Result<(String, bool), String> {
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let fences = fence_state(&lines);
    let is_box = |i: usize, l: &str| {
        fences[i] == Fence::None && ListPrefix::parse(l).is_some_and(|p| p.checked.is_some())
    };
    let at = line
        .checked_sub(1)
        .filter(|&i| i < lines.len() && is_box(i, &lines[i]));
    let target = match expect {
        Some(e) if at.is_none_or(|i| lines[i] != e) => {
            let same: Vec<usize> = (0..lines.len())
                .filter(|&i| lines[i] == e && is_box(i, &lines[i]))
                .collect();
            match same.as_slice() {
                [only] => *only,
                [] => return Err("that checkbox is no longer in the note".into()),
                _ => return Err("the note changed and that checkbox is ambiguous now".into()),
            }
        }
        _ => at.ok_or_else(|| format!("line {line} has no checkbox"))?,
    };
    let p = ListPrefix::parse(&lines[target]).unwrap();
    let checked = p.checked.unwrap();
    let mut chars: Vec<char> = lines[target].chars().collect();
    chars[p.marker_end + 2] = if checked { ' ' } else { 'x' };
    lines[target] = chars.into_iter().collect();
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    Ok((out, !checked))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_checkboxes_by_line_or_text() {
        let text = "# t\n- [ ] a\n```\n- [ ] code\n```\n  1. [X] b\n";
        assert_eq!(
            toggle_checkbox(text, 2, None).unwrap(),
            (text.replace("- [ ] a", "- [x] a"), true)
        );
        assert_eq!(
            toggle_checkbox(text, 6, None).unwrap().0,
            text.replace("[X] b", "[ ] b")
        );
        assert!(
            toggle_checkbox(text, 4, None).is_err(),
            "inside a code block"
        );
        assert!(toggle_checkbox(text, 1, None).is_err(), "not a list item");
        let moved = format!("new line\n{text}");
        assert!(toggle_checkbox(&moved, 2, Some("- [ ] a")).unwrap().1);
        assert!(toggle_checkbox(&moved, 2, Some("- [ ] gone")).is_err());
        assert!(
            toggle_checkbox("- [ ] a\n- [ ] a\nx\n", 3, Some("- [ ] a")).is_err(),
            "ambiguous"
        );
    }

    #[test]
    fn list_prefixes() {
        let p = ListPrefix::parse("  - [x] done").unwrap();
        assert_eq!(
            (p.indent, p.marker_end, p.checked, p.len),
            (2, 3, Some(true), 8)
        );
        assert_eq!(p.continuation("  - [x] done"), "  - [ ] ");
        let p = ListPrefix::parse("9. nine").unwrap();
        assert_eq!(p.continuation("9. nine"), "10. ");
        assert!(ListPrefix::parse("-not a list").is_none());
        assert!(ListPrefix::parse("**bold**").is_none());
        assert_eq!(ListPrefix::parse("- [ ]").unwrap().checked, Some(false));
    }

    #[test]
    fn inline_styles() {
        let st = line_styles("a `b` **c** *d* [e](f)", Fence::None);
        assert_eq!(st[3].fg, Some(Color::Yellow));
        assert!(st[8].add_modifier.contains(Modifier::BOLD));
        assert!(st[13].add_modifier.contains(Modifier::ITALIC));
        assert!(st[17].add_modifier.contains(Modifier::UNDERLINED));
        assert!(!st[0].add_modifier.contains(Modifier::BOLD));
    }
}
