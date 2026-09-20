//! Derived search text and exact identities. Presentation IDs and image URLs
//! are not content identities; managed images use their stored SHA-256.
use std::collections::{BTreeSet, HashMap};

use ego_tree::NodeRef;
use scraper::{Html, Node};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use unicode_normalization::UnicodeNormalization;

use super::models::{
    CommandError, CommandResult, QuestionDraftApi, QuestionOptionApi, RichContentApi,
};

pub const FINGERPRINT_VERSION: i64 = 2;
pub type ResourceHashes = HashMap<String, String>;

#[derive(Default)]
struct Content {
    tokens: Vec<(String, String)>,
    text: String,
    meaningful: bool,
    image_ids: BTreeSet<String>,
    unverified: bool,
}

fn visible(value: &str) -> bool {
    value
        .chars()
        .any(|c| !c.is_whitespace() && !matches!(c, '\u{200b}' | '\u{feff}'))
}

fn normalized_text(value: &str) -> String {
    value
        .nfkc()
        .flat_map(char::to_lowercase)
        .filter(|c| !c.is_whitespace() && !matches!(c, '\u{200b}' | '\u{feff}'))
        .collect()
}

impl Content {
    fn text(&mut self, value: &str) {
        self.text.push_str(value);
        self.meaningful |= visible(value);
        let normalized = normalized_text(value);
        if normalized.is_empty() {
            return;
        }
        if let Some((kind, previous)) = self.tokens.last_mut()
            && kind == "text"
        {
            previous.push_str(&normalized);
        } else {
            self.tokens.push(("text".into(), normalized));
        }
    }

    fn formula(&mut self, latex: &str) {
        if !visible(latex) {
            return;
        }
        self.meaningful = true;
        self.text.push_str(latex.trim());
        // LaTeX is case-sensitive, and whitespace inside \text{} is meaningful.
        self.tokens
            .push(("formula".into(), latex.trim().nfkc().collect()));
    }

    fn image(&mut self, id: &str, source: &str, alt: &str, hashes: &ResourceHashes) {
        if !id.trim().is_empty() {
            self.meaningful = true;
            self.image_ids.insert(id.to_owned());
            self.unverified |= !hashes.contains_key(id);
            let identity = hashes
                .get(id)
                .map(|hash| format!("sha256:{hash}"))
                .unwrap_or_else(|| format!("unresolved-resource:{id}"));
            self.tokens.push(("image".into(), identity));
        } else if !source.is_empty() {
            self.unverified = true;
            // Legacy unmanaged images remain distinguishable. Existing resource
            // and export validation still decides whether they can be used.
            self.tokens.push((
                "unmanaged-image".into(),
                hex(&Sha256::digest(source.as_bytes())),
            ));
        }
        self.text.push_str(alt);
    }

    fn open(&mut self, tag: &str, detail: String) {
        self.tokens.push((format!("open:{tag}"), detail));
    }

    fn close(&mut self, tag: &str) {
        self.tokens.push((format!("close:{tag}"), String::new()));
        if !matches!(tag, "sup" | "sub") {
            self.text.push('\n');
        }
    }
}

fn structural(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "ul"
            | "ol"
            | "li"
            | "table"
            | "tr"
            | "td"
            | "th"
            | "sup"
            | "sub"
    )
}

fn html_node(node: NodeRef<'_, Node>, output: &mut Content, hashes: &ResourceHashes) {
    // HTML parsers accept deeply nested input; walk on the heap rather than
    // recursively consuming the thread stack during import or migration.
    let mut pending = vec![(node, false)];
    while let Some((node, closing)) = pending.pop() {
        if closing {
            if let Node::Element(element) = node.value() {
                output.close(element.name());
            }
            continue;
        }
        match node.value() {
            Node::Text(text) => output.text(text),
            Node::Element(element) => {
                let tag = element.name();
                if matches!(tag, "script" | "style" | "template") {
                    continue;
                }
                if let Some(latex) = element.attr("data-latex") {
                    output.formula(latex);
                    continue;
                }
                if tag == "img" {
                    output.image(
                        element.attr("data-resource-id").unwrap_or(""),
                        element.attr("src").unwrap_or(""),
                        element.attr("alt").unwrap_or(""),
                        hashes,
                    );
                    continue;
                }
                if tag == "br" {
                    output.tokens.push(("break".into(), String::new()));
                    output.text.push('\n');
                    continue;
                }
                if matches!(tag, "svg" | "math" | "object") {
                    output.unverified = true;
                    if let Some(element) = scraper::ElementRef::wrap(node) {
                        output.tokens.push((
                            "opaque".into(),
                            hex(&Sha256::digest(element.html().as_bytes())),
                        ));
                    }
                    continue;
                }
                let is_structural = structural(tag);
                if is_structural {
                    let detail = if matches!(tag, "td" | "th") {
                        format!(
                            "{},{}",
                            element.attr("colspan").unwrap_or("1"),
                            element.attr("rowspan").unwrap_or("1")
                        )
                    } else {
                        String::new()
                    };
                    output.open(tag, detail);
                }
                if is_structural {
                    pending.push((node, true));
                }
                pending.extend(node.children().rev().map(|child| (child, false)));
            }
            _ => {
                pending.extend(node.children().rev().map(|child| (child, false)));
            }
        }
    }
}

fn json_node(node: &Value, output: &mut Content, hashes: &ResourceHashes) {
    let kind = node["type"].as_str().unwrap_or("");
    let attrs = &node["attrs"];
    match kind {
        "text" => {
            let marks = node["marks"].as_array().map(Vec::as_slice).unwrap_or(&[]);
            let tags: Vec<&str> = marks
                .iter()
                .filter_map(|mark| match mark["type"].as_str() {
                    Some("superscript") => Some("sup"),
                    Some("subscript") => Some("sub"),
                    _ => None,
                })
                .collect();
            for tag in &tags {
                output.open(tag, String::new());
            }
            output.text(node["text"].as_str().unwrap_or(""));
            for tag in tags.iter().rev() {
                output.close(tag);
            }
            return;
        }
        "mathNode" => {
            output.formula(attrs["latex"].as_str().unwrap_or(""));
            return;
        }
        "image" => {
            output.image(
                attrs["resourceId"].as_str().unwrap_or(""),
                attrs["src"].as_str().unwrap_or(""),
                attrs["alt"].as_str().unwrap_or(""),
                hashes,
            );
            return;
        }
        "hardBreak" => {
            output.tokens.push(("break".into(), String::new()));
            output.text.push('\n');
            return;
        }
        _ => {}
    }
    let heading = format!("h{}", attrs["level"].as_u64().unwrap_or(1).clamp(1, 6));
    let tag = match kind {
        "paragraph" => "p",
        "heading" => &heading,
        "blockquote" => "blockquote",
        "bulletList" => "ul",
        "orderedList" => "ol",
        "listItem" => "li",
        "table" => "table",
        "tableRow" => "tr",
        "tableCell" => "td",
        "tableHeader" => "th",
        _ => "",
    };
    if !tag.is_empty() {
        let detail = if matches!(tag, "td" | "th") {
            format!(
                "{},{}",
                attrs["colspan"].as_u64().unwrap_or(1),
                attrs["rowspan"].as_u64().unwrap_or(1)
            )
        } else {
            String::new()
        };
        output.open(tag, detail);
    }
    if let Some(children) = node["content"].as_array() {
        for child in children {
            json_node(child, output, hashes);
        }
    }
    if !tag.is_empty() {
        output.close(tag);
    }
}

fn content(value: &RichContentApi, hashes: &ResourceHashes) -> Content {
    let mut result = Content::default();
    if value.html.trim().is_empty() {
        if let Some(document) = &value.document {
            json_node(document, &mut result, hashes);
        } else {
            result.open("p", String::new());
            result.text(&value.plain_text);
            result.close("p");
        }
    } else {
        let fragment = Html::parse_fragment(&value.html);
        html_node(fragment.tree.root(), &mut result, hashes);
        if let Some(document) = &value.document {
            let mut structured = Content::default();
            json_node(document, &mut structured, hashes);
            if structured.tokens != result.tokens {
                result.unverified = true;
                result
                    .tokens
                    .push(("document-differs-from-html".into(), String::new()));
                result.tokens.extend(structured.tokens);
            }
            result.image_ids.extend(structured.image_ids);
            result.unverified |= structured.unverified;
        }
    }
    if let Some(xml) = value
        .source_ooxml
        .as_ref()
        .filter(|xml| !xml.trim().is_empty())
    {
        result
            .tokens
            .push(("source-ooxml".into(), hex(&Sha256::digest(xml.as_bytes()))));
    }
    result
}

pub fn searchable_text(value: &RichContentApi) -> String {
    content(value, &HashMap::new()).text.trim().to_owned()
}

pub fn has_content(value: &RichContentApi) -> bool {
    content(value, &HashMap::new()).meaningful
}

pub fn normalize_search_fields(draft: &QuestionDraftApi) -> QuestionDraftApi {
    let mut draft = draft.clone();
    draft.stem.plain_text = searchable_text(&draft.stem);
    draft.answer.plain_text = searchable_text(&draft.answer);
    draft.explanation.plain_text = searchable_text(&draft.explanation);
    for option in &mut draft.options {
        option.content.plain_text = searchable_text(&option.content);
    }
    draft
}

fn fields(draft: &QuestionDraftApi) -> impl Iterator<Item = &RichContentApi> {
    [&draft.stem, &draft.answer, &draft.explanation]
        .into_iter()
        .chain(draft.options.iter().map(|option| &option.content))
}

pub async fn resource_hashes(
    connection: &mut SqliteConnection,
    drafts: &[&QuestionDraftApi],
) -> CommandResult<ResourceHashes> {
    let mut ids = BTreeSet::new();
    for draft in drafts {
        for field in fields(draft) {
            ids.extend(content(field, &HashMap::new()).image_ids);
        }
    }
    let mut hashes = HashMap::new();
    for id in ids {
        if let Some(hash) =
            sqlx::query_scalar::<_, Vec<u8>>("SELECT sha256 FROM resources WHERE id = ?")
                .bind(&id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(CommandError::database)?
        {
            hashes.insert(id, hex(&hash));
        }
    }
    Ok(hashes)
}

pub fn fingerprint(draft: &QuestionDraftApi, hashes: &ResourceHashes) -> Vec<u8> {
    // JSON frames each field unambiguously, including table/superscript
    // structure and opaque original content that cannot safely be flattened.
    let tokens: Vec<_> = std::iter::once(&draft.stem)
        .chain(draft.options.iter().map(|option| &option.content))
        .chain([&draft.answer, &draft.explanation])
        .map(|field| content(field, hashes).tokens)
        .collect();
    let identity = serde_json::json!([
        FINGERPRINT_VERSION,
        draft.question_type,
        draft.options.len(),
        tokens
    ]);
    Sha256::digest(identity.to_string().as_bytes()).to_vec()
}

pub fn comparable(draft: &QuestionDraftApi, hashes: &ResourceHashes) -> bool {
    fields(draft).all(|field| !content(field, hashes).unverified)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Rebuild derived fields without changing business JSON, question versions,
/// timestamps or paper snapshots. During a schema upgrade this runs on the
/// validated candidate database, before the original file is replaced.
pub async fn rebuild_pending(
    connection: &mut SqliteConnection,
    only_id: Option<&str>,
) -> CommandResult<usize> {
    let mut rebuilt = 0;
    loop {
        let row = sqlx::query_as::<_, (String, String, String, String, String, String, String)>(
            "SELECT id, question_type, subject_id, chapter_id, stem_json, answer_json, explanation_json \
             FROM questions WHERE fingerprint_version <> 2 AND (? IS NULL OR id = ?) ORDER BY id LIMIT 1"
        ).bind(only_id).bind(only_id).fetch_optional(&mut *connection).await.map_err(CommandError::database)?;
        let Some((id, question_type, subject_id, chapter_id, stem, answer, explanation)) = row
        else {
            break;
        };
        let rows = sqlx::query_as::<_, (String, i64, String)>(
            "SELECT id, position, content_json FROM question_options WHERE question_id = ? ORDER BY position"
        ).bind(&id).fetch_all(&mut *connection).await.map_err(CommandError::database)?;
        let mut options = Vec::with_capacity(rows.len());
        for (option_id, position, json) in rows {
            options.push(QuestionOptionApi {
                id: option_id,
                position: u32::try_from(position).map_err(CommandError::database)?,
                content: serde_json::from_str(&json).map_err(CommandError::database)?,
            });
        }
        let draft = normalize_search_fields(&QuestionDraftApi {
            answer_review_required: false,
            id: None,
            question_id: Some(id.clone()),
            question_type,
            subject_id,
            chapter_id,
            stem: serde_json::from_str(&stem).map_err(CommandError::database)?,
            answer: serde_json::from_str(&answer).map_err(CommandError::database)?,
            explanation: serde_json::from_str(&explanation).map_err(CommandError::database)?,
            options,
            tag_ids: Vec::new(),
            resource_refs: Vec::new(),
            base_content_version: None,
        });
        let hashes = resource_hashes(connection, &[&draft]).await?;
        let fingerprint = fingerprint(&draft, &hashes);
        let options_plain = draft
            .options
            .iter()
            .map(|o| o.content.plain_text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        sqlx::query("UPDATE questions SET fingerprint_version = 2, fingerprint_comparable = ?, exact_fingerprint = ?, stem_plain = ?, \
            options_plain = ?, answer_plain = ?, explanation_plain = ? WHERE id = ?")
            .bind(comparable(&draft, &hashes)).bind(fingerprint).bind(&draft.stem.plain_text).bind(options_plain)
            .bind(&draft.answer.plain_text).bind(&draft.explanation.plain_text).bind(&id)
            .execute(&mut *connection).await.map_err(CommandError::database)?;
        for option in &draft.options {
            sqlx::query("UPDATE question_options SET plain_text = ? WHERE id = ?")
                .bind(&option.content.plain_text)
                .bind(&option.id)
                .execute(&mut *connection)
                .await
                .map_err(CommandError::database)?;
        }
        rebuilt += 1;
    }
    Ok(rebuilt)
}
