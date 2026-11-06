use super::{
    content_identity,
    models::{CommandError, CommandResult, RichContentApi},
};

pub(super) const MAX_RICH_HTML_BYTES: usize = 2 * 1024 * 1024;
pub(super) const MAX_RICH_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_RICH_PLAIN_BYTES: usize = 512 * 1024;
const MAX_SOURCE_OOXML_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) enum RichContentPolicy {
    Question { allow_empty: bool },
    DraftField,
    // A full document batch is bounded by the caller's payload limit.
    DraftDocument,
}

pub(super) fn validate_rich_content(
    label: &str,
    content: &RichContentApi,
    policy: RichContentPolicy,
) -> CommandResult<()> {
    let question = matches!(policy, RichContentPolicy::Question { .. });
    let enforce_field_limits = !matches!(policy, RichContentPolicy::DraftDocument);
    validate_rich_document(label, content, enforce_field_limits, question)?;
    if question
        && content
            .plain_text
            .chars()
            .any(|character| matches!(character, '\u{1e}' | '\u{1f}'))
    {
        return Err(CommandError::validation(format!(
            "{label}包含软件保留的控制字符，请删除后重试。"
        )));
    }
    if enforce_field_limits && content.html.len() > MAX_RICH_HTML_BYTES {
        return Err(CommandError::validation(format!(
            "{label}的富文本超过 {} MB 的安全上限。",
            MAX_RICH_HTML_BYTES / 1024 / 1024
        )));
    }
    if enforce_field_limits && content.plain_text.len() > MAX_RICH_PLAIN_BYTES {
        return Err(CommandError::validation(format!(
            "{label}的可搜索文字过长{}。",
            if question { "，请拆分内容" } else { "" }
        )));
    }
    if enforce_field_limits
        && content
            .source_ooxml
            .as_ref()
            .is_some_and(|xml| xml.len() > MAX_SOURCE_OOXML_BYTES)
    {
        return Err(CommandError::validation(format!(
            "{label}保留的 Word 原始内容超过安全上限。"
        )));
    }

    if !question
        && (contains_reserved_separator(&content.html)
            || contains_reserved_separator(&content.plain_text)
            || content
                .source_ooxml
                .as_ref()
                .is_some_and(|xml| contains_reserved_separator(xml)))
    {
        return Err(CommandError::validation(format!(
            "{label}包含软件保留的控制分隔符，请删除后重试。"
        )));
    }
    let normalized_html = content
        .html
        .to_ascii_lowercase()
        .split_ascii_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let forbidden = [
        "<script",
        "<iframe",
        "<object",
        "<embed",
        "<link",
        "<meta",
        "<base",
        "javascript:",
        "data:text/html",
        "srcdoc=",
    ];
    if forbidden
        .iter()
        .any(|pattern| normalized_html.contains(pattern))
        || contains_inline_event_handler(&normalized_html)
    {
        return Err(CommandError::new(
            "UNSAFE_RICH_CONTENT",
            format!(
                "{label}中包含不安全的网页代码，已停止{}保存。",
                if question { "" } else { "自动" }
            ),
        ));
    }
    if matches!(policy, RichContentPolicy::Question { allow_empty: false })
        && !content_identity::has_content(content)
    {
        return Err(CommandError::validation(format!("{label}不能为空。")));
    }
    Ok(())
}

pub(super) fn contains_reserved_separator(value: &str) -> bool {
    value
        .chars()
        .any(|character| matches!(character, '\u{1e}' | '\u{1f}'))
}

fn contains_inline_event_handler(html: &str) -> bool {
    let bytes = html.as_bytes();
    let mut index = 0usize;
    while index + 3 < bytes.len() {
        if bytes[index] == b' ' && bytes[index + 1] == b'o' && bytes[index + 2] == b'n' {
            let mut cursor = index + 3;
            while cursor < bytes.len() && bytes[cursor].is_ascii_alphabetic() {
                cursor += 1;
            }
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor > index + 3 && bytes.get(cursor) == Some(&b'=') {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn validate_rich_document(
    label: &str,
    content: &RichContentApi,
    enforce_document_limit: bool,
    question: bool,
) -> CommandResult<()> {
    match content.schema_version {
        1 => {
            if content.editor.is_some()
                || content.editor_version.is_some()
                || content.document.is_some()
            {
                return Err(CommandError::validation(format!(
                    "{label}的旧内容版本不能携带编辑器 JSON。"
                )));
            }
        }
        2 => {
            if content.editor.as_deref() != Some("tiptap") {
                return Err(CommandError::validation(format!(
                    "{label}的编辑器类型不受支持。"
                )));
            }
            let version = content.editor_version.as_deref().unwrap_or_default().trim();
            if version.is_empty() || version.chars().count() > 64 {
                return Err(CommandError::validation(format!(
                    "{label}的编辑器版本无效。"
                )));
            }
            let document = content.document.as_ref().ok_or_else(|| {
                CommandError::validation(format!("{label}缺少 Tiptap 文档 JSON。"))
            })?;
            if document.get("type").and_then(serde_json::Value::as_str) != Some("doc") {
                return Err(CommandError::validation(format!(
                    "{label}的 Tiptap 文档根节点无效。"
                )));
            }
            if enforce_document_limit
                && serde_json::to_vec(document)
                    .map_err(CommandError::database)?
                    .len()
                    > MAX_RICH_DOCUMENT_BYTES
            {
                return Err(CommandError::validation(format!(
                    "{label}的 Tiptap 文档 JSON 超过安全上限。"
                )));
            }
        }
        _ => {
            return Err(CommandError::validation(format!(
                "{label}使用了{}软件{}支持的内容版本。",
                if question { "" } else { "当前" },
                if question { "暂不" } else { "不" }
            )));
        }
    }
    Ok(())
}

pub(super) fn rich_content_bytes(content: &RichContentApi) -> usize {
    content
        .html
        .len()
        .saturating_add(content.plain_text.len())
        .saturating_add(
            content
                .source_ooxml
                .as_ref()
                .map_or(0, std::string::String::len),
        )
        .saturating_add(
            content
                .document
                .as_ref()
                .and_then(|document| serde_json::to_vec(document).ok())
                .map_or(0, |document| document.len()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inline_event_handler_detection_matches_question_write_safety_rule() {
        assert!(contains_inline_event_handler(
            "<img src=x onerror =alert(1)>"
        ));
        assert!(!contains_inline_event_handler("<strong>on call</strong>"));
    }
}
