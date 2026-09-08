//! Safe conversion of the narrow OMML subset used by Word-import documents.
//!
//! `document.rs` already validates and bounds the source XML before retaining
//! exact OMML fragments. This module extracts their visible math text and
//! converts common Unicode math tokens into the editable LaTeX model used by
//! the frontend. The current converter intentionally follows Word's visible
//! math text; richer structural OMML support can be added independently.

use std::io::{Read, Seek};

use quick_xml::{events::Event, reader::NsReader};

use super::{DocxError, DocxLimits, DocxResult, read_document_from_docx, xml::local_name};

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
    let mut text_depth = 0usize;
    let mut visible_text = String::new();

    loop {
        let position = reader.buffer_position();
        match reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(OMML_PART, reader.error_position(), error))?
        {
            Event::Start(ref start) => {
                if local_name(start) == b"t" {
                    text_depth += 1;
                }
            }
            Event::End(ref end) => {
                if end.local_name().as_ref() == b"t" {
                    text_depth = text_depth.saturating_sub(1);
                }
            }
            Event::Text(ref text) if text_depth > 0 => {
                let decoded = text
                    .xml10_content()
                    .map_err(|error| DocxError::xml(OMML_PART, position, error))?;
                visible_text.push_str(&decoded);
            }
            Event::CData(ref text) if text_depth > 0 => {
                let decoded = text
                    .xml10_content()
                    .map_err(|error| DocxError::xml(OMML_PART, position, error))?;
                visible_text.push_str(&decoded);
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

    let latex = unicode_math_text_to_latex(&visible_text);
    if latex.trim().is_empty() {
        return Err(invalid_omml(
            "the OMML formula did not contain editable mathematical text",
        ));
    }
    Ok(latex)
}

fn unicode_math_text_to_latex(text: &str) -> String {
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
            '−' | '–' => output.push('-'),
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
            '^' => output.push_str(r"\hat{}"),
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
            r"\sin \alpha \cos \alpha ,\quad \sin \alpha -\cos \alpha"
        );
        assert_eq!(convert_omml_to_latex(point.as_bytes()).unwrap(), "A(8,-6)");
    }
}
