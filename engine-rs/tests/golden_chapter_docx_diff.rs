//! R50/1652 号：docx 导出纯函数族 golden 差分（TS 生产向量 → Rust 镜像
//! 对拍）。四组向量（escape/bodyParagraphs/paragraph/document）+ 三常量
//! part sha256——向量由 TS 生产函数导出（golden-chapter-docx.test.ts 同
//! 文件锁定），双端逐位一致 = duel 条目级可比的前提。

use inkos_engine::interaction::export_artifact::{
    docx_body_paragraphs, docx_document_xml, docx_escape_xml, docx_paragraph_xml, DocxChapterInput,
    DOCX_CONTENT_TYPES_XML, DOCX_RELS_XML, DOCX_STYLES_XML,
};
use inkos_engine::models::book::BookVolume;
use sha2::{Digest, Sha256};

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/chapter-docx-vectors.json");

#[derive(serde::Deserialize)]
struct VectorFile {
    #[serde(rename = "staticParts")]
    static_parts: std::collections::BTreeMap<String, String>,
    escape: Vec<TextVector>,
    #[serde(rename = "bodyParagraphs")]
    body_paragraphs: Vec<ParagraphsVector>,
    paragraph: Vec<ParagraphVector>,
    document: Vec<DocumentVector>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextVector {
    name: String,
    input: String,
    expected: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParagraphsVector {
    name: String,
    input: String,
    expected: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParagraphVector {
    name: String,
    style_id: String,
    text: String,
    expected: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocumentVector {
    name: String,
    book_title: String,
    volumes: Vec<BookVolume>,
    chapters: Vec<ChapterVector>,
    expected: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChapterVector {
    number: u32,
    title: String,
    volume_id: Option<String>,
    paragraphs: Vec<String>,
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

#[test]
fn docx_pure_functions_match_ts_golden_vectors() {
    let file: VectorFile = serde_json::from_str(VECTORS).expect("向量文件可解析");
    assert!(!file.escape.is_empty(), "escape golden 非空防空转");
    assert!(
        !file.body_paragraphs.is_empty(),
        "bodyParagraphs golden 非空"
    );
    assert!(!file.paragraph.is_empty(), "paragraph golden 非空");
    assert!(!file.document.is_empty(), "document golden 非空");

    for vector in &file.escape {
        assert_eq!(
            docx_escape_xml(&vector.input),
            vector.expected,
            "{} escape 漂移",
            vector.name
        );
    }
    for vector in &file.body_paragraphs {
        assert_eq!(
            docx_body_paragraphs(&vector.input),
            vector.expected,
            "{} bodyParagraphs 漂移",
            vector.name
        );
    }
    for vector in &file.paragraph {
        assert_eq!(
            docx_paragraph_xml(&vector.style_id, &vector.text),
            vector.expected,
            "{} paragraph 漂移",
            vector.name
        );
    }
    for vector in &file.document {
        let chapters: Vec<DocxChapterInput> = vector
            .chapters
            .iter()
            .map(|chapter| DocxChapterInput {
                number: chapter.number,
                title: chapter.title.clone(),
                volume_id: chapter.volume_id.clone(),
                paragraphs: chapter.paragraphs.clone(),
            })
            .collect();
        assert_eq!(
            docx_document_xml(&vector.book_title, &vector.volumes, &chapters),
            vector.expected,
            "{} document 漂移",
            vector.name
        );
    }

    // 三常量 part sha256（TS 字面镜像漂移守卫——duel 条目级逐位可比前提）。
    assert_eq!(
        sha256_hex(DOCX_CONTENT_TYPES_XML.as_bytes()),
        file.static_parts["[Content_Types].xml"],
        "[Content_Types].xml 字面漂移"
    );
    assert_eq!(
        sha256_hex(DOCX_RELS_XML.as_bytes()),
        file.static_parts["_rels/.rels"],
        "_rels/.rels 字面漂移"
    );
    assert_eq!(
        sha256_hex(DOCX_STYLES_XML.as_bytes()),
        file.static_parts["word/styles.xml"],
        "word/styles.xml 字面漂移"
    );
}
