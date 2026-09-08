use std::io::{Read, Seek, SeekFrom};

use zip::ZipArchive;

use super::{
    Diagnostic, DiagnosticSeverity, DocxLimits, DocxResult, PackageKind, TemplateAnchor,
    package::{archive_size_diagnostic, inspect_archive_with_size, read_part_limited},
    template::check_template_anchors,
};

const DOCUMENT_PART: &str = "word/document.xml";

/// One-pass package-level template analysis suitable for import and later
/// database commands. The ZIP package is inspected once, and the same archive
/// instance is then used to read `word/document.xml` and locate anchors.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TemplatePackageAnalysis {
    pub package_kind: PackageKind,
    pub archive_bytes: u64,
    pub part_count: usize,
    pub total_uncompressed_bytes: u64,
    pub document_xml: Option<Vec<u8>>,
    pub anchors: Vec<TemplateAnchor>,
    pub diagnostics: Vec<Diagnostic>,
}

impl TemplatePackageAnalysis {
    pub fn is_acceptable(&self) -> bool {
        self.document_xml.is_some()
            && !self
                .diagnostics
                .iter()
                .any(|item| item.severity == DiagnosticSeverity::Error)
    }

    pub fn anchors_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a TemplateAnchor> {
        self.anchors
            .iter()
            .filter(move |anchor| anchor.name == name)
    }
}

pub fn analyze_template_package<R: Read + Seek>(
    source: R,
    limits: &DocxLimits,
) -> DocxResult<TemplatePackageAnalysis> {
    analyze_template_package_with_requirements(source, &[], limits)
}

pub fn analyze_template_package_with_requirements<R: Read + Seek>(
    mut source: R,
    required_anchor_names: &[&str],
    limits: &DocxLimits,
) -> DocxResult<TemplatePackageAnalysis> {
    let archive_bytes = source.seek(SeekFrom::End(0))?;
    if let Some(diagnostic) = archive_size_diagnostic(archive_bytes, limits) {
        return Ok(TemplatePackageAnalysis {
            archive_bytes,
            diagnostics: vec![diagnostic],
            ..TemplatePackageAnalysis::default()
        });
    }
    source.seek(SeekFrom::Start(0))?;

    let mut archive = ZipArchive::new(source)?;
    let inspection = inspect_archive_with_size(&mut archive, Some(archive_bytes), limits)?;
    let mut analysis = TemplatePackageAnalysis {
        package_kind: inspection.package_kind,
        archive_bytes,
        part_count: inspection.parts.len(),
        total_uncompressed_bytes: inspection.total_uncompressed_bytes,
        document_xml: None,
        anchors: Vec::new(),
        diagnostics: inspection.diagnostics,
    };
    if analysis
        .diagnostics
        .iter()
        .any(|item| item.severity == DiagnosticSeverity::Error)
    {
        return Ok(analysis);
    }

    let document_xml = read_part_limited(&mut archive, DOCUMENT_PART, limits.max_xml_part_bytes)?;
    let anchor_report = check_template_anchors(&document_xml, required_anchor_names, limits)?;
    analysis.anchors = anchor_report.anchors;
    analysis.diagnostics.extend(anchor_report.diagnostics);
    analysis.document_xml = Some(document_xml);
    Ok(analysis)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::docx::package::test_support::minimal_docx;

    const TEMPLATE: &str = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:body>
  <w:sdt><w:sdtPr><w:tag w:val="ZT_TITLE"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>Title</w:t></w:r></w:p></w:sdtContent></w:sdt>
  <w:p><w:r><w:t>{{ZT_QUESTIONS}}</w:t></w:r></w:p>
</w:body></w:document>"#;

    #[test]
    fn inspects_once_and_returns_document_and_anchors() {
        let bytes = minimal_docx(TEMPLATE);
        let analysis = analyze_template_package_with_requirements(
            Cursor::new(bytes),
            &["ZT_TITLE", "ZT_QUESTIONS"],
            &DocxLimits::default(),
        )
        .unwrap();

        assert!(analysis.is_acceptable(), "{:?}", analysis.diagnostics);
        assert_eq!(analysis.package_kind, PackageKind::Document);
        assert_eq!(analysis.part_count, 4);
        assert!(analysis.document_xml.is_some());
        assert_eq!(analysis.anchors.len(), 2);
    }

    #[test]
    fn does_not_return_document_bytes_for_rejected_packages() {
        let limits = DocxLimits {
            max_archive_bytes: 4,
            ..DocxLimits::default()
        };
        let analysis =
            analyze_template_package(Cursor::new(minimal_docx(TEMPLATE)), &limits).unwrap();

        assert!(!analysis.is_acceptable());
        assert!(analysis.document_xml.is_none());
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|item| item.code == "DOCX_ARCHIVE_TOO_LARGE")
        );
    }
}
