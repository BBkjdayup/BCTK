//! Converts the bounded HTML stored in `RichContent` into a small, explicit
//! document model that the DOCX writer can render without executing HTML.

use ego_tree::NodeRef;
use scraper::{Html, Node};

const MAX_RICH_NODES: usize = 100_000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaperRichContent {
    pub blocks: Vec<PaperBlock>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaperBlock {
    Paragraph(PaperParagraph),
    Table(PaperTable),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaperParagraph {
    pub inlines: Vec<PaperInline>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaperInline {
    Text(PaperTextRun),
    Formula { latex: String },
    Image(PaperImage),
    LineBreak,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaperTextRun {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub superscript: bool,
    pub subscript: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaperImage {
    pub resource_id: String,
    pub width_px: Option<u32>,
    pub height_px: Option<u32>,
    pub alt: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaperTable {
    pub rows: Vec<PaperTableRow>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaperTableRow {
    pub cells: Vec<PaperTableCell>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaperTableCell {
    pub blocks: Vec<PaperBlock>,
    pub column_span: u32,
    pub row_span: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RichContentParse {
    pub content: PaperRichContent,
    pub unsupported: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default)]
struct TextStyle {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    superscript: bool,
    subscript: bool,
}

pub fn parse_rich_content(html: &str, plain_text: &str) -> RichContentParse {
    if html.trim().is_empty() {
        return RichContentParse {
            content: plain_content(plain_text),
            unsupported: Vec::new(),
        };
    }

    let fragment = Html::parse_fragment(html);
    if fragment.tree.nodes().count() > MAX_RICH_NODES {
        return RichContentParse {
            content: plain_content(plain_text),
            unsupported: vec!["富文本节点数量超过安全上限".to_owned()],
        };
    }

    let mut result = RichContentParse::default();
    for child in fragment.tree.root().children() {
        collect_blocks(child, &mut result.content.blocks, &mut result.unsupported);
    }
    if result.content.blocks.is_empty() && !plain_text.is_empty() {
        result.content = plain_content(plain_text);
    }
    result
}

fn plain_content(value: &str) -> PaperRichContent {
    let normalized = value.replace("\r\n", "\n").replace('\r', "\n");
    let blocks = normalized
        .split('\n')
        .map(|line| {
            PaperBlock::Paragraph(PaperParagraph {
                inlines: vec![PaperInline::Text(PaperTextRun {
                    text: line.to_owned(),
                    ..PaperTextRun::default()
                })],
            })
        })
        .collect();
    PaperRichContent { blocks }
}

fn collect_blocks(
    node: NodeRef<'_, Node>,
    blocks: &mut Vec<PaperBlock>,
    unsupported: &mut Vec<String>,
) {
    match node.value() {
        Node::Text(text) => {
            if !text.trim().is_empty() {
                let mut inlines = Vec::new();
                collect_inlines(node, TextStyle::default(), &mut inlines, unsupported);
                push_paragraph(blocks, inlines);
            }
        }
        Node::Element(element) => match element.name() {
            "html" | "body" => {
                for child in node.children() {
                    collect_blocks(child, blocks, unsupported);
                }
            }
            "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" => {
                let mut inlines = Vec::new();
                for child in node.children() {
                    collect_inlines(child, TextStyle::default(), &mut inlines, unsupported);
                }
                push_paragraph(blocks, inlines);
            }
            "ul" => collect_list(node, false, blocks, unsupported),
            "ol" => collect_list(node, true, blocks, unsupported),
            "table" => blocks.push(PaperBlock::Table(parse_table(node, unsupported))),
            "script" | "style" | "iframe" | "object" | "embed" => {
                unsupported.push(format!("不支持的 HTML 元素 <{}>", element.name()));
            }
            _ => {
                let mut inlines = Vec::new();
                collect_inlines(node, TextStyle::default(), &mut inlines, unsupported);
                push_paragraph(blocks, inlines);
            }
        },
        _ => {
            for child in node.children() {
                collect_blocks(child, blocks, unsupported);
            }
        }
    }
}

fn collect_list(
    node: NodeRef<'_, Node>,
    ordered: bool,
    blocks: &mut Vec<PaperBlock>,
    unsupported: &mut Vec<String>,
) {
    let mut index = 0usize;
    for child in node.children() {
        let Node::Element(element) = child.value() else {
            continue;
        };
        if element.name() != "li" {
            continue;
        }
        index += 1;
        let mut inlines = vec![PaperInline::Text(PaperTextRun {
            text: if ordered {
                format!("{index}. ")
            } else {
                "• ".to_owned()
            },
            ..PaperTextRun::default()
        })];
        for grandchild in child.children() {
            collect_inlines(grandchild, TextStyle::default(), &mut inlines, unsupported);
        }
        push_paragraph(blocks, inlines);
    }
}

fn parse_table(node: NodeRef<'_, Node>, unsupported: &mut Vec<String>) -> PaperTable {
    let mut rows = Vec::new();
    collect_table_rows(node, &mut rows, unsupported);
    PaperTable { rows }
}

fn collect_table_rows(
    node: NodeRef<'_, Node>,
    rows: &mut Vec<PaperTableRow>,
    unsupported: &mut Vec<String>,
) {
    for child in node.children() {
        let Some(element) = child.value().as_element() else {
            continue;
        };
        match element.name() {
            "tr" => {
                let mut cells = Vec::new();
                for cell_node in child.children() {
                    let Some(cell_element) = cell_node.value().as_element() else {
                        continue;
                    };
                    if !matches!(cell_element.name(), "td" | "th") {
                        continue;
                    }
                    let mut blocks = Vec::new();
                    for cell_child in cell_node.children() {
                        collect_blocks(cell_child, &mut blocks, unsupported);
                    }
                    if blocks.is_empty() {
                        blocks.push(PaperBlock::Paragraph(PaperParagraph::default()));
                    }
                    let row_span = positive_attribute(cell_element.attr("rowspan"));
                    if row_span > 1 {
                        unsupported.push("暂不支持带纵向合并单元格的表格".to_owned());
                    }
                    cells.push(PaperTableCell {
                        blocks,
                        column_span: positive_attribute(cell_element.attr("colspan")),
                        row_span,
                    });
                }
                if !cells.is_empty() {
                    rows.push(PaperTableRow { cells });
                }
            }
            "table" => unsupported.push("暂不支持表格中嵌套表格".to_owned()),
            _ => collect_table_rows(child, rows, unsupported),
        }
    }
}

fn positive_attribute(value: Option<&str>) -> u32 {
    value
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|value| (1..=64).contains(value))
        .unwrap_or(1)
}

fn push_paragraph(blocks: &mut Vec<PaperBlock>, inlines: Vec<PaperInline>) {
    blocks.push(PaperBlock::Paragraph(PaperParagraph { inlines }));
}

fn collect_inlines(
    node: NodeRef<'_, Node>,
    style: TextStyle,
    output: &mut Vec<PaperInline>,
    unsupported: &mut Vec<String>,
) {
    match node.value() {
        Node::Text(text) => push_text(output, text, style),
        Node::Element(element) => {
            let name = element.name();
            if is_formula(element) {
                match element.attr("data-latex").map(str::trim) {
                    Some(latex) if !latex.is_empty() => output.push(PaperInline::Formula {
                        latex: latex.to_owned(),
                    }),
                    _ => unsupported.push("发现缺少 LaTeX 内容的公式节点".to_owned()),
                }
                return;
            }
            match name {
                "img" => match element.attr("data-resource-id").map(str::trim) {
                    Some(resource_id) if !resource_id.is_empty() => {
                        output.push(PaperInline::Image(PaperImage {
                            resource_id: resource_id.to_owned(),
                            width_px: pixel_attribute(element.attr("width")),
                            height_px: pixel_attribute(element.attr("height")),
                            alt: element.attr("alt").unwrap_or("题目图片").to_owned(),
                        }));
                    }
                    _ => unsupported.push("发现没有受管资源 ID 的图片".to_owned()),
                },
                "br" => output.push(PaperInline::LineBreak),
                "table" => unsupported.push("表格不能嵌入普通文字段落".to_owned()),
                "svg" | "math" | "canvas" | "video" | "audio" | "iframe" | "object" | "embed" => {
                    unsupported.push(format!("不支持的 HTML 元素 <{name}>"))
                }
                "script" | "style" => {
                    unsupported.push(format!("不允许导出的 HTML 元素 <{name}>"));
                }
                _ => {
                    let mut nested = style;
                    match name {
                        "strong" | "b" => nested.bold = true,
                        "em" | "i" => nested.italic = true,
                        "u" => nested.underline = true,
                        "s" | "strike" | "del" => nested.strike = true,
                        "sup" => nested.superscript = true,
                        "sub" => nested.subscript = true,
                        _ => {}
                    }
                    for child in node.children() {
                        collect_inlines(child, nested, output, unsupported);
                    }
                }
            }
        }
        _ => {
            for child in node.children() {
                collect_inlines(child, style, output, unsupported);
            }
        }
    }
}

fn is_formula(element: &scraper::node::Element) -> bool {
    element.attr("data-latex").is_some()
        || element.attr("class").is_some_and(|classes| {
            classes
                .split_ascii_whitespace()
                .any(|value| value == "math-node")
        })
}

fn pixel_attribute(value: Option<&str>) -> Option<u32> {
    value
        .and_then(|value| value.trim().trim_end_matches("px").parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0 && *value <= 20_000.0)
        .map(|value| value.round() as u32)
}

fn push_text(output: &mut Vec<PaperInline>, text: &str, style: TextStyle) {
    if text.is_empty() {
        return;
    }
    if let Some(PaperInline::Text(previous)) = output.last_mut()
        && previous.bold == style.bold
        && previous.italic == style.italic
        && previous.underline == style.underline
        && previous.strike == style.strike
        && previous.superscript == style.superscript
        && previous.subscript == style.subscript
    {
        previous.text.push_str(text);
        return;
    }
    output.push(PaperInline::Text(PaperTextRun {
        text: text.to_owned(),
        bold: style.bold,
        italic: style.italic,
        underline: style.underline,
        strike: style.strike,
        superscript: style.superscript,
        subscript: style.subscript,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_formula_and_managed_image_dimensions() {
        let parsed = parse_rich_content(
            r#"<p><strong>题目</strong><span class="math-node" data-latex="\frac{1}{2}">x</span><img data-resource-id="image-1" width="320" alt="图"></p>"#,
            "题目",
        );
        assert!(parsed.unsupported.is_empty());
        let PaperBlock::Paragraph(paragraph) = &parsed.content.blocks[0] else {
            panic!("expected paragraph")
        };
        assert!(matches!(&paragraph.inlines[0], PaperInline::Text(run) if run.bold));
        assert!(
            matches!(&paragraph.inlines[1], PaperInline::Formula { latex } if latex == r"\frac{1}{2}")
        );
        assert!(
            matches!(&paragraph.inlines[2], PaperInline::Image(image) if image.resource_id == "image-1" && image.width_px == Some(320))
        );
    }

    #[test]
    fn parses_native_table_cells() {
        let parsed = parse_rich_content(
            "<table><tbody><tr><td><p>x</p></td><td><p>1</p></td></tr><tr><td><p>y</p></td><td><p>0</p></td></tr></tbody></table>",
            "x 1 y 0",
        );
        assert!(parsed.unsupported.is_empty());
        assert!(
            matches!(&parsed.content.blocks[0], PaperBlock::Table(table) if table.rows.len() == 2 && table.rows[0].cells.len() == 2)
        );
    }

    #[test]
    fn rejects_unmanaged_image() {
        let parsed = parse_rich_content(r#"<p><img src="https://example.invalid/a.png"></p>"#, "");
        assert_eq!(parsed.unsupported, vec!["发现没有受管资源 ID 的图片"]);
    }
}
