//! M2.3c 的本地 PDF 内容解析。

use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    path::Path,
};

use crate::{Document, DocumentId};

use super::{
    build_document, ensure_extension, prepare_source, read_file_contents, validate_size_limit,
    ParseError,
};

/// 从一个本地 PDF 文件提取文本并生成统一 `Document`。
///
/// `max_input_bytes` 限制原始 PDF 文件，`max_decompressed_bytes` 限制 PDF 解析和页面
/// 文本提取过程中单个解压流的大小，`max_output_bytes` 限制最终正文。页面按 PDF 的
/// 逻辑页码顺序处理，非空页面之间使用两个换行连接；当前 `Document` 仍使用
/// `whole_document` 位置，不把页码扩展进领域模型。
///
/// 解析完全在内存中进行，不渲染页面、不写临时文件、不访问外部资源，也不处理密码
/// 或 OCR。第三方解析器的 panic 会在本地边界内转换为安全错误，避免损坏输入拖垮调用方。
pub fn parse_pdf_file<P: AsRef<Path>>(
    id: DocumentId,
    path: P,
    max_input_bytes: u64,
    max_decompressed_bytes: u64,
    max_output_bytes: u64,
) -> Result<Document, ParseError> {
    validate_size_limit(max_input_bytes)?;
    validate_size_limit(max_decompressed_bytes)?;
    validate_size_limit(max_output_bytes)?;

    let (source, title) = prepare_source(path)?;
    ensure_extension(source.path(), is_pdf_extension)?;

    let contents = read_file_contents(source.path(), max_input_bytes)?;
    let body = extract_pdf_text(&contents, max_decompressed_bytes, max_output_bytes)?;

    build_document(id, source, title, body)
}

fn extract_pdf_text(
    contents: &[u8],
    max_decompressed_bytes: u64,
    max_output_bytes: u64,
) -> Result<String, ParseError> {
    let max_decompressed_bytes_usize =
        usize::try_from(max_decompressed_bytes).map_err(|_| ParseError::InvalidSizeLimit)?;

    let result = catch_unwind(AssertUnwindSafe(|| -> Result<String, ParseError> {
        let options = lopdf::LoadOptions::with_max_decompressed_size(max_decompressed_bytes_usize);
        let document = lopdf::Document::load_mem_with_options(contents, options)
            .map_err(|source| classify_pdf_error(source, max_decompressed_bytes))?;
        let page_numbers = document.get_pages().keys().copied().collect::<Vec<_>>();
        let mut body = String::new();

        for page_number in page_numbers {
            let page_text = document
                .extract_text_with_limit(&[page_number], max_decompressed_bytes_usize)
                .map_err(|source| classify_pdf_error(source, max_decompressed_bytes))?;
            append_page_text(&mut body, &page_text, max_output_bytes)?;
        }

        Ok(body)
    }));

    match result {
        Ok(result) => result,
        Err(_) => Err(ParseError::PdfParserFailed),
    }
}

fn classify_pdf_error(source: lopdf::Error, limit: u64) -> ParseError {
    match source {
        source @ lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded {
            ..
        }) => ParseError::PdfContentTooLarge { limit, source },
        source => ParseError::InvalidPdf { source },
    }
}

fn append_page_text(
    body: &mut String,
    page_text: &str,
    max_output_bytes: u64,
) -> Result<(), ParseError> {
    let page_text = page_text.trim();
    if page_text.is_empty() {
        return Ok(());
    }

    let separator_len = if body.is_empty() { 0 } else { 2 };
    let next_len = body
        .len()
        .checked_add(separator_len)
        .and_then(|length| length.checked_add(page_text.len()))
        .and_then(|length| u64::try_from(length).ok());

    if next_len.is_none_or(|length| length > max_output_bytes) {
        return Err(ParseError::ExtractedTextTooLarge {
            limit: max_output_bytes,
        });
    }

    if !body.is_empty() {
        body.push_str("\n\n");
    }
    body.push_str(page_text);
    Ok(())
}

fn is_pdf_extension(extension: &str) -> bool {
    extension.eq_ignore_ascii_case("pdf")
}
