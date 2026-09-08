//! Offline, bounded DOCX/OPC inspection and extraction primitives.
//!
//! This module deliberately does not try to model the whole WordprocessingML
//! standard.  It validates the ZIP/OPC container first, then exposes a narrow
//! reader for the pieces Zhitiku needs: visible paragraph text, raw OMML
//! fragments and template anchors.  Untouched ZIP members can be copied as-is
//! during export so unsupported Word features are not needlessly rewritten.

mod diagnostic;
mod document;
mod error;
mod export;
mod images;
mod limits;
mod mathtype;
mod omml;
mod package;
mod paper;
mod rich_content;
mod template;
mod template_config;
mod template_package;
mod types;
mod xml;

pub use diagnostic::{Diagnostic, DiagnosticSeverity};
pub use document::{
    DocumentTable, FORMULA_PLACEHOLDER, Paragraph, parse_document_xml, read_document_from_docx,
};
pub use error::{DocxError, DocxResult};
pub use export::{
    RawCopyExport, raw_copy_with_replacements, raw_copy_with_replacements_and_additions,
    raw_copy_with_replacements_and_removals,
};
pub(crate) use images::inspect_raster_image;
pub use images::{ExtractedImageOccurrence, read_images_from_docx};
pub use limits::DocxLimits;
pub use mathtype::{ExtractedMathTypeOccurrence, read_mathtype_from_docx};
pub use omml::{ExtractedEditableFormulaOccurrence, read_omml_from_docx};
#[cfg(test)]
pub(crate) use package::test_support::minimal_docx;
pub use package::{PackageInspection, PackageKind, inspect_docx};
pub use paper::{
    PaperContentMode, PaperImageRelationship, PaperPageSetupOverride, PaperRichDocument,
    PaperRichItem, override_document_page_setup, render_rich_paper_document_xml_with_styles,
};
#[cfg(test)]
pub use rich_content::PaperBlock;
pub use rich_content::{PaperRichContent, parse_rich_content};
pub use template::{AnchorKind, TemplateAnchor, check_template_anchors};
#[cfg(test)]
pub use template_config::ParagraphStylePrototype;
pub use template_config::{
    TemplateFontTheme, TemplateLayoutBlock, TemplatePageSetup, TemplateRegionConfiguration,
    TemplateRegionPlacement, TemplateStyleProfile, TemplateStyleSelection,
    configure_template_package, preview_template_configuration, preview_template_layout,
};
pub use template_package::{
    TemplatePackageAnalysis, analyze_template_package, analyze_template_package_with_requirements,
};
pub use types::ByteSpan;
