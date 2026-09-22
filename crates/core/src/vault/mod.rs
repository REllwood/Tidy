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
        // A link to another Tidy page: Obsidian reads the same [[Title]] form.
        Some("wikilink") => out.push_str(&format!("[[{}]]", wikilink_title(node))),
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

    // Recurse into nested children (indented one level). Numbering restarts
    // inside each list, and a non-numbered child breaks the run.
    if let Some(children) = block.get("children").and_then(|c| c.as_array()) {
        let mut ord = 0usize;
        for child in children {
            if child.get("type").and_then(|t| t.as_str()) == Some("numberedListItem") {
                ord += 1;
            } else {
                ord = 0;
            }
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

// ---- Markdown inline runs -> BlockNote inline content ----------------------

/// The inline styles we round-trip. Anything else survives as literal text.
#[derive(Clone, Copy, Default)]
struct Styles {
    bold: bool,
    italic: bool,
    code: bool,
}

impl Styles {
    fn to_json(self) -> Value {
        let mut map = serde_json::Map::new();
        if self.bold {
            map.insert("bold".into(), Value::Bool(true));
        }
        if self.italic {
            map.insert("italic".into(), Value::Bool(true));
        }
        if self.code {
            map.insert("code".into(), Value::Bool(true));
        }
        Value::Object(map)
    }
}

fn push_run(text: &str, styles: Styles, out: &mut Vec<Value>) {
    if text.is_empty() {
        return;
    }
    out.push(serde_json::json!({
        "type": "text",
        "text": text,
        "styles": styles.to_json(),
    }));
}

/// `[label](href)` starting at the front of `s` → (label, href, byte length).
fn parse_link(s: &str) -> Option<(&str, &str, usize)> {
    let close = s.find(']')?;
    if !s[close + 1..].starts_with('(') {
        return None;
    }
    let open = close + 1;
    let end = s[open..].find(')')? + open;
    Some((&s[1..close], &s[open + 1..end], end + 1))
}

/// Parse `**bold**`, `*italic*`, `` `code` `` and `[text](href)` out of a run of
/// Markdown into BlockNote inline nodes, carrying `styles` down into nested
/// markers so `**bold *and italic*` keeps both.
fn parse_runs(text: &str, styles: Styles, out: &mut Vec<Value>) {
    let mut plain = String::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        // Inside a code run everything is literal.
        if !styles.code {
            if let Some(inner) = rest.strip_prefix("**") {
                if let Some(end) = inner.find("**").filter(|e| *e > 0) {
                    push_run(&plain, styles, out);
                    plain.clear();
                    parse_runs(
                        &inner[..end],
                        Styles { bold: true, ..styles },
                        out,
                    );
                    i += 2 + end + 2;
                    continue;
                }
            }
            if let Some(inner) = rest.strip_prefix('*') {
                if !inner.starts_with('*') {
                    if let Some(end) = inner.find('*').filter(|e| *e > 0) {
                        push_run(&plain, styles, out);
                        plain.clear();
                        parse_runs(
                            &inner[..end],
                            Styles { italic: true, ..styles },
                            out,
                        );
                        i += 1 + end + 1;
                        continue;
                    }
                }
            }
            if let Some(inner) = rest.strip_prefix('`') {
                if let Some(end) = inner.find('`').filter(|e| *e > 0) {
                    push_run(&plain, styles, out);
                    plain.clear();
                    push_run(&inner[..end], Styles { code: true, ..styles }, out);
                    i += 1 + end + 1;
                    continue;
                }
            }
            if rest.starts_with('[') {
                if let Some((label, href, len)) = parse_link(rest) {
                    push_run(&plain, styles, out);
                    plain.clear();
                    let mut inner = Vec::new();
                    parse_runs(label, styles, &mut inner);
                    out.push(serde_json::json!({
                        "type": "link",
                        "href": href,
                        "content": inner,
                    }));
                    i += len;
                    continue;
                }
            }
        }
        let ch = rest.chars().next().unwrap_or_default();
        plain.push(ch);
        i += ch.len_utf8();
    }
    push_run(&plain, styles, out);
}

/// Build a block's `content` from Markdown. Unformatted text stays a plain
/// string (the compact form BlockNote also accepts); anything with styling or a
/// link becomes an array of inline nodes.
fn inline_content(text: &str) -> Value {
    let mut runs = Vec::new();
    parse_runs(text, Styles::default(), &mut runs);
    let unstyled = runs.len() == 1
        && runs[0].get("type").and_then(|t| t.as_str()) == Some("text")
        && runs[0]
            .get("styles")
            .and_then(|s| s.as_object())
            .map(|s| s.is_empty())
            .unwrap_or(false);
    if unstyled {
        return Value::String(
            runs[0]
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string(),
        );
    }
    if runs.is_empty() {
        return Value::String(String::new());
    }
    Value::Array(runs)
}

// ---- Markdown -> BlockNote blocks ------------------------------------------

/// Width of a line's leading whitespace, tabs counted as one indent step.
fn indent_width(raw: &str) -> usize {
    let mut width = 0;
    for ch in raw.chars() {
        match ch {
            ' ' => width += 1,
            '\t' => width += 2,
            _ => break,
        }
    }
    width
}

/// Attach a finished block to its parent (or to the document root).
fn close_block(stack: &mut [(usize, Value)], roots: &mut Vec<Value>, block: Value) {
    match stack.last_mut() {
        Some((_, parent)) => {
            if let Some(children) = parent.get_mut("children").and_then(|c| c.as_array_mut()) {
                children.push(block);
            }
        }
        None => roots.push(block),
    }
}

/// Drop the empty `children` arrays we used while building the tree.
fn prune_children(block: &mut Value) {
    let empty = block
        .get("children")
        .and_then(|c| c.as_array())
        .map(|c| c.is_empty())
        .unwrap_or(false);
    if empty {
        if let Some(map) = block.as_object_mut() {
            map.remove("children");
        }
        return;
    }
    if let Some(children) = block.get_mut("children").and_then(|c| c.as_array_mut()) {
        for child in children {
            prune_children(child);
        }
    }
}

/// Open a block at `indent`: anything at the same or a shallower indent is
/// finished first, so what stays on the stack is exactly this block's ancestry.
fn open_block(
    indent: usize,
    mut block: Value,
    stack: &mut Vec<(usize, Value)>,
    roots: &mut Vec<Value>,
) {
    block["children"] = Value::Array(Vec::new());
    while let Some((top, _)) = stack.last() {
        if indent > *top {
            break;
        }
        let (_, done) = stack.pop().unwrap_or_default();
        close_block(stack, roots, done);
    }
    stack.push((indent, block));
}

/// Parse Markdown back into a BlockNote document (a JSON array of blocks). The
/// inverse of `blocks_to_markdown`: indentation becomes nested `children` and
/// inline markers become styled runs, so an external edit round-trips into the
/// canonical `content` column instead of being flattened.
pub fn markdown_to_blocks(md: &str) -> String {
    let md = md.replace("\r\n", "\n");
    let mut roots: Vec<Value> = Vec::new();
    // Ancestors of the block being read, shallowest first.
    let mut stack: Vec<(usize, Value)> = Vec::new();
    let mut in_code = false;
    let mut code_lang = String::new();
    let mut code_indent = 0usize;
    let mut code_buf: Vec<String> = Vec::new();

    for raw in md.lines() {
        let ts = raw.trim_start();
        if let Some(rest) = ts.strip_prefix("```") {
            if !in_code {
                in_code = true;
                code_lang = rest.trim().to_string();
                code_indent = indent_width(raw);
                code_buf.clear();
            } else {
                in_code = false;
                open_block(
                    code_indent,
                    serde_json::json!({
                        "type": "codeBlock",
                        "props": { "language": code_lang },
                        "content": code_buf.join("\n"),
                    }),
                    &mut stack,
                    &mut roots,
                );
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
        let indent = indent_width(raw);
        let block = if let Some(level) = heading_level(t) {
            let text = t[level..].trim_start();
            serde_json::json!({"type":"heading","props":{"level":level},"content":inline_content(text)})
        } else if let Some(rest) = t.strip_prefix("- [ ] ").or_else(|| t.strip_prefix("- [] ")) {
            serde_json::json!({"type":"checkListItem","props":{"checked":false},"content":inline_content(rest)})
        } else if let Some(rest) = t.strip_prefix("- [x] ").or_else(|| t.strip_prefix("- [X] ")) {
            serde_json::json!({"type":"checkListItem","props":{"checked":true},"content":inline_content(rest)})
        } else if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            serde_json::json!({"type":"bulletListItem","content":inline_content(rest)})
        } else if let Some(rest) = split_numbered(t) {
            serde_json::json!({"type":"numberedListItem","content":inline_content(rest)})
        } else if let Some(rest) = t.strip_prefix("> ") {
            serde_json::json!({"type":"quote","content":inline_content(rest)})
        } else {
            serde_json::json!({"type":"paragraph","content":inline_content(t)})
        };
        open_block(indent, block, &mut stack, &mut roots);
    }
    if in_code && !code_buf.is_empty() {
        open_block(
            code_indent,
            serde_json::json!({
                "type": "codeBlock",
                "props": { "language": code_lang },
                "content": code_buf.join("\n"),
            }),
            &mut stack,
            &mut roots,
        );
    }
    while let Some((_, done)) = stack.pop() {
        close_block(&mut stack, &mut roots, done);
    }
    for block in roots.iter_mut() {
        prune_children(block);
    }
    if roots.is_empty() {
        roots.push(serde_json::json!({"type":"paragraph","content":""}));
    }
    Value::Array(roots).to_string()
}

/// Collect the visible text of a block's inline content, markers and all
/// styling dropped.
/// The title a page link shows (`{type: "wikilink", props: {title}}`).
pub fn wikilink_title(node: &Value) -> &str {
    node.get("props")
        .and_then(|p| p.get("title"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
}

fn inline_text(content: &Value, out: &mut String) {
    match content {
        Value::String(s) => out.push_str(s),
        Value::Array(nodes) => {
            for node in nodes {
                if let Some(s) = node.as_str() {
                    out.push_str(s);
                } else if node.get("type").and_then(|t| t.as_str()) == Some("wikilink") {
                    out.push_str(wikilink_title(node));
                } else if node.get("type").and_then(|t| t.as_str()) == Some("link") {
                    if let Some(inner) = node.get("content") {
                        inline_text(inner, out);
                    }
                } else if let Some(t) = node.get("text").and_then(|t| t.as_str()) {
                    out.push_str(t);
                }
            }
        }
        _ => {}
    }
}

fn plaintext_lines(blocks: &[Value], out: &mut Vec<String>) {
    for block in blocks {
        let mut line = String::new();
        if let Some(content) = block.get("content") {
            inline_text(content, &mut line);
        }
        let line = line.trim();
        if !line.is_empty() {
            out.push(line.to_string());
        }
        if let Some(children) = block.get("children").and_then(|c| c.as_array()) {
            plaintext_lines(children, out);
        }
    }
}

/// A plain-text projection of a document body (for FTS / previews): the prose
/// only, with no Markdown markers or BlockNote scaffolding.
pub fn blocks_to_plaintext(blocks_json: &str) -> String {
    let parsed: Value = match serde_json::from_str(blocks_json) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    let arr = match parsed.as_array() {
        Some(a) => a,
        None => return String::new(),
    };
    let mut lines = Vec::new();
    plaintext_lines(arr, &mut lines);
    lines.join("\n")
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
    fn page_links_export_as_wiki_links_and_stay_searchable() {
        let body = r#"[{"type":"paragraph","content":[{"type":"text","text":"See ","styles":{}},{"type":"wikilink","props":{"pageId":"p2","title":"Launch plan"}}]}]"#;
        assert_eq!(blocks_to_markdown(body).trim(), "See [[Launch plan]]");
        assert_eq!(blocks_to_plaintext(body), "See Launch plan");
    }

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

    /// Export then import must give back the same document: the vault is a
    /// mirror, so a user editing the file in Obsidian cannot be punished with a
    /// flattened list or literal asterisks in their note.
    #[test]
    fn export_import_round_trips_structure_and_styles() {
        const DOC: &str = r##"[
            {"type":"heading","props":{"level":2},"content":"Summary"},
            {"type":"bulletListItem","content":"top","children":[
                {"type":"bulletListItem","content":"nested","children":[
                    {"type":"bulletListItem","content":"deeper"}
                ]}
            ]},
            {"type":"numberedListItem","content":"one"},
            {"type":"numberedListItem","content":"two","children":[
                {"type":"numberedListItem","content":"two a"}
            ]},
            {"type":"checkListItem","props":{"checked":true},"content":"done"},
            {"type":"checkListItem","props":{"checked":false},"content":"todo"}
        ]"##;
        let md = blocks_to_markdown(DOC);
        let back: Value = serde_json::from_str(&markdown_to_blocks(&md)).unwrap();
        let blocks = back.as_array().unwrap();

        let kind = |b: &Value| b.get("type").unwrap().as_str().unwrap().to_string();
        let text = |b: &Value| b.get("content").unwrap().as_str().unwrap().to_string();
        assert_eq!(kind(&blocks[0]), "heading");
        assert_eq!(blocks[0].pointer("/props/level").unwrap(), &serde_json::json!(2));

        // the nested bullets survived as children, not as flattened siblings
        assert_eq!(kind(&blocks[1]), "bulletListItem");
        assert_eq!(text(&blocks[1]), "top");
        let nested = &blocks[1].get("children").unwrap().as_array().unwrap()[0];
        assert_eq!(text(nested), "nested");
        let deeper = &nested.get("children").unwrap().as_array().unwrap()[0];
        assert_eq!(text(deeper), "deeper");

        assert_eq!(kind(&blocks[2]), "numberedListItem");
        assert_eq!(text(&blocks[2]), "one");
        assert_eq!(text(&blocks[3]), "two");
        let sub = &blocks[3].get("children").unwrap().as_array().unwrap()[0];
        assert_eq!(kind(sub), "numberedListItem");
        assert_eq!(text(sub), "two a");

        assert_eq!(kind(&blocks[4]), "checkListItem");
        assert_eq!(blocks[4].pointer("/props/checked").unwrap(), &serde_json::json!(true));
        assert_eq!(blocks[5].pointer("/props/checked").unwrap(), &serde_json::json!(false));

        // and a second lap is stable
        assert_eq!(blocks_to_markdown(&markdown_to_blocks(&md)), md);
    }

    #[test]
    fn inline_styles_and_links_round_trip_as_runs() {
        const DOC: &str = r##"[{"type":"paragraph","content":[
            {"type":"text","text":"plain ","styles":{}},
            {"type":"text","text":"bold","styles":{"bold":true}},
            {"type":"text","text":" ","styles":{}},
            {"type":"text","text":"italic","styles":{"italic":true}},
            {"type":"text","text":" ","styles":{}},
            {"type":"text","text":"code","styles":{"code":true}},
            {"type":"text","text":" ","styles":{}},
            {"type":"link","href":"https://example.com","content":[{"type":"text","text":"link","styles":{}}]}
        ]}]"##;
        let md = blocks_to_markdown(DOC);
        assert_eq!(md, "plain **bold** *italic* `code` [link](https://example.com)");

        let back: Value = serde_json::from_str(&markdown_to_blocks(&md)).unwrap();
        let runs = back[0].get("content").unwrap().as_array().unwrap();
        let styled = |name: &str| {
            runs.iter()
                .find(|r| r.get("text").and_then(|t| t.as_str()) == Some(name))
                .unwrap_or_else(|| panic!("no run for {name}"))
        };
        assert_eq!(styled("bold").pointer("/styles/bold").unwrap(), &serde_json::json!(true));
        assert_eq!(styled("italic").pointer("/styles/italic").unwrap(), &serde_json::json!(true));
        assert_eq!(styled("code").pointer("/styles/code").unwrap(), &serde_json::json!(true));
        let link = runs.last().unwrap();
        assert_eq!(link.get("type").unwrap(), &serde_json::json!("link"));
        assert_eq!(link.get("href").unwrap(), &serde_json::json!("https://example.com"));
        assert_eq!(link.pointer("/content/0/text").unwrap(), &serde_json::json!("link"));

        // no formatting means the compact string form, as before
        let plain: Value = serde_json::from_str(&markdown_to_blocks("just words")).unwrap();
        assert_eq!(plain[0].get("content").unwrap(), &serde_json::json!("just words"));
    }

    #[test]
    fn plaintext_projection_is_prose_only() {
        const DOC: &str = r##"[
            {"type":"heading","props":{"level":2},"content":"Summary"},
            {"type":"bulletListItem","content":[
                {"type":"text","text":"call ","styles":{}},
                {"type":"link","href":"https://x.test","content":[{"type":"text","text":"Acme","styles":{"bold":true}}]}
            ],"children":[{"type":"bulletListItem","content":"nested point"}]}
        ]"##;
        assert_eq!(
            blocks_to_plaintext(DOC),
            "Summary\ncall Acme\nnested point"
        );
        assert_eq!(blocks_to_plaintext("not json"), "");
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
