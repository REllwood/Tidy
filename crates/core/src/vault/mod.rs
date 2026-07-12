//! Vault serialization: convert a page (BlockNote JSON body + metadata) to a
//! portable Markdown file with YAML frontmatter, and parse it back. Pure and
//! testable — the filesystem flusher/watcher runtime lives in the desktop app.
//!
//! Identity is the frontmatter `id`, never the path, so a Finder move/rename
//! re-links rather than orphaning.

use serde_json::Value;

pub mod export;

/// A filesystem-safe slug for a page title (used only for the *filename*, which
/// is cosmetic — identity is the frontmatter id).
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in title.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let s = out.trim_matches('-').to_string();
    if s.is_empty() {
        "untitled".to_string()
    } else {
        s.chars().take(60).collect()
    }
}

fn yaml_escape(s: &str) -> String {
    // Quote when the value could be misparsed; escape embedded quotes.
    if s.is_empty()
        || s.contains(['\n', '"', ':', '#', '\'', '[', ']', '{', '}'])
        || s.trim() != s
    {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// Metadata written to a page's frontmatter.
pub struct PageMeta<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub icon: Option<&'a str>,
    pub kind: &'a str,
    pub parent: Option<&'a str>,
    pub created: i64,
    pub updated: i64,
    pub tags: &'a [String],
}

/// Render YAML frontmatter for a page.
pub fn frontmatter(meta: &PageMeta) -> String {
    let mut s = String::from("---\n");
    s.push_str(&format!("id: {}\n", meta.id));
    s.push_str(&format!("title: {}\n", yaml_escape(meta.title)));
    if let Some(icon) = meta.icon {
        s.push_str(&format!("icon: {}\n", yaml_escape(icon)));
    }
    s.push_str(&format!("type: {}\n", meta.kind));
    if let Some(p) = meta.parent {
        s.push_str(&format!("parent: {p}\n"));
    }
    s.push_str(&format!("created: {}\n", meta.created));
    s.push_str(&format!("updated: {}\n", meta.updated));
    if !meta.tags.is_empty() {
        let tags = meta
            .tags
            .iter()
            .map(|t| yaml_escape(t))
            .collect::<Vec<_>>()
            .join(", ");
        s.push_str(&format!("tags: [{tags}]\n"));
    }
    s.push_str("---\n\n");
    s
}

/// A whole page rendered as a Markdown file.
pub fn render_page(meta: &PageMeta, body_markdown: &str) -> String {
    let mut s = frontmatter(meta);
    let body = body_markdown.trim_end();
    if !body.is_empty() {
        s.push_str(body);
        s.push('\n');
    }
    s
}

// ---- BlockNote JSON -> Markdown -------------------------------------------

/// Render a single inline content node (or string) to Markdown.
fn inline_to_md(node: &Value, out: &mut String) {
    if let Some(text) = node.as_str() {
        out.push_str(text);
        return;
    }
    match node.get("type").and_then(|t| t.as_str()) {
        Some("link") => {
            let href = node.get("href").and_then(|h| h.as_str()).unwrap_or("");
            let mut inner = String::new();
            if let Some(content) = node.get("content") {
                inlines_to_md(content, &mut inner);
            }
            out.push_str(&format!("[{inner}]({href})"));
        }
        _ => {
            let text = node.get("text").and_then(|t| t.as_str()).unwrap_or("");
            let styles = node.get("styles");
            let bold = styles.and_then(|s| s.get("bold")).and_then(|b| b.as_bool()).unwrap_or(false);
            let italic = styles.and_then(|s| s.get("italic")).and_then(|b| b.as_bool()).unwrap_or(false);
            let code = styles.and_then(|s| s.get("code")).and_then(|b| b.as_bool()).unwrap_or(false);
            let mut t = text.to_string();
            if code {
                t = format!("`{t}`");
            }
            if bold {
                t = format!("**{t}**");
            }
            if italic {
                t = format!("*{t}*");
            }
            out.push_str(&t);
        }
    }
}

/// Render a block's `content` (string or array of inline nodes) to Markdown.
fn inlines_to_md(content: &Value, out: &mut String) {
    match content {
        Value::String(s) => out.push_str(s),
        Value::Array(arr) => {
            for node in arr {
                inline_to_md(node, out);
            }
        }
        _ => {}
    }
}

fn block_to_md(block: &Value, depth: usize, ordinal: usize, out: &mut String) {
    let kind = block.get("type").and_then(|t| t.as_str()).unwrap_or("paragraph");
    let indent = "  ".repeat(depth);
    let mut text = String::new();
    if let Some(content) = block.get("content") {
        inlines_to_md(content, &mut text);
    }
    let props = block.get("props");

    match kind {
        "heading" => {
            let level = props
                .and_then(|p| p.get("level"))
                .and_then(|l| l.as_u64())
                .unwrap_or(1)
                .clamp(1, 6) as usize;
            out.push_str(&format!("{}{} {}\n\n", indent, "#".repeat(level), text));
        }
        "bulletListItem" => out.push_str(&format!("{indent}- {text}\n")),
        "numberedListItem" => out.push_str(&format!("{indent}{ordinal}. {text}\n")),
        "checkListItem" => {
            let checked = props
                .and_then(|p| p.get("checked"))
                .and_then(|c| c.as_bool())
                .unwrap_or(false);
            let mark = if checked { "x" } else { " " };
            out.push_str(&format!("{indent}- [{mark}] {text}\n"));
        }
        "quote" => out.push_str(&format!("{indent}> {text}\n\n")),
        "codeBlock" => {
            let lang = props
                .and_then(|p| p.get("language"))
                .and_then(|l| l.as_str())
                .unwrap_or("");
            out.push_str(&format!("```{lang}\n{text}\n```\n\n"));
        }
        _ => out.push_str(&format!("{indent}{text}\n\n")),
    }

    // Recurse into nested children (indented one level).
    if let Some(children) = block.get("children").and_then(|c| c.as_array()) {
        let mut ord = 0usize;
        for child in children {
            ord += 1;
            block_to_md(child, depth + 1, ord, out);
        }
    }
}

/// Convert a BlockNote document (a JSON array of blocks) to Markdown. Lossy by
/// design — the SQLite JSON stays authoritative; the `.md` is a best-effort
/// portable mirror.
pub fn blocks_to_markdown(blocks_json: &str) -> String {
    let parsed: Value = match serde_json::from_str(blocks_json) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    let arr = match parsed.as_array() {
        Some(a) => a,
        None => return String::new(),
    };
    let mut out = String::new();
    let mut ordinal = 0usize;
    for block in arr {
        let is_numbered = block.get("type").and_then(|t| t.as_str()) == Some("numberedListItem");
        if is_numbered {
            ordinal += 1;
        } else {
            ordinal = 0;
        }
        block_to_md(block, 0, ordinal, &mut out);
    }
    out.trim_end().to_string()
}

fn heading_level(s: &str) -> Option<usize> {
    let hashes = s.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && s.as_bytes().get(hashes) == Some(&b' ') {
        Some(hashes)
    } else {
        None
    }
}

fn split_numbered(s: &str) -> Option<&str> {
    let dot = s.find(". ")?;
    if s[..dot].chars().all(|c| c.is_ascii_digit()) && !s[..dot].is_empty() {
        Some(&s[dot + 2..])
    } else {
        None
    }
}

/// Parse Markdown back into a BlockNote document (a JSON array of blocks with
/// string content). The inverse of `blocks_to_markdown` — best-effort but
/// lossless for the *text*, so an external edit round-trips into the canonical
/// `content` column instead of being dropped.
pub fn markdown_to_blocks(md: &str) -> String {
    let md = md.replace("\r\n", "\n");
    let mut blocks: Vec<Value> = Vec::new();
    let mut in_code = false;
    let mut code_lang = String::new();
    let mut code_buf: Vec<String> = Vec::new();

    for raw in md.lines() {
        let ts = raw.trim_start();
        if let Some(rest) = ts.strip_prefix("```") {
            if !in_code {
                in_code = true;
                code_lang = rest.trim().to_string();
                code_buf.clear();
            } else {
                in_code = false;
                blocks.push(serde_json::json!({
                    "type": "codeBlock",
                    "props": { "language": code_lang },
                    "content": code_buf.join("\n"),
                }));
                code_lang.clear();
            }
            continue;
        }
        if in_code {
            code_buf.push(raw.to_string());
            continue;
        }
        let t = raw.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(level) = heading_level(t) {
            let text = t[level..].trim_start().to_string();
            blocks.push(serde_json::json!({"type":"heading","props":{"level":level},"content":text}));
        } else if let Some(rest) = t.strip_prefix("- [ ] ").or_else(|| t.strip_prefix("- [] ")) {
            blocks.push(serde_json::json!({"type":"checkListItem","props":{"checked":false},"content":rest}));
        } else if let Some(rest) = t.strip_prefix("- [x] ").or_else(|| t.strip_prefix("- [X] ")) {
            blocks.push(serde_json::json!({"type":"checkListItem","props":{"checked":true},"content":rest}));
        } else if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            blocks.push(serde_json::json!({"type":"bulletListItem","content":rest}));
        } else if let Some(rest) = split_numbered(t) {
            blocks.push(serde_json::json!({"type":"numberedListItem","content":rest}));
        } else if let Some(rest) = t.strip_prefix("> ") {
            blocks.push(serde_json::json!({"type":"quote","content":rest}));
        } else {
            blocks.push(serde_json::json!({"type":"paragraph","content":t}));
        }
    }
    if in_code && !code_buf.is_empty() {
        blocks.push(serde_json::json!({
            "type": "codeBlock",
            "props": { "language": code_lang },
            "content": code_buf.join("\n"),
        }));
    }
    if blocks.is_empty() {
        blocks.push(serde_json::json!({"type":"paragraph","content":""}));
    }
    Value::Array(blocks).to_string()
}

/// A plain-text projection of a document body (for FTS / previews).
pub fn blocks_to_plaintext(blocks_json: &str) -> String {
    let md = blocks_to_markdown(blocks_json);
    md.lines()
        .map(|l| l.trim_start_matches(['#', '-', '>', ' ', '*']).trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

// ---- Markdown -> frontmatter map + body (file -> app) ----------------------

/// Split a Markdown file into (frontmatter key→value map, body markdown).
/// Only the flat `key: value` frontmatter lines we write are parsed.
fn unquote(v: &str) -> String {
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        // invert yaml_escape (which escaped '\' then '"')
        v[1..v.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        v.to_string()
    }
}

pub fn parse_frontmatter(text: &str) -> (std::collections::HashMap<String, String>, String) {
    use std::collections::HashMap;
    let mut map = HashMap::new();
    // Tolerate a BOM and CRLF line endings (Obsidian/Windows).
    let text = text.strip_prefix('\u{feff}').unwrap_or(text).replace("\r\n", "\n");
    if let Some(rest) = text.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let front = &rest[..end];
            for line in front.lines() {
                if let Some((k, v)) = line.split_once(':') {
                    map.insert(k.trim().to_string(), unquote(v.trim()));
                }
            }
            // body is after the closing --- line
            let after = &rest[end + 4..];
            let body = after.trim_start_matches('\n').to_string();
            return (map, body);
        }
    }
    (map, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_filesystem_safe() {
        assert_eq!(slug("Project Apollo!"), "project-apollo");
        assert_eq!(slug("  Café / Notes  "), "caf-notes");
        assert_eq!(slug(""), "untitled");
        assert_eq!(slug("***"), "untitled");
    }

    #[test]
    fn renders_common_blocks() {
        let doc = r#"[
            {"type":"heading","props":{"level":2},"content":"Summary"},
            {"type":"paragraph","content":"Hello world"},
            {"type":"bulletListItem","content":"first"},
            {"type":"bulletListItem","content":"second"},
            {"type":"checkListItem","props":{"checked":true},"content":"done"},
            {"type":"numberedListItem","content":"one"},
            {"type":"numberedListItem","content":"two"}
        ]"#;
        let md = blocks_to_markdown(doc);
        assert!(md.contains("## Summary"));
        assert!(md.contains("Hello world"));
        assert!(md.contains("- first"));
        assert!(md.contains("- [x] done"));
        assert!(md.contains("1. one"));
        assert!(md.contains("2. two"));
    }

    #[test]
    fn renders_inline_styles_and_links() {
        let doc = r#"[{"type":"paragraph","content":[
            {"type":"text","text":"bold","styles":{"bold":true}},
            {"type":"text","text":" and ","styles":{}},
            {"type":"link","href":"https://x.com","content":[{"type":"text","text":"link","styles":{}}]}
        ]}]"#;
        let md = blocks_to_markdown(doc);
        assert!(md.contains("**bold**"));
        assert!(md.contains("[link](https://x.com)"));
    }

    #[test]
    fn invalid_json_is_empty() {
        assert_eq!(blocks_to_markdown("not json"), "");
        assert_eq!(blocks_to_markdown("{}"), "");
    }

    #[test]
    fn markdown_blocks_roundtrip_text() {
        const MD: &str =
            "## Summary\n\nHello world\n\n- first\n- second\n- [x] done\n\n> a quote\n\n```rust\nlet x = 1;\n```";
        let blocks_json = markdown_to_blocks(MD);
        // valid JSON array
        let v: Value = serde_json::from_str(&blocks_json).unwrap();
        assert!(v.is_array());
        // text survives the md -> blocks -> md round-trip
        let back = blocks_to_markdown(&blocks_json);
        assert!(back.contains("## Summary"));
        assert!(back.contains("Hello world"));
        assert!(back.contains("- first"));
        assert!(back.contains("- [x] done"));
        assert!(back.contains("> a quote"));
        assert!(back.contains("let x = 1;"));
    }

    #[test]
    fn frontmatter_tolerates_crlf() {
        let file = "---\r\nid: abc\r\ntitle: Hi\r\n---\r\n\r\nbody line\r\n";
        let (map, body) = parse_frontmatter(file);
        assert_eq!(map.get("id").unwrap(), "abc");
        assert_eq!(map.get("title").unwrap(), "Hi");
        assert_eq!(body.trim(), "body line");
    }

    #[test]
    fn frontmatter_roundtrips_id_and_title() {
        let tags = vec!["client".to_string(), "urgent".to_string()];
        let meta = PageMeta {
            id: "abc-123",
            title: "My: Tricky # Title",
            icon: Some("📝"),
            kind: "doc",
            parent: Some("parent-1"),
            created: 1000,
            updated: 2000,
            tags: &tags,
        };
        let file = render_page(&meta, "body text");
        let (map, body) = parse_frontmatter(&file);
        assert_eq!(map.get("id").unwrap(), "abc-123");
        assert_eq!(map.get("title").unwrap(), "My: Tricky # Title");
        assert_eq!(map.get("type").unwrap(), "doc");
        assert_eq!(map.get("parent").unwrap(), "parent-1");
        assert_eq!(body.trim(), "body text");
    }
}
