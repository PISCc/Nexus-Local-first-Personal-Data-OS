//! M2.3a 的本地 HTML 内容解析。

use std::path::Path;

use dom_query::Document as HtmlDocument;

use crate::{Document, DocumentId};

use super::{
    build_document, decode_text, ensure_extension, prepare_source, read_file_contents,
    validate_size_limit, ParseError,
};

/// 从一个本地 HTML 文件提取可见文本并生成统一 `Document`。
///
/// 解析器接受 `.html` 和 `.htm` 扩展名，按严格 UTF-8 解码并仅移除开头的 UTF-8
/// BOM。HTML 按 HTML5 规则容错解析；`script`、`style`、`noscript` 和 `template`
/// 元素及其内容不会进入正文。输出文本会做确定性的空白和块边界规范化，并受
/// `max_output_bytes` 限制。解析器只读取一个文件，不写数据库、不调用 UI，也不
/// 保存 DOM 位置。
pub fn parse_html_file<P: AsRef<Path>>(
    id: DocumentId,
    path: P,
    max_input_bytes: u64,
    max_output_bytes: u64,
) -> Result<Document, ParseError> {
    validate_size_limit(max_input_bytes)?;
    validate_size_limit(max_output_bytes)?;

    let (source, title) = prepare_source(path)?;
    ensure_extension(source.path(), is_html_extension)?;

    let contents = read_file_contents(source.path(), max_input_bytes)?;
    let html = decode_text(&contents)?;
    let body = extract_visible_text(&html);

    if body.len() as u64 > max_output_bytes {
        return Err(ParseError::ExtractedTextTooLarge {
            limit: max_output_bytes,
        });
    }

    build_document(id, source, title, body)
}

fn extract_visible_text(html: &str) -> String {
    let document = HtmlDocument::from(html);
    document
        .select("script, style, noscript, template")
        .remove();

    document
        .body()
        .map(|body| body.formatted_text().to_string())
        .unwrap_or_else(|| document.formatted_text().to_string())
}

fn is_html_extension(extension: &str) -> bool {
    extension.eq_ignore_ascii_case("html") || extension.eq_ignore_ascii_case("htm")
}
