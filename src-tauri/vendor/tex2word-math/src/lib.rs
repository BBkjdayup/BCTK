//! tex2word math engine — LaTeX math -> OMML (OfficeMath).
//!
//! A LaTeX-math parser builds a small AST, and an OMML renderer produces the
//! `m:` elements Word edits as native equations. The public entry point wraps
//! the rendered content in an `<m:oMath>` element (the `m` namespace is declared
//! on the document root by the back-end).

mod omml;
mod parser;
mod symbols;

/// Render inline LaTeX math to a complete `<m:oMath>…</m:oMath>` element.
pub fn to_omath(latex: &str) -> String {
    format!("<m:oMath>{}</m:oMath>", omml::render(&parser::parse(latex)))
}

/// Convert only when every input structure can be represented. Never return a
/// partial equation after an unsupported command or malformed argument.
pub fn try_to_omath(latex: &str) -> Result<String, String> {
    if latex.trim().is_empty() {
        return Err("公式为空".into());
    }
    if latex.len() > 64 * 1024 {
        return Err("公式超过 64 KB".into());
    }
    if latex.chars().any(|c| !matches!(c, '\t' | '\n' | '\r' | ' '..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')) {
        return Err("公式包含无法写入 Word 的控制字符".into());
    }
    let node = parser::parse_checked(latex)?;
    let content = omml::render(&node);
    if content.is_empty() {
        return Err("公式没有可导出的内容".into());
    }
    Ok(format!("<m:oMath>{content}</m:oMath>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_in_omath() {
        let x = to_omath("x^2");
        assert!(x.starts_with("<m:oMath>") && x.ends_with("</m:oMath>"));
        assert!(x.contains("<m:sSup>"));
    }

    #[test]
    fn full_expression() {
        // E = mc^2 -> a leading text run (E=m, adjacent runs merge) + sSup for c^2
        let x = to_omath("E = mc^2");
        assert!(x.contains("<m:sSup>"));
        assert!(x.contains('E') && x.contains("<m:t>c</m:t>"));
    }
}
