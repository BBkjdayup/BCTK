/// Resource limits applied before and while a DOCX package is parsed.
///
/// The defaults are intentionally much larger than an ordinary worksheet but
/// finite, so a corrupt or hostile ZIP cannot consume unbounded memory, disk or
/// CPU time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocxLimits {
    /// Allows the Word-import path to inspect embedded MathType OLE objects.
    ///
    /// This remains disabled for generic DOCX analysis, templates and export.
    /// Even when enabled, the importer only reads a bounded `Equation Native`
    /// stream and rejects every non-MathType embedded object.
    pub allow_mathtype_ole: bool,
    /// Maximum size of the complete `.docx` file.
    pub max_archive_bytes: u64,
    /// Maximum number of ZIP central-directory entries.
    pub max_entries: usize,
    /// Maximum sum of all declared uncompressed entry sizes.
    pub max_total_uncompressed_bytes: u64,
    /// Maximum declared size of any non-media entry.
    pub max_part_uncompressed_bytes: u64,
    /// Tighter maximum for XML and relationship parts.
    pub max_xml_part_bytes: u64,
    /// Tighter maximum for entries under common media directories.
    pub max_media_part_bytes: u64,
    /// Maximum XML element nesting depth.
    pub max_xml_depth: usize,
    /// Compression ratios above this value are diagnosed as suspicious.
    pub suspicious_compression_ratio: u64,
}

impl Default for DocxLimits {
    fn default() -> Self {
        Self {
            allow_mathtype_ole: false,
            max_archive_bytes: 100 * 1024 * 1024,
            max_entries: 4_096,
            max_total_uncompressed_bytes: 512 * 1024 * 1024,
            max_part_uncompressed_bytes: 128 * 1024 * 1024,
            max_xml_part_bytes: 64 * 1024 * 1024,
            max_media_part_bytes: 64 * 1024 * 1024,
            max_xml_depth: 256,
            suspicious_compression_ratio: 200,
        }
    }
}

impl DocxLimits {
    pub(crate) fn part_limit(&self, name: &str) -> u64 {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".xml") || lower.ends_with(".rels") {
            self.max_xml_part_bytes
        } else if lower.contains("/media/") || lower.starts_with("word/media/") {
            self.max_media_part_bytes
        } else {
            self.max_part_uncompressed_bytes
        }
    }
}
