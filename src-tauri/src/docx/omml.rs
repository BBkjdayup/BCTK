//! Bounded conversion of Word's editable OMML formula structures to LaTeX.
//!
//! Flattening all `m:t` nodes loses fraction bars, roots and scripts while
//! producing plausible-looking but mathematically different expressions.

use std::io::{Read, Seek};

use quick_xml::{
    events::{BytesStart, Event},
    reader::NsReader,
};

use super::{
    DocxError, DocxLimits, DocxResult, read_document_from_docx,
    xml::{decode_reference, local_name},
};

const OMML_PART: &str = "word/document.xml OMML";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedEditableFormulaOccurrence {
    pub paragraph_index: usize,
    pub text_char_offset: usize,
    pub latex: String,
    pub source_kind: String,
    pub product_version: u8,
    pub product_subversion: u8,
}

pub fn read_omml_from_docx<R: Read + Seek>(
    source: R,
    limits: &DocxLimits,
) -> DocxResult<Vec<ExtractedEditableFormulaOccurrence>> {
    let parsed = read_document_from_docx(source, limits)?;
    parsed
        .math_fragments
        .into_iter()
        .map(|fragment| {
            let paragraph_index = fragment
                .paragraph_index
                .ok_or_else(|| invalid_omml("an OMML formula occurs outside a Word paragraph"))?;
            let text_char_offset = fragment.text_char_offset.ok_or_else(|| {
                invalid_omml("an OMML formula does not have a visible text position")
            })?;
            Ok(ExtractedEditableFormulaOccurrence {
                paragraph_index,
                text_char_offset,
                latex: convert_omml_to_latex(&fragment.raw_xml)?,
                source_kind: "word_omml".to_owned(),
                product_version: 0,
                product_subversion: 0,
            })
        })
        .collect()
}

fn convert_omml_to_latex(xml: &[u8]) -> DocxResult<String> {
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut stack = Vec::<OmmlNode>::new();
    let mut root = None;

    loop {
        let position = reader.buffer_position();
        match reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(OMML_PART, reader.error_position(), error))?
        {
            Event::Start(ref start) => {
                stack.push(OmmlNode::from_start(start)?);
            }
            Event::End(ref end) => {
                let node = stack
                    .pop()
                    .ok_or_else(|| invalid_omml("unexpected closing tag"))?;
                if node.name.as_bytes() != end.local_name().as_ref() {
                    return Err(invalid_omml("mismatched OMML closing tag"));
                }
                append_node(&mut stack, &mut root, node)?;
            }
            Event::Empty(ref start) => {
                append_node(&mut stack, &mut root, OmmlNode::from_start(start)?)?;
            }
            Event::Text(ref text) => {
                let decoded = text
                    .xml10_content()
                    .map_err(|error| DocxError::xml(OMML_PART, position, error))?;
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&decoded);
                }
            }
            Event::CData(ref text) => {
                let decoded = text
                    .xml10_content()
                    .map_err(|error| DocxError::xml(OMML_PART, position, error))?;
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&decoded);
                }
            }
            Event::GeneralRef(ref reference) => {
                if let Some(node) = stack.last_mut() {
                    node.text
                        .push(decode_reference(reference, OMML_PART, position)?);
                }
            }
            Event::DocType(_) => {
                return Err(DocxError::xml(
                    OMML_PART,
                    position,
                    "DOCTYPE declarations are not allowed",
                ));
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }

    if !stack.is_empty() {
        return Err(invalid_omml("unclosed OMML element"));
    }
    let latex = render_omml(&root.ok_or_else(|| invalid_omml("missing OMML root"))?)?;
    if latex.trim().is_empty() {
        return Err(invalid_omml(
            "the OMML formula did not contain editable mathematical text",
        ));
    }
    Ok(latex)
}

#[derive(Debug)]
struct OmmlNode {
    name: String,
    value: Option<String>,
    text: String,
    children: Vec<OmmlNode>,
}

impl OmmlNode {
    fn from_start(start: &BytesStart<'_>) -> DocxResult<Self> {
        let name = String::from_utf8(local_name(start))
            .map_err(|_| invalid_omml("invalid OMML element name"))?;
        let mut value = None;
        for attribute in start.attributes() {
            let attribute = attribute.map_err(|error| invalid_omml(error.to_string()))?;
            if attribute.key.local_name().as_ref() == b"val" {
                #[allow(deprecated)]
                let decoded = attribute
                    .decode_and_unescape_value(start.decoder())
                    .map_err(|error| invalid_omml(error.to_string()))?;
                value = Some(decoded.into_owned());
            }
        }
        Ok(Self {
            name,
            value,
            text: String::new(),
            children: Vec::new(),
        })
    }

    fn child(&self, name: &str) -> Option<&Self> {
        self.children.iter().find(|child| child.name == name)
    }

    fn required_child(&self, name: &str) -> DocxResult<&Self> {
        self.child(name)
            .ok_or_else(|| invalid_omml(format!("{} is missing {name}", self.name)))
    }
}

fn append_node(
    stack: &mut [OmmlNode],
    root: &mut Option<OmmlNode>,
    node: OmmlNode,
) -> DocxResult<()> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if root.replace(node).is_some() {
        return Err(invalid_omml("multiple OMML roots"));
    }
    Ok(())
}

fn render_children(node: &OmmlNode) -> DocxResult<String> {
    let mut output = String::new();
    for child in &node.children {
        let part = render_omml(child)?;
        if ends_in_control_word(&output)
            && part
                .starts_with(|character: char| character.is_ascii_alphabetic() || character == '\\')
        {
            output.push(' ');
        }
        output.push_str(&part);
    }
    Ok(output)
}

fn ends_in_control_word(text: &str) -> bool {
    let letters = text
        .chars()
        .rev()
        .take_while(char::is_ascii_alphabetic)
        .count();
    letters > 0 && text.chars().rev().nth(letters) == Some('\\')
}

fn script_base(base: String) -> String {
    if base.chars().count() == 1 || base.starts_with('\\') {
        base
    } else {
        format!("{{{base}}}")
    }
}

fn delimiter(value: &str) -> String {
    match value {
        "" => ".".to_owned(),
        "{" => r"\{".to_owned(),
        "}" => r"\}".to_owned(),
        other => other.to_owned(),
    }
}

fn render_omml(node: &OmmlNode) -> DocxResult<String> {
    match node.name.as_str() {
        "oMath" | "oMathPara" | "e" | "num" | "den" | "sup" | "sub" | "deg" | "fName" | "mr"
        | "r" => render_children(node),
        "t" => Ok(unicode_math_text_to_latex(&node.text)),
        "f" => Ok(format!(
            r"\frac{{{}}}{{{}}}",
            render_omml(node.required_child("num")?)?,
            render_omml(node.required_child("den")?)?,
        )),
        "rad" => {
            let radicand = render_omml(node.required_child("e")?)?;
            let degree = node
                .child("deg")
                .map(render_omml)
                .transpose()?
                .unwrap_or_default();
            if degree.trim().is_empty() {
                Ok(format!(r"\sqrt{{{radicand}}}"))
            } else {
                Ok(format!(r"\sqrt[{degree}]{{{radicand}}}"))
            }
        }
        "sSup" => Ok(format!(
            "{}^{{{}}}",
            script_base(render_omml(node.required_child("e")?)?),
            render_omml(node.required_child("sup")?)?,
        )),
        "sSub" => Ok(format!(
            "{}_{{{}}}",
            script_base(render_omml(node.required_child("e")?)?),
            render_omml(node.required_child("sub")?)?,
        )),
        "sSubSup" => Ok(format!(
            "{}_{{{}}}^{{{}}}",
            script_base(render_omml(node.required_child("e")?)?),
            render_omml(node.required_child("sub")?)?,
            render_omml(node.required_child("sup")?)?,
        )),
        "func" => {
            node.required_child("fName")?;
            node.required_child("e")?;
            render_children(node)
        }
        "m" => {
            let rows = node
                .children
                .iter()
                .filter(|child| child.name == "mr")
                .map(|row| {
                    row.children
                        .iter()
                        .filter(|child| child.name == "e")
                        .map(render_omml)
                        .collect::<DocxResult<Vec<_>>>()
                        .map(|cells| cells.join(" & "))
                })
                .collect::<DocxResult<Vec<_>>>()?;
            Ok(format!(
                r"\begin{{matrix}}{}\end{{matrix}}",
                rows.join(r" \\ ")
            ))
        }
        "d" => {
            let properties = node.child("dPr");
            let left = properties
                .and_then(|p| p.child("begChr"))
                .and_then(|v| v.value.as_deref())
                .unwrap_or("(");
            let right = properties
                .and_then(|p| p.child("endChr"))
                .and_then(|v| v.value.as_deref())
                .unwrap_or(")");
            let separator = properties
                .and_then(|p| p.child("sepChr"))
                .and_then(|v| v.value.as_deref())
                .unwrap_or("|");
            let entries = node
                .children
                .iter()
                .filter(|child| child.name == "e")
                .map(render_omml)
                .collect::<DocxResult<Vec<_>>>()?;
            Ok(format!(
                r"\left{}{}\right{}",
                delimiter(left),
                entries.join(&delimiter(separator)),
                delimiter(right)
            ))
        }
        name if name.ends_with("Pr")
            || matches!(name, "degHide" | "begChr" | "endChr" | "sepChr") =>
        {
            Ok(String::new())
        }
        name => Err(invalid_omml(format!("unsupported OMML element: {name}"))),
    }
}

pub(crate) fn unicode_math_text_to_latex(text: &str) -> String {
    let characters = text.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut index = 0usize;

    while index < characters.len() {
        let remaining = characters[index..].iter().collect::<String>();
        if let Some((name, latex)) = [
            ("arcsin", r"\arcsin "),
            ("arccos", r"\arccos "),
            ("arctan", r"\arctan "),
            ("sin", r"\sin "),
            ("cos", r"\cos "),
            ("tan", r"\tan "),
            ("log", r"\log "),
            ("ln", r"\ln "),
        ]
        .into_iter()
        .find(|(name, _)| remaining.starts_with(name))
        {
            output.push_str(latex);
            index += name.chars().count();
            continue;
        }

        let character = characters[index];
        match character {
            ' ' | '\t' | '\r' | '\n' => {
                let start = index;
                while index < characters.len() && characters[index].is_whitespace() {
                    index += 1;
                }
                if index.saturating_sub(start) >= 2 {
                    output.push_str(r"\quad ");
                } else if !output.ends_with(' ') {
                    output.push(' ');
                }
                continue;
            }
            '⊿' | '△' => output.push_str(r"\triangle "),
            'α' => output.push_str(r"\alpha "),
            'β' => output.push_str(r"\beta "),
            'γ' => output.push_str(r"\gamma "),
            'δ' => output.push_str(r"\delta "),
            'θ' => output.push_str(r"\theta "),
            'λ' => output.push_str(r"\lambda "),
            'μ' => output.push_str(r"\mu "),
            'π' => output.push_str(r"\pi "),
            'ρ' => output.push_str(r"\rho "),
            'σ' => output.push_str(r"\sigma "),
            'φ' => output.push_str(r"\varphi "),
            'ω' => output.push_str(r"\omega "),
            'Γ' => output.push_str(r"\Gamma "),
            'Δ' => output.push_str(r"\Delta "),
            'Θ' => output.push_str(r"\Theta "),
            'Λ' => output.push_str(r"\Lambda "),
            'Π' => output.push_str(r"\Pi "),
            'Σ' => output.push_str(r"\Sigma "),
            'Φ' => output.push_str(r"\Phi "),
            'Ω' => output.push_str(r"\Omega "),
            '−' | '–' | '﹣' => output.push('-'),
            '（' => output.push('('),
            '）' => output.push(')'),
            '＞' => output.push('>'),
            '＜' => output.push('<'),
            '×' => output.push_str(r"\times "),
            '÷' => output.push_str(r"\div "),
            '±' => output.push_str(r"\pm "),
            '·' | '⋅' => output.push_str(r"\cdot "),
            '∞' => output.push_str(r"\infty "),
            '∠' => output.push_str(r"\angle "),
            '∈' => output.push_str(r"\in "),
            '∉' => output.push_str(r"\notin "),
            '≤' => output.push_str(r"\le "),
            '≥' => output.push_str(r"\ge "),
            '≠' => output.push_str(r"\ne "),
            '#' => output.push_str(r"\#"),
            '$' => output.push_str(r"\$"),
            '%' => output.push_str(r"\%"),
            '&' => output.push_str(r"\&"),
            '_' => output.push_str(r"\_"),
            '{' => output.push_str(r"\{"),
            '}' => output.push_str(r"\}"),
            '\\' => output.push_str(r"\backslash "),
            '^' => output.push('^'),
            '~' => output.push_str(r"\sim "),
            other => output.push(other),
        }
        index += 1;
    }

    output.trim().to_owned()
}

fn invalid_omml(message: impl Into<String>) -> DocxError {
    DocxError::Xml {
        part_name: OMML_PART.to_owned(),
        position: 0,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_the_three_native_word_formula_shapes_from_the_math_exam() {
        let triangle = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r><m:t>⊿</m:t></m:r><m:r><m:t>ABC</m:t></m:r></m:oMath>"#;
        let functions = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:func><m:fName><m:r><m:t>sin</m:t></m:r></m:fName><m:e><m:r><m:t>α</m:t></m:r></m:e></m:func><m:func><m:fName><m:r><m:t>cos</m:t></m:r></m:fName><m:e><m:r><m:t>α</m:t></m:r></m:e></m:func><m:r><m:t>,      </m:t></m:r><m:func><m:fName><m:r><m:t>sin</m:t></m:r></m:fName><m:e><m:r><m:t>α</m:t></m:r></m:e></m:func><m:r><m:t>−</m:t></m:r><m:func><m:fName><m:r><m:t>cos</m:t></m:r></m:fName><m:e><m:r><m:t>α</m:t></m:r></m:e></m:func></m:oMath>"#;
        let point = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r><m:t>A(8,−6)</m:t></m:r></m:oMath>"#;

        assert_eq!(
            convert_omml_to_latex(triangle.as_bytes()).unwrap(),
            r"\triangle ABC"
        );
        assert_eq!(
            convert_omml_to_latex(functions.as_bytes()).unwrap(),
            r"\sin \alpha \cos \alpha,\quad \sin \alpha-\cos \alpha"
        );
        assert_eq!(convert_omml_to_latex(point.as_bytes()).unwrap(), "A(8,-6)");
    }

    #[test]
    fn retains_fraction_root_and_scripts_instead_of_flattening_them() {
        let formula = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r><m:t>y=</m:t></m:r><m:f><m:num><m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup></m:num><m:den><m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg/><m:e><m:r><m:t>3</m:t></m:r></m:e></m:rad></m:den></m:f></m:oMath>"#;
        assert_eq!(
            convert_omml_to_latex(formula.as_bytes()).unwrap(),
            r"y=\frac{x^{2}}{\sqrt{3}}"
        );
    }

    #[test]
    fn preserves_indexed_roots_and_subscripts() {
        let formula = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:sSub><m:e><m:r><m:t>a</m:t></m:r></m:e><m:sub><m:r><m:t>1</m:t></m:r></m:sub></m:sSub><m:r><m:t>=</m:t></m:r><m:rad><m:deg><m:r><m:t>3</m:t></m:r></m:deg><m:e><m:r><m:t>x</m:t></m:r></m:e></m:rad></m:oMath>"#;
        assert_eq!(
            convert_omml_to_latex(formula.as_bytes()).unwrap(),
            r"a_{1}=\sqrt[3]{x}"
        );
    }

    #[test]
    fn keeps_linear_exponent_syntax_in_math_text() {
        let formula = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r><m:t>x^2+1</m:t></m:r></m:oMath>"#;
        assert_eq!(convert_omml_to_latex(formula.as_bytes()).unwrap(), "x^2+1");
    }

    #[test]
    fn rejects_unknown_structures_instead_of_silently_changing_the_formula() {
        let formula = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:unsupported><m:e><m:r><m:t>x</m:t></m:r></m:e></m:unsupported></m:oMath>"#;
        assert!(convert_omml_to_latex(formula.as_bytes()).is_err());
    }
}
