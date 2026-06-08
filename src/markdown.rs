//! Render user-authored Markdown (label wiki pages) to HTML.
//!
//! Wiki content is written by authenticated users but rendered into
//! pages other users view, so we render with comrak's safe defaults
//! (`render.unsafe_ = false`): raw HTML in the source is escaped rather
//! than passed through, and dangerous URL schemes (javascript:, etc.)
//! are filtered. We additionally enable a few GFM niceties that are
//! handy in a wiki.
//!
//! Wiki-links: `[[some-label]]` (or `[[some-label|display text]]`)
//! becomes a link to that label's filtered view. We parse to comrak's
//! AST and rewrite each wiki-link's target to `/?label=…`; doing it on
//! the AST means `[[…]]` inside a code span/block is left alone, since
//! it never parses as a wiki-link in the first place.

use comrak::nodes::{AstNode, NodeValue};
use comrak::{format_html, parse_document, Arena, Options};

/// Render Markdown to sanitised HTML. Raw HTML is escaped, so the output
/// is safe to embed with Askama's `|safe`.
pub fn render(source: &str) -> String {
    let mut options = Options::default();
    // GFM extras that read naturally in a wiki.
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    // [[label]] / [[label|title]] → wiki-link nodes, retargeted below.
    options.extension.wikilinks_title_after_pipe = true;
    // unsafe_ stays false (the default): raw inline/block HTML is escaped
    // and unsafe link protocols are stripped.

    let arena = Arena::new();
    let root = parse_document(&arena, source, &options);
    retarget_wikilinks(root);

    let mut out = String::new();
    if format_html(root, &options, &mut out).is_err() {
        return String::new();
    }
    out
}

/// Point every wiki-link at the label-filter URL for its target.
fn retarget_wikilinks<'a>(node: &'a AstNode<'a>) {
    for child in node.children() {
        retarget_wikilinks(child);
    }
    let mut ast = node.data.borrow_mut();
    if let NodeValue::WikiLink(link) = &mut ast.value {
        link.url = label_filter_url(&link.url);
    }
}

/// `some-label` → `/?label=some-label`, percent-encoding the target so
/// the result is always a valid URL. Matching is case-insensitive
/// server-side, so the target is passed through as written.
fn label_filter_url(target: &str) -> String {
    let mut encoded = String::with_capacity(target.len());
    for &b in target.trim().as_bytes() {
        let c = b as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            encoded.push(c);
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    format!("/?label={encoded}")
}

#[cfg(test)]
mod tests {
    use super::render;

    #[test]
    fn renders_basic_markdown() {
        let html = render("# Title\n\nSome **bold** text.");
        assert!(html.contains("<h1>"));
        assert!(html.contains("<strong>bold</strong>"));
    }

    #[test]
    fn escapes_raw_html_and_scripts() {
        let html = render("Hello <script>alert(1)</script> world");
        // The script tag must not survive as live HTML.
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn strips_javascript_links() {
        let html = render("[click](javascript:alert(1))");
        assert!(!html.contains("javascript:"));
    }

    #[test]
    fn wikilinks_become_label_filter_links() {
        let html = render("See the [[verse-1]] section.");
        assert!(html.contains(r#"href="/?label=verse-1""#), "got: {html}");
        // Default link text is the target.
        assert!(html.contains(">verse-1</a>"));
    }

    #[test]
    fn wikilinks_support_custom_text() {
        let html = render("[[chorus|the big chorus]]");
        assert!(html.contains(r#"href="/?label=chorus""#), "got: {html}");
        assert!(html.contains(">the big chorus</a>"));
    }

    #[test]
    fn wikilinks_in_code_are_left_alone() {
        let html = render("Type `[[verse]]` to link.");
        assert!(html.contains("<code>[[verse]]</code>"));
        assert!(
            !html.contains("href="),
            "code span must not become a link: {html}"
        );
    }
}
