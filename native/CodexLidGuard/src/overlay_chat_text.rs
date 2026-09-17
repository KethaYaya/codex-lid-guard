//! Small, inert markdown renderer for the native conversation viewport.
//! Only emphasis, inline code and list markers are interpreted; no HTML or actions.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Style { Plain, Bold, Code, Muted, Notice }

pub(super) struct Fonts { pub body: Handle, pub bold: Handle, pub code: Handle, pub label: Handle, pub small: Handle }
impl Fonts {
    pub fn new(dpi: u32) -> Self {
        unsafe { Self { body: group_window::font(13, 400, dpi), bold: group_window::font(13, 600, dpi),
            code: CreateFontW(-scale_dip(12, dpi), 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0, wide("Consolas").as_ptr()),
            label: group_window::font(11, 600, dpi), small: group_window::font(12, 400, dpi) } }
    }
    pub fn get(&self, style: Style) -> Handle {
        match style { Style::Bold => self.bold, Style::Code => self.code, Style::Notice => self.small, _ => self.body }
    }
}
impl Drop for Fonts { fn drop(&mut self) { unsafe { for font in [self.body, self.bold, self.code, self.label, self.small] { DeleteObject(font); } } } }

#[derive(Debug)]
pub(super) struct Fragment { pub x: i32, pub y: i32, pub width: i32, pub text: Vec<u16>, pub style: Style }
#[derive(Default)]
pub(super) struct Text { pub fragments: Vec<Fragment>, pub height: i32, pub width: i32 }

// Unclosed delimiters remain literal while a reply is streaming.
fn inline(text: &str) -> Vec<(Style, String)> {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(escaped) = rest.strip_prefix('\\')
            && let Some(ch) = escaped.chars().next().filter(|ch| "\\*`_".contains(*ch)) {
            plain.push(ch); rest = &escaped[ch.len_utf8()..]; continue;
        }
        let delimiter = if rest.starts_with('`') { "`" } else if rest.starts_with("**") { "**" }
            else if rest.starts_with("__") { "__" } else { "" };
        if !delimiter.is_empty()
            && let Some(end) = rest[delimiter.len()..].find(delimiter).filter(|end| *end > 0) {
            if !plain.is_empty() { spans.push((Style::Plain, std::mem::take(&mut plain))); }
            let body = &rest[delimiter.len()..delimiter.len() + end];
            if delimiter == "`" { spans.push((Style::Code, body.into())); }
            else {
                // Code inside bold remains code; all other nested text is semibold.
                spans.extend(inline(body).into_iter().map(|(style, text)| (if style == Style::Code { style } else { Style::Bold }, text)));
            }
            rest = &rest[delimiter.len() * 2 + end..];
        } else {
            let ch = rest.chars().next().unwrap(); plain.push(ch); rest = &rest[ch.len_utf8()..];
        }
    }
    if !plain.is_empty() { spans.push((Style::Plain, plain)); }
    spans
}

fn list_item(line: &str) -> Option<(String, &str)> {
    let trimmed = line.trim_start();
    for marker in ["- ", "* ", "+ "] {
        if let Some(body) = trimmed.strip_prefix(marker) { return Some(("\u{2022}".into(), body.trim_start())); }
    }
    let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=9).contains(&digits) && (trimmed[digits..].starts_with(". ") || trimmed[digits..].starts_with(") ")) {
        Some((trimmed[..digits + 1].into(), trimmed[digits + 2..].trim_start()))
    } else { None }
}

pub(super) fn measure(dc: Handle, font: Handle, text: &str) -> i32 {
    unsafe {
        let old = SelectObject(dc, font);
        let text = wide(text);
        let mut rect = Rect { left: 0, top: 0, right: 0, bottom: 0 };
        DrawTextW(dc, text.as_ptr(), wide_text_length(&text), &mut rect, DT_SINGLELINE | DT_NOPREFIX | DT_CALCRECT);
        SelectObject(dc, old); rect.right
    }
}

impl Text {
    pub fn layout(dc: Handle, fonts: &Fonts, text: &str, width: i32, dpi: u32, role: Role) -> Self {
        let mut result = Self::default();
        let markdown = role == Role::Assistant;
        let d = |n| scale_dip(n, dpi);
        let line_height = d(20);
        let width = width.max(1);
        let mut y = 0;
        let mut gap = false;
        for line in text.lines() {
            if line.trim().is_empty() { gap = y > 0; continue; }
            if gap { y += d(8); gap = false; }
            let item = markdown.then(|| list_item(line)).flatten();
            let mut indent = 0;
            if let Some((marker, _)) = &item {
                let size = measure(dc, fonts.body, marker);
                indent = d(22).max(size + d(6)).min(width / 2);
                result.fragments.push(Fragment { x: 0, y, width: size, text: wide(marker), style: Style::Muted });
                result.width = result.width.max(size.min(width));
            }
            let spans = if markdown { inline(item.as_ref().map_or(line, |(_, body)| *body)) }
                else { vec![(if role == Role::Notice { Style::Notice } else { Style::Plain }, line.into())] };
            let mut x = indent;
            let mut pending_space = 0;
            for (style, span) in spans {
                // Keep inline code together when it fits; ordinary words wrap independently.
                let tokens: Vec<&str> = if style == Style::Code { vec![&span] }
                    else { span.split_inclusive(char::is_whitespace).collect() };
                for token in tokens {
                    let word = if style == Style::Code { token } else { token.trim_end() };
                    let suffix = &token[word.len()..];
                    if !word.is_empty() {
                        let pad = if style == Style::Code { d(5) } else { 0 };
                        let size = measure(dc, fonts.get(style), word) + 2 * pad;
                        if x > indent && x + pending_space + size > width { y += line_height; x = indent; }
                        if x > indent { x += pending_space; }
                        let mut remaining = word;
                        while !remaining.is_empty() {
                            let available = (width - x - 2 * pad).max(1);
                            let end = if measure(dc, fonts.get(style), remaining) <= available { remaining.len() } else {
                                let mut boundaries = remaining.char_indices().map(|(index, _)| index).skip(1).collect::<Vec<_>>();
                                boundaries.push(remaining.len());
                                let mut low = 0; let mut high = boundaries.len();
                                while low < high {
                                    let mid = low + (high - low) / 2;
                                    if measure(dc, fonts.get(style), &remaining[..boundaries[mid]]) <= available { low = mid + 1; }
                                    else { high = mid; }
                                }
                                boundaries[low.saturating_sub(1)]
                            };
                            let part = &remaining[..end];
                            let size = measure(dc, fonts.get(style), part) + 2 * pad;
                            result.fragments.push(Fragment { x, y, width: size, text: wide(part), style });
                            x += size; result.width = result.width.max(x.min(width));
                            remaining = &remaining[end..];
                            if !remaining.is_empty() { y += line_height; x = indent; }
                        }
                        pending_space = 0;
                    }
                    if !suffix.is_empty() { pending_space += measure(dc, fonts.body, " "); }
                }
            }
            y += line_height;
        }
        result.height = y;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markup_is_inert_and_incomplete_streaming_delimiters_stay_visible() {
        assert_eq!(inline("Use **bold** and `a * b`.").iter().map(|(s, t)| (*s, t.as_str())).collect::<Vec<_>>(),
            [(Style::Plain, "Use "), (Style::Bold, "bold"), (Style::Plain, " and "), (Style::Code, "a * b"), (Style::Plain, ".")]);
        assert_eq!(inline("**unfinished `code"), [(Style::Plain, "**unfinished `code".into())]);
        assert_eq!(inline(r"\*literal\* <b>HTML</b>"), [(Style::Plain, "*literal* <b>HTML</b>".into())]);
        assert_eq!(inline("**Run `cargo test` now**")[1], (Style::Code, "cargo test".into()));
        assert_eq!(list_item("  12. **Check** data"), Some(("12.".into(), "**Check** data")));
        assert_eq!(list_item("- item"), Some(("\u{2022}".into(), "item")));
        assert_eq!(list_item("3.14 is a number"), None);
    }
}
