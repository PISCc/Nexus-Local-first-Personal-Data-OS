//! M2.3b 的本地 DOCX 内容解析。

use std::{
    io::{Cursor, Read},
    path::Path,
};

use quick_xml::{events::Event, reader::Reader};
use zip::ZipArchive;

use crate::{Document, DocumentId};

use super::{
    build_document, decode_text, ensure_extension, prepare_source, read_file_contents,
    validate_size_limit, ParseError,
};

const DOCUMENT_XML_PATH: &str = "word/document.xml";

/// 从一个本地 DOCX 文件提取主文档文本并生成统一 `Document`。
///
/// DOCX 是 ZIP 容器。本单元只读取 `word/document.xml`，提取 WordprocessingML 的
/// 段落、文本、换行和制表符；不解压到文件系统，不执行宏或其他嵌入内容，也不读取
/// 页眉、页脚、批注或关系目标。`max_input_bytes` 限制 ZIP 文件，`max_entry_bytes`
/// 限制主文档 XML，`max_output_bytes` 限制提取后的正文。
pub fn parse_docx_file<P: AsRef<Path>>(
    id: DocumentId,
    path: P,
    max_input_bytes: u64,
    max_entry_bytes: u64,
    max_output_bytes: u64,
) -> Result<Document, ParseError> {
    validate_size_limit(max_input_bytes)?;
    validate_size_limit(max_entry_bytes)?;
    validate_size_limit(max_output_bytes)?;

    let (source, title) = prepare_source(path)?;
    ensure_extension(source.path(), is_docx_extension)?;

    let contents = read_file_contents(source.path(), max_input_bytes)?;
    let xml = read_document_xml(&contents, max_entry_bytes)?;
    let body = extract_document_text(&xml)?;

    if body.len() as u64 > max_output_bytes {
        return Err(ParseError::ExtractedTextTooLarge {
            limit: max_output_bytes,
        });
    }

    build_document(id, source, title, body)
}

fn read_document_xml(contents: &[u8], max_entry_bytes: u64) -> Result<Vec<u8>, ParseError> {
    let mut archive = ZipArchive::new(Cursor::new(contents))
        .map_err(|source| ParseError::InvalidDocxArchive { source })?;
    let mut entry = archive
        .by_name(DOCUMENT_XML_PATH)
        .map_err(|source| match source {
            zip::result::ZipError::FileNotFound => ParseError::MissingDocxDocumentXml,
            source => ParseError::InvalidDocxArchive { source },
        })?;

    if entry.is_dir() || entry.is_symlink() {
        return Err(ParseError::MissingDocxDocumentXml);
    }

    if entry.size() > max_entry_bytes {
        return Err(ParseError::DocxEntryTooLarge {
            limit: max_entry_bytes,
        });
    }

    let capacity = usize::try_from(entry.size()).map_err(|_| ParseError::DocxEntryTooLarge {
        limit: max_entry_bytes,
    })?;
    let mut xml = Vec::with_capacity(capacity);
    entry
        .read_to_end(&mut xml)
        .map_err(|source| ParseError::InvalidDocxArchive {
            source: zip::result::ZipError::Io(source),
        })?;

    if xml.len() as u64 > max_entry_bytes {
        return Err(ParseError::DocxEntryTooLarge {
            limit: max_entry_bytes,
        });
    }

    Ok(xml)
}

fn extract_document_text(xml: &[u8]) -> Result<String, ParseError> {
    let xml = decode_text(xml)?;
    let mut reader = Reader::from_reader(xml.as_bytes());
    let mut buffer = Vec::new();
    let mut body = String::new();
    let mut in_text = false;

    loop {
        match reader
            .read_event_into(&mut buffer)
            .map_err(|source| ParseError::InvalidDocxXml { source })?
        {
            Event::Start(start) => {
                let name = start.local_name();
                if name.as_ref() == b"t" {
                    in_text = true;
                } else if name.as_ref() == b"p" {
                    append_paragraph_separator(&mut body);
                } else {
                    append_empty_element(&mut body, name.as_ref());
                }
            }
            Event::Empty(empty) => append_empty_element(&mut body, empty.local_name().as_ref()),
            Event::Text(text) if in_text => {
                let text = std::str::from_utf8(text.as_ref())
                    .map_err(|source| ParseError::InvalidUtf8 { source })?;
                let text = quick_xml::escape::unescape(text)
                    .map_err(quick_xml::Error::from)
                    .map_err(|source| ParseError::InvalidDocxXml { source })?;
                body.push_str(&text);
            }
            Event::GeneralRef(reference) if in_text => {
                append_general_reference(&mut body, reference.as_ref())?;
            }
            Event::End(end) => {
                if end.local_name().as_ref() == b"t" {
                    in_text = false;
                }
                if end.local_name().as_ref() == b"p" {
                    trim_trailing_spaces(&mut body);
                    body.push('\n');
                }
            }
            Event::CData(data) if in_text => {
                let text = std::str::from_utf8(data.as_ref())
                    .map_err(|source| ParseError::InvalidUtf8 { source })?;
                body.push_str(text);
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }

    trim_document_text(&mut body);
    Ok(body)
}

fn append_general_reference(body: &mut String, reference: &[u8]) -> Result<(), ParseError> {
    let reference =
        std::str::from_utf8(reference).map_err(|source| ParseError::InvalidUtf8 { source })?;
    let mut escaped = String::with_capacity(reference.len() + 2);
    escaped.push('&');
    escaped.push_str(reference);
    escaped.push(';');

    let text = quick_xml::escape::unescape(&escaped)
        .map_err(quick_xml::Error::from)
        .map_err(|source| ParseError::InvalidDocxXml { source })?;
    body.push_str(&text);
    Ok(())
}

fn append_empty_element(body: &mut String, local_name: &[u8]) {
    match local_name {
        b"br" | b"cr" => body.push('\n'),
        b"tab" => body.push('\t'),
        _ => {}
    }
}

fn append_paragraph_separator(body: &mut String) {
    if !body.is_empty() && !body.ends_with(['\n', '\t', ' ']) {
        body.push('\n');
    }
}

fn trim_trailing_spaces(body: &mut String) {
    while body.ends_with([' ', '\t']) {
        body.pop();
    }
}

fn trim_document_text(body: &mut String) {
    trim_trailing_spaces(body);
    while body.ends_with('\n') {
        body.pop();
    }
}

fn is_docx_extension(extension: &str) -> bool {
    extension.eq_ignore_ascii_case("docx")
}
