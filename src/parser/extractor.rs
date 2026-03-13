use tree_sitter::Node;

// Shared helpers used by all language extractors.

pub fn strip_quotes(text: &str) -> &str {
    text.trim_matches(|c| c == '\'' || c == '"' || c == '`')
}

/// Extract the doc comment immediately preceding a node (JS/TS/Rust style).
pub fn get_doc_comment(source: &str, node: &Node) -> Option<String> {
    let start_line = node.start_position().row;
    let lines: Vec<&str> = source.lines().collect();

    let mut comment_lines: Vec<&str> = Vec::new();

    if start_line == 0 {
        return None;
    }

    for i in (0..start_line).rev() {
        let trimmed = lines[i].trim();

        if trimmed.starts_with("/**")
            || trimmed.starts_with("* ")
            || trimmed.starts_with("*/")
            || trimmed == "*"
        {
            comment_lines.push(trimmed);
            if trimmed.starts_with("/**") {
                break;
            }
            continue;
        }

        if trimmed.starts_with("///") || trimmed.starts_with("//") {
            comment_lines.push(trimmed);
            continue;
        }

        break;
    }

    if comment_lines.is_empty() {
        return None;
    }

    comment_lines.reverse();

    let result: Vec<String> = comment_lines
        .iter()
        .map(|line| {
            let s = line.to_string();
            let s = s.trim_start_matches("/**").trim_start();
            let s = s.trim_start_matches("*/").trim_start();
            let s = s
                .trim_start_matches("* ")
                .trim_start_matches('*')
                .trim_start();
            let s = s.trim_start_matches("/// ").trim_start_matches("///");
            let s = s.trim_start_matches("// ").trim_start_matches("//");
            s.trim().to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();

    if result.is_empty() {
        return None;
    }

    Some(result.join("\n"))
}

/// Get a concise signature string for a node — everything up to the opening brace.
pub fn get_signature(node: &Node, source: &[u8]) -> String {
    let text = node.utf8_text(source).unwrap_or("");
    if let Some(brace_idx) = text.find('{') {
        return text[..brace_idx].trim().to_string();
    }
    if let Some(newline_idx) = text.find('\n') {
        return text[..newline_idx].trim().to_string();
    }
    text.trim().to_string()
}
