//! M2 的本地内容解析。

mod docx;
mod html;
mod pdf;

pub use docx::parse_docx_file;
pub use html::parse_html_file;
pub use pdf::parse_pdf_file;

use std::{
    error::Error,
    ffi::OsStr,
    fmt,
    fs::{self, File},
    io::{self, Read},
    path::Path,
    str::Utf8Error,
};

use crate::{
    document::{DocumentError, DocumentLocation},
    Document, DocumentId, DocumentSource,
};

const SUPPORTED_EXTENSIONS: &[&str] = &["txt", "md", "py", "rs", "js", "ts", "java", "cpp"];

/// 目录索引使用的默认解析边界。
///
/// 单文件解析入口仍要求调用方显式传入每一项限制；批量索引通过这个结构把
/// 默认资源边界集中在核心层，避免桌面层复制解析安全策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    pub max_input_bytes: u64,
    pub max_output_bytes: u64,
    pub max_pdf_decompressed_bytes: u64,
    pub max_docx_entry_bytes: u64,
    pub max_json_depth: usize,
}

impl Default for ParseOptions {
    fn default() -> Self {
        const SIXTEEN_MIB: u64 = 16 * 1024 * 1024;

        Self {
            max_input_bytes: SIXTEEN_MIB,
            max_output_bytes: SIXTEEN_MIB,
            max_pdf_decompressed_bytes: 64 * 1024 * 1024,
            max_docx_entry_bytes: SIXTEEN_MIB,
            max_json_depth: 32,
        }
    }
}

/// 按文件扩展名选择已验收的解析器。
///
/// 该入口只负责单个文件的格式分派和资源边界传递，不遍历目录、不写数据库，
/// 便于扫描编排层把一个文件的失败降级为统计项并继续处理后续文件。
pub fn parse_file<P: AsRef<Path>>(
    id: DocumentId,
    path: P,
    options: ParseOptions,
) -> Result<Document, ParseError> {
    let path = path.as_ref();
    let extension = path
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase);

    match extension.as_deref() {
        Some(extension) if is_supported_extension(extension) => {
            parse_local_file(id, path, options.max_input_bytes)
        }
        Some("json") => parse_json_file(id, path, options.max_input_bytes, options.max_json_depth),
        Some("html") | Some("htm") => {
            parse_html_file(id, path, options.max_input_bytes, options.max_output_bytes)
        }
        Some("docx") => parse_docx_file(
            id,
            path,
            options.max_input_bytes,
            options.max_docx_entry_bytes,
            options.max_output_bytes,
        ),
        Some("pdf") => parse_pdf_file(
            id,
            path,
            options.max_input_bytes,
            options.max_pdf_decompressed_bytes,
            options.max_output_bytes,
        ),
        _ => Err(ParseError::UnsupportedExtension),
    }
}

/// 从一个本地文件生成统一文本 Document。
///
/// `max_bytes` 是调用方明确提供的正文读取上限。解析器只处理支持的纯文本、
/// Markdown 和代码扩展名；正文按 UTF-8 解码，仅移除开头的 UTF-8 BOM，其余
/// 内容保持不变。解析器不会写数据库、调用 UI 或遍历目录。
pub fn parse_local_file<P: AsRef<Path>>(
    id: DocumentId,
    path: P,
    max_bytes: u64,
) -> Result<Document, ParseError> {
    validate_size_limit(max_bytes)?;
    let (source, title) = prepare_source(path)?;
    let path = source.path();

    ensure_extension(path, is_supported_extension)?;
    let contents = read_file_contents(path, max_bytes)?;
    let body = decode_text(&contents)?;

    build_document(id, source, title, body)
}

/// 从一个本地 JSON 文件生成统一文本 Document。
///
/// JSON 在本单元只负责合法性校验和嵌套深度限制，输出保留原始 UTF-8 文本，
/// 不进行格式化或字段重排。标量根节点深度为 0；对象和数组每增加一层深度
/// 加 1。
pub fn parse_json_file<P: AsRef<Path>>(
    id: DocumentId,
    path: P,
    max_bytes: u64,
    max_depth: usize,
) -> Result<Document, ParseError> {
    validate_size_limit(max_bytes)?;
    let (source, title) = prepare_source(path)?;
    let path = source.path();

    ensure_extension(path, is_json_extension)?;
    let contents = read_file_contents(path, max_bytes)?;
    let body = decode_text(&contents)?;
    let value = serde_json::from_str::<serde_json::Value>(&body)
        .map_err(|source| ParseError::InvalidJson { source })?;

    if json_depth(&value) > max_depth {
        return Err(ParseError::JsonTooDeep { limit: max_depth });
    }

    build_document(id, source, title, body)
}

fn validate_size_limit(max_bytes: u64) -> Result<(), ParseError> {
    if max_bytes == 0 {
        return Err(ParseError::InvalidSizeLimit);
    }

    Ok(())
}

fn prepare_source<P: AsRef<Path>>(path: P) -> Result<(DocumentSource, String), ParseError> {
    let source = DocumentSource::local_file(path).map_err(ParseError::Document)?;
    let title = source
        .path()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or(ParseError::MissingFileName)?;

    Ok((source, title))
}

fn ensure_extension(path: &Path, predicate: fn(&str) -> bool) -> Result<(), ParseError> {
    let extension = path.extension().and_then(OsStr::to_str);
    if extension.is_some_and(predicate) {
        return Ok(());
    }

    Err(ParseError::UnsupportedExtension)
}

fn read_file_contents(path: &Path, max_bytes: u64) -> Result<Vec<u8>, ParseError> {
    let metadata = fs::metadata(path).map_err(|source| ParseError::Metadata { source })?;
    if !metadata.is_file() {
        return Err(ParseError::NotRegularFile);
    }

    let mut file = File::open(path).map_err(|source| ParseError::Open { source })?;
    let opened_metadata = file
        .metadata()
        .map_err(|source| ParseError::Metadata { source })?;
    if !opened_metadata.is_file() {
        return Err(ParseError::NotRegularFile);
    }

    if opened_metadata.len() > max_bytes {
        return Err(ParseError::TooLarge { limit: max_bytes });
    }

    let mut contents = Vec::new();
    {
        let mut limited_file = (&mut file).take(max_bytes);
        limited_file
            .read_to_end(&mut contents)
            .map_err(|source| ParseError::Read { source })?;
    }

    let mut extra_byte = [0_u8; 1];
    if file
        .read(&mut extra_byte)
        .map_err(|source| ParseError::Read { source })?
        != 0
    {
        return Err(ParseError::TooLarge { limit: max_bytes });
    }

    Ok(contents)
}

fn decode_text(contents: &[u8]) -> Result<String, ParseError> {
    let text =
        std::str::from_utf8(contents).map_err(|source| ParseError::InvalidUtf8 { source })?;
    Ok(text.strip_prefix('\u{feff}').unwrap_or(text).to_owned())
}

fn build_document(
    id: DocumentId,
    source: DocumentSource,
    title: String,
    body: String,
) -> Result<Document, ParseError> {
    Document::new(id, source, title, body, DocumentLocation::whole_document())
        .map_err(ParseError::Document)
}

fn is_supported_extension(extension: &str) -> bool {
    SUPPORTED_EXTENSIONS
        .iter()
        .any(|supported| extension.eq_ignore_ascii_case(supported))
}

fn is_json_extension(extension: &str) -> bool {
    extension.eq_ignore_ascii_case("json")
}

fn json_depth(value: &serde_json::Value) -> usize {
    let mut maximum_depth = 0;
    let mut pending = vec![(value, 0_usize)];

    while let Some((value, depth)) = pending.pop() {
        match value {
            serde_json::Value::Array(values) => {
                let child_depth = depth.saturating_add(1);
                maximum_depth = maximum_depth.max(child_depth);
                pending.extend(values.iter().map(|value| (value, child_depth)));
            }
            serde_json::Value::Object(values) => {
                let child_depth = depth.saturating_add(1);
                maximum_depth = maximum_depth.max(child_depth);
                pending.extend(values.values().map(|value| (value, child_depth)));
            }
            serde_json::Value::Null
            | serde_json::Value::Bool(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::String(_) => {}
        }
    }

    maximum_depth
}

/// 本地文件解析错误。
///
/// 错误展示不包含输入路径、文件名、正文或原始系统错误；原始错误仅通过
/// `Error::source` 保留给进程内诊断。
#[derive(Debug)]
pub enum ParseError {
    InvalidSizeLimit,
    UnsupportedExtension,
    Metadata { source: io::Error },
    NotRegularFile,
    Open { source: io::Error },
    TooLarge { limit: u64 },
    Read { source: io::Error },
    InvalidUtf8 { source: Utf8Error },
    InvalidJson { source: serde_json::Error },
    JsonTooDeep { limit: usize },
    ExtractedTextTooLarge { limit: u64 },
    InvalidDocxArchive { source: zip::result::ZipError },
    MissingDocxDocumentXml,
    InvalidDocxXml { source: quick_xml::Error },
    DocxEntryTooLarge { limit: u64 },
    InvalidPdf { source: lopdf::Error },
    PdfContentTooLarge { limit: u64, source: lopdf::Error },
    PdfParserFailed,
    MissingFileName,
    Document(DocumentError),
}

impl ParseError {
    /// 返回不包含路径、文件名或正文的安全错误分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidSizeLimit => "parse_size_limit_invalid",
            Self::UnsupportedExtension => "parse_format_unsupported",
            Self::Metadata { .. } => "parse_file_metadata",
            Self::NotRegularFile => "parse_file_not_regular",
            Self::Open { .. } => "parse_file_open",
            Self::TooLarge { .. } => "parse_file_too_large",
            Self::Read { .. } => "parse_file_read",
            Self::InvalidUtf8 { .. } => "parse_utf8_invalid",
            Self::InvalidJson { .. } => "parse_json_invalid",
            Self::JsonTooDeep { .. } => "parse_json_depth_exceeded",
            Self::ExtractedTextTooLarge { .. } => "parse_output_too_large",
            Self::InvalidDocxArchive { .. } => "parse_docx_archive_invalid",
            Self::MissingDocxDocumentXml => "parse_docx_document_missing",
            Self::InvalidDocxXml { .. } => "parse_docx_xml_invalid",
            Self::DocxEntryTooLarge { .. } => "parse_docx_entry_too_large",
            Self::InvalidPdf { .. } => "parse_pdf_invalid",
            Self::PdfContentTooLarge { .. } => "parse_pdf_content_too_large",
            Self::PdfParserFailed => "parse_pdf_parser_failed",
            Self::MissingFileName => "parse_file_name_invalid",
            Self::Document(source) => source.kind(),
        }
    }

    /// 返回可以直接展示给用户的非敏感中文说明。
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::InvalidSizeLimit => "解析大小限制无效。",
            Self::UnsupportedExtension => "不支持的文件格式。",
            Self::Metadata { .. } => "无法读取文件元数据。",
            Self::NotRegularFile => "只支持普通文件。",
            Self::Open { .. } => "无法打开文件。",
            Self::TooLarge { .. } => "文件超过解析大小限制。",
            Self::Read { .. } => "无法读取文件内容。",
            Self::InvalidUtf8 { .. } => "文件不是有效的 UTF-8 文本。",
            Self::InvalidJson { .. } => "JSON 格式无效。",
            Self::JsonTooDeep { .. } => "JSON 嵌套深度超过限制。",
            Self::ExtractedTextTooLarge { .. } => "提取后的文本超过大小限制。",
            Self::InvalidDocxArchive { .. } => "DOCX 压缩包格式无效。",
            Self::MissingDocxDocumentXml => "DOCX 主文档内容不存在。",
            Self::InvalidDocxXml { .. } => "DOCX 主文档 XML 无效。",
            Self::DocxEntryTooLarge { .. } => "DOCX 文档内容超过解析大小限制。",
            Self::InvalidPdf { .. } => "PDF 格式无效。",
            Self::PdfContentTooLarge { .. } => "PDF 解压后的内容超过解析大小限制。",
            Self::PdfParserFailed => "PDF 解析器无法处理该文档。",
            Self::MissingFileName => "文件路径不包含文件名。",
            Self::Document(source) => source.user_message(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "本地文件解析失败: {}", self.kind())
    }
}

impl Error for ParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Metadata { source } | Self::Open { source } | Self::Read { source } => {
                Some(source)
            }
            Self::InvalidUtf8 { source } => Some(source),
            Self::InvalidJson { source } => Some(source),
            Self::Document(source) => Some(source),
            Self::InvalidSizeLimit
            | Self::UnsupportedExtension
            | Self::NotRegularFile
            | Self::TooLarge { .. }
            | Self::JsonTooDeep { .. }
            | Self::ExtractedTextTooLarge { .. }
            | Self::MissingDocxDocumentXml
            | Self::DocxEntryTooLarge { .. }
            | Self::PdfParserFailed
            | Self::MissingFileName => None,
            Self::InvalidDocxArchive { source } => Some(source),
            Self::InvalidDocxXml { source } => Some(source),
            Self::InvalidPdf { source } | Self::PdfContentTooLarge { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        io::{Cursor, Write},
        path::{Path, PathBuf},
        process,
        sync::atomic::{AtomicUsize, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        parse_docx_file, parse_file, parse_html_file, parse_json_file, parse_local_file,
        parse_pdf_file, ParseError, ParseOptions,
    };
    use crate::{DocumentId, DocumentLocation};

    static TEMP_DIRECTORY_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TemporaryDirectory {
        path: PathBuf,
    }

    impl TemporaryDirectory {
        fn new() -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let counter = TEMP_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);

            for attempt in 0..100 {
                let path = env::temp_dir().join(format!(
                    "nexus-parser-test-{}-{timestamp}-{counter}-{attempt}",
                    process::id()
                ));

                match fs::create_dir(&path) {
                    Ok(()) => return Self { path },
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("创建解析器测试临时目录失败: {error}"),
                }
            }

            panic!("无法创建唯一解析器测试临时目录")
        }

        fn child_path(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_file(path: &Path, contents: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("创建解析器测试父目录失败");
        }
        fs::write(path, contents).expect("写入解析器测试文件失败");
    }

    fn write_docx_file(path: &Path, entries: &[(&str, &[u8])]) {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        for (name, contents) in entries {
            archive
                .start_file(*name, options)
                .expect("创建 DOCX 测试条目失败");
            archive.write_all(contents).expect("写入 DOCX 测试条目失败");
        }

        let archive = archive.finish().expect("完成 DOCX 测试压缩包失败");
        write_file(path, &archive.into_inner());
    }

    fn write_pdf_file(path: &Path, pages: &[&str]) {
        use lopdf::{
            content::{Content, Operation},
            dictionary, Document as PdfDocument, Object as PdfObject, Stream,
        };

        let mut document = PdfDocument::with_version("1.4");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let resources_id = document.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let mut page_ids = Vec::with_capacity(pages.len());

        for page_text in pages {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 12.into()]),
                    Operation::new("Td", vec![50.into(), 750.into()]),
                    Operation::new(
                        "Tj",
                        vec![PdfObject::string_literal((*page_text).to_owned())],
                    ),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id = document.add_object(Stream::new(
                dictionary! {},
                content.encode().expect("编码 PDF 测试页面失败"),
            ));
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            });
            page_ids.push(page_id);
        }

        let page_count = page_ids.len() as i64;
        let kids = page_ids
            .into_iter()
            .map(PdfObject::Reference)
            .collect::<Vec<_>>();
        document.objects.insert(
            pages_id,
            PdfObject::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => page_count,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        document.compress();
        document.save(path).expect("写入 PDF 测试文档失败");
    }

    fn document_id(value: &str) -> DocumentId {
        DocumentId::new(value).expect("创建解析器测试文档 ID 失败")
    }

    #[test]
    fn parses_supported_extensions_as_plain_utf8_documents() {
        let temporary_directory = TemporaryDirectory::new();
        let extensions = ["txt", "md", "py", "rs", "js", "ts", "java", "cpp"];

        for extension in extensions {
            let path = temporary_directory.child_path(&format!("sample.{extension}"));
            let body = format!("body for {extension}\n");
            write_file(&path, body.as_bytes());

            let document = parse_local_file(
                document_id(&format!("file:sample.{extension}")),
                &path,
                1024,
            )
            .expect("解析支持的纯文本文件失败");

            assert_eq!(document.source.path(), path);
            assert_eq!(document.title, format!("sample.{extension}"));
            assert_eq!(document.body, body);
            assert_eq!(document.location, DocumentLocation::whole_document());
        }
    }

    #[test]
    fn dispatches_all_supported_formats_with_default_index_limits() {
        let temporary_directory = TemporaryDirectory::new();
        let options = ParseOptions::default();

        let text_path = temporary_directory.child_path("notes.md");
        write_file(&text_path, b"plain text");
        assert_eq!(
            parse_file(document_id("file:notes"), &text_path, options)
                .expect("分派 Markdown 解析失败")
                .body,
            "plain text"
        );

        let json_path = temporary_directory.child_path("data.json");
        write_file(&json_path, br#"{"name":"Nexus"}"#);
        assert_eq!(
            parse_file(document_id("file:data"), &json_path, options)
                .expect("分派 JSON 解析失败")
                .body,
            r#"{"name":"Nexus"}"#
        );

        let html_path = temporary_directory.child_path("page.html");
        write_file(&html_path, b"<main>visible</main>");
        assert!(parse_file(document_id("file:page"), &html_path, options)
            .expect("分派 HTML 解析失败")
            .body
            .contains("visible"));

        let docx_path = temporary_directory.child_path("document.docx");
        write_docx_file(
            &docx_path,
            &[(
                "word/document.xml",
                br#"<w:document xmlns:w="urn:word"><w:body><w:p><w:r><w:t>docx text</w:t></w:r></w:p></w:body></w:document>"#,
            )],
        );
        assert!(
            parse_file(document_id("file:document"), &docx_path, options)
                .expect("分派 DOCX 解析失败")
                .body
                .contains("docx text")
        );

        let pdf_path = temporary_directory.child_path("document.pdf");
        write_pdf_file(&pdf_path, &["pdf text"]);
        assert!(parse_file(document_id("file:pdf"), &pdf_path, options)
            .expect("分派 PDF 解析失败")
            .body
            .contains("pdf text"));

        let unsupported_path = temporary_directory.child_path("archive.bin");
        let error = parse_file(document_id("file:archive"), &unsupported_path, options)
            .expect_err("不支持格式不应被分派到解析器");
        assert!(matches!(error, ParseError::UnsupportedExtension));
    }

    #[test]
    fn accepts_case_insensitive_extensions_and_strips_only_a_leading_bom() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("README.MD");
        write_file(&path, b"\xef\xbb\xbf# Nexus\r\n\r\nbody");

        let document =
            parse_local_file(document_id("file:readme"), &path, 1024).expect("解析带 BOM 文件失败");

        assert_eq!(document.body, "# Nexus\r\n\r\nbody");
    }

    #[test]
    fn preserves_an_empty_file_as_an_empty_document_body() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("empty.txt");
        write_file(&path, b"");

        let document =
            parse_local_file(document_id("file:empty"), &path, 1).expect("解析空文件失败");

        assert!(document.body.is_empty());
    }

    #[test]
    fn rejects_unsupported_format_before_reading_the_file() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("archive.bin");

        let error = parse_local_file(document_id("file:archive"), &path, 1024)
            .expect_err("不支持的扩展名不应解析成功");

        assert!(matches!(error, ParseError::UnsupportedExtension));
        assert_eq!(error.kind(), "parse_format_unsupported");
        assert_eq!(error.user_message(), "不支持的文件格式。");
    }

    #[test]
    fn reports_missing_files_as_safe_metadata_errors() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("missing.txt");
        let sensitive_path = path.display().to_string();

        let error = parse_local_file(document_id("file:missing"), &path, 1024)
            .expect_err("不存在的文件不应解析成功");

        assert!(matches!(error, ParseError::Metadata { .. }));
        assert_eq!(error.kind(), "parse_file_metadata");
        assert_eq!(error.user_message(), "无法读取文件元数据。");
        assert!(!error.to_string().contains(&sensitive_path));
    }

    #[test]
    fn rejects_directories_and_zero_or_oversized_limits() {
        let temporary_directory = TemporaryDirectory::new();
        let directory = temporary_directory.child_path("folder.txt");
        fs::create_dir(&directory).expect("创建解析器测试目录失败");

        let directory_error = parse_local_file(document_id("file:folder"), &directory, 1024)
            .expect_err("目录不应解析为文件");
        assert!(matches!(directory_error, ParseError::NotRegularFile));
        assert_eq!(directory_error.kind(), "parse_file_not_regular");

        let path = temporary_directory.child_path("large.txt");
        write_file(&path, b"1234");

        let zero_limit =
            parse_local_file(document_id("file:zero"), &path, 0).expect_err("零大小限制不应被接受");
        assert!(matches!(zero_limit, ParseError::InvalidSizeLimit));

        let oversized = parse_local_file(document_id("file:large"), &path, 3)
            .expect_err("超过大小限制的文件不应解析成功");
        assert!(matches!(oversized, ParseError::TooLarge { limit: 3 }));
        assert_eq!(oversized.kind(), "parse_file_too_large");

        let exact = parse_local_file(document_id("file:large-exact"), &path, 4)
            .expect("恰好达到大小限制的文件应解析成功");
        assert_eq!(exact.body, "1234");
    }

    #[test]
    fn rejects_invalid_utf8_without_exposing_path_or_content() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("invalid.txt");
        let sensitive_path = path.display().to_string();
        write_file(&path, &[0xff, 0xfe, b'x']);

        let error = parse_local_file(document_id("file:invalid"), &path, 1024)
            .expect_err("无效 UTF-8 不应解析成功");

        assert!(matches!(error, ParseError::InvalidUtf8 { .. }));
        assert_eq!(error.kind(), "parse_utf8_invalid");
        assert_eq!(error.user_message(), "文件不是有效的 UTF-8 文本。");
        assert!(!error.to_string().contains(&sensitive_path));
        assert!(!error.to_string().contains("ff"));
    }

    #[test]
    fn parses_json_and_preserves_original_text() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("document.JSON");
        let body = " {\n  \"name\": \"Nexus\"\n}\n";
        write_file(&path, body.as_bytes());

        let document = parse_json_file(document_id("file:document"), &path, 1024, 4)
            .expect("解析合法 JSON 文件失败");

        assert_eq!(document.source.path(), path);
        assert_eq!(document.title, "document.JSON");
        assert_eq!(document.body, body);
        assert_eq!(document.location, DocumentLocation::whole_document());
    }

    #[test]
    fn accepts_scalar_json_at_zero_depth_and_rejects_deeper_json() {
        let temporary_directory = TemporaryDirectory::new();
        let scalar_path = temporary_directory.child_path("scalar.json");
        write_file(&scalar_path, b"null\n");

        parse_json_file(document_id("file:scalar"), &scalar_path, 1024, 0)
            .expect("零深度限制应允许标量 JSON");

        let nested_path = temporary_directory.child_path("nested.json");
        write_file(&nested_path, br#"{"outer":{"inner":true}}"#);
        let error = parse_json_file(document_id("file:nested"), &nested_path, 1024, 1)
            .expect_err("超过嵌套深度的 JSON 不应解析成功");

        assert!(matches!(error, ParseError::JsonTooDeep { limit: 1 }));
        assert_eq!(error.kind(), "parse_json_depth_exceeded");
        assert_eq!(error.user_message(), "JSON 嵌套深度超过限制。");
    }

    #[test]
    fn rejects_malformed_json_without_preventing_a_following_file() {
        let temporary_directory = TemporaryDirectory::new();
        let invalid_path = temporary_directory.child_path("invalid.json");
        let sensitive_path = invalid_path.display().to_string();
        write_file(&invalid_path, br#"{"name":}"#);

        let error = parse_json_file(document_id("file:invalid"), &invalid_path, 1024, 4)
            .expect_err("malformed JSON 不应解析成功");

        assert!(matches!(error, ParseError::InvalidJson { .. }));
        assert_eq!(error.kind(), "parse_json_invalid");
        assert_eq!(error.user_message(), "JSON 格式无效。");
        assert!(!error.to_string().contains(&sensitive_path));
        assert!(!error.to_string().contains("name"));

        let valid_path = temporary_directory.child_path("valid.json");
        write_file(&valid_path, br#"{"ok":true}"#);
        parse_json_file(document_id("file:valid"), &valid_path, 1024, 4)
            .expect("前一个 JSON 失败不应阻止后续文件解析");
    }

    #[test]
    fn rejects_json_with_an_unsupported_extension_before_reading() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("document.txt");

        let error = parse_json_file(document_id("file:document"), &path, 1024, 4)
            .expect_err("非 JSON 扩展名不应由 JSON 解析器处理");

        assert!(matches!(error, ParseError::UnsupportedExtension));
        assert_eq!(error.kind(), "parse_format_unsupported");
    }

    #[test]
    fn parses_html_visible_text_and_excludes_non_content_elements() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("page.HTML");
        write_file(
            &path,
            br#"<!doctype html>
<html>
  <head>
    <title>Page title should not be body text</title>
    <style>.hidden { display: none; }</style>
  </head>
  <body>
    <h1>Hello&nbsp;Nexus</h1>
    <p>Local <strong>first</strong> paragraph<br>next.</p>
    <ul><li>One</li><li>Two</li></ul>
    <script>secret script text</script>
    <noscript>secret noscript text</noscript>
    <template>secret template text</template>
  </body>
</html>"#,
        );

        let document = parse_html_file(document_id("file:page"), &path, 4096, 1024)
            .expect("解析 HTML 可见文本失败");

        assert_eq!(document.source.path(), path);
        assert_eq!(document.title, "page.HTML");
        assert_eq!(document.location, DocumentLocation::whole_document());
        assert!(document.body.contains("Hello Nexus"));
        assert!(document.body.contains("Local first paragraph"));
        assert!(document.body.contains("next."));
        assert!(document.body.contains("One"));
        assert!(document.body.contains("Two"));
        assert!(!document.body.contains("Page title should not be body text"));
        assert!(!document.body.contains("hidden"));
        assert!(!document.body.contains("secret"));
    }

    #[test]
    fn accepts_malformed_html_and_strips_a_leading_bom() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("fragment.htm");
        write_file(&path, b"\xef\xbb\xbf<p>one <b>two");

        let document = parse_html_file(document_id("file:fragment"), &path, 1024, 1024)
            .expect("容错解析 malformed HTML 失败");

        assert_eq!(document.body, "one two");
    }

    #[test]
    fn rejects_html_when_extracted_text_exceeds_output_limit() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("large-output.html");
        let sensitive_text = "sensitive page text";
        write_file(&path, format!("<p>{sensitive_text}</p>").as_bytes());

        let error = parse_html_file(document_id("file:large-output"), &path, 1024, 4)
            .expect_err("超过输出限制的 HTML 不应解析成功");

        assert!(matches!(
            error,
            ParseError::ExtractedTextTooLarge { limit: 4 }
        ));
        assert_eq!(error.kind(), "parse_output_too_large");
        assert_eq!(error.user_message(), "提取后的文本超过大小限制。");
        assert!(!error.to_string().contains(sensitive_text));
    }

    #[test]
    fn rejects_html_with_invalid_utf8_and_invalid_limits() {
        let temporary_directory = TemporaryDirectory::new();
        let invalid_path = temporary_directory.child_path("invalid.html");
        write_file(&invalid_path, &[0xff, 0xfe]);

        let invalid_utf8 = parse_html_file(document_id("file:invalid"), &invalid_path, 1024, 1024)
            .expect_err("无效 UTF-8 HTML 不应解析成功");
        assert!(matches!(invalid_utf8, ParseError::InvalidUtf8 { .. }));

        let valid_path = temporary_directory.child_path("valid.html");
        write_file(&valid_path, b"<p>valid</p>");
        let zero_input = parse_html_file(document_id("file:zero-input"), &valid_path, 0, 1024)
            .expect_err("零输入限制不应被接受");
        assert!(matches!(zero_input, ParseError::InvalidSizeLimit));

        let zero_output = parse_html_file(document_id("file:zero-output"), &valid_path, 1024, 0)
            .expect_err("零输出限制不应被接受");
        assert!(matches!(zero_output, ParseError::InvalidSizeLimit));
    }

    #[test]
    fn parses_docx_main_document_text_and_structural_markers() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("document.DOCX");
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>Hello &amp; Nexus</w:t></w:r></w:p>
    <w:p><w:r><w:t>Second</w:t><w:tab/><w:t>cell</w:t><w:br/><w:t>line</w:t></w:r></w:p>
  </w:body>
</w:document>"#;
        write_docx_file(&path, &[("word/document.xml", xml)]);

        let document = parse_docx_file(document_id("file:document"), &path, 4096, 4096, 1024)
            .expect("解析 DOCX 主文档文本失败");

        assert_eq!(document.source.path(), path);
        assert_eq!(document.title, "document.DOCX");
        assert_eq!(document.body, "Hello & Nexus\nSecond\tcell\nline");
        assert_eq!(document.location, DocumentLocation::whole_document());
    }

    #[test]
    fn rejects_invalid_or_incomplete_docx_archives_safely() {
        let temporary_directory = TemporaryDirectory::new();
        let invalid_path = temporary_directory.child_path("invalid.docx");
        let sensitive_path = invalid_path.display().to_string();
        write_file(&invalid_path, b"not a zip archive");

        let invalid_archive =
            parse_docx_file(document_id("file:invalid"), &invalid_path, 1024, 1024, 1024)
                .expect_err("无效 DOCX 压缩包不应解析成功");
        assert!(matches!(
            invalid_archive,
            ParseError::InvalidDocxArchive { .. }
        ));
        assert_eq!(invalid_archive.kind(), "parse_docx_archive_invalid");
        assert_eq!(invalid_archive.user_message(), "DOCX 压缩包格式无效。");
        assert!(!invalid_archive.to_string().contains(&sensitive_path));

        let missing_path = temporary_directory.child_path("missing-document.docx");
        write_docx_file(&missing_path, &[("word/styles.xml", b"<w:styles/>")]);

        let missing = parse_docx_file(
            document_id("file:missing-document"),
            &missing_path,
            1024,
            1024,
            1024,
        )
        .expect_err("缺少 DOCX 主文档条目不应解析成功");
        assert!(matches!(missing, ParseError::MissingDocxDocumentXml));
        assert_eq!(missing.kind(), "parse_docx_document_missing");
        assert_eq!(missing.user_message(), "DOCX 主文档内容不存在。");
    }

    #[test]
    fn enforces_docx_entry_and_output_limits_before_returning_a_document() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("limited.docx");
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>long text</w:t></w:r></w:p></w:body></w:document>"#;
        write_docx_file(&path, &[("word/document.xml", xml)]);

        let entry_limit = parse_docx_file(document_id("file:entry-limit"), &path, 4096, 4, 1024)
            .expect_err("超过 DOCX 条目上限不应解析成功");
        assert!(matches!(
            entry_limit,
            ParseError::DocxEntryTooLarge { limit: 4 }
        ));
        assert_eq!(entry_limit.kind(), "parse_docx_entry_too_large");

        let output_limit = parse_docx_file(document_id("file:output-limit"), &path, 4096, 4096, 4)
            .expect_err("超过 DOCX 输出上限不应解析成功");
        assert!(matches!(
            output_limit,
            ParseError::ExtractedTextTooLarge { limit: 4 }
        ));
        assert_eq!(output_limit.kind(), "parse_output_too_large");
    }

    #[test]
    fn rejects_malformed_docx_xml_without_exposing_xml_content() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("malformed.docx");
        let sensitive_text = "secret malformed text";
        let xml = format!(
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:t>{sensitive_text}</w:p></w:body>"
        );
        write_docx_file(&path, &[("word/document.xml", xml.as_bytes())]);

        let error = parse_docx_file(document_id("file:malformed"), &path, 4096, 4096, 1024)
            .expect_err("malformed DOCX XML 不应解析成功");

        assert!(matches!(error, ParseError::InvalidDocxXml { .. }));
        assert_eq!(error.kind(), "parse_docx_xml_invalid");
        assert_eq!(error.user_message(), "DOCX 主文档 XML 无效。");
        assert!(!error.to_string().contains(sensitive_text));
    }

    #[test]
    fn parses_pdf_pages_as_one_document_in_page_order() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("document.PDF");
        write_pdf_file(&path, &["First PDF page", "", "Second PDF page"]);

        let document = parse_pdf_file(
            document_id("file:document-pdf"),
            &path,
            16 * 1024,
            16 * 1024,
            1024,
        )
        .expect("解析 PDF 页面文本失败");

        assert_eq!(document.source.path(), path);
        assert_eq!(document.title, "document.PDF");
        assert_eq!(document.body, "First PDF page\n\nSecond PDF page");
        assert_eq!(document.location, DocumentLocation::whole_document());
    }

    #[test]
    fn rejects_invalid_pdf_without_exposing_path_or_content() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("invalid.pdf");
        let sensitive_path = path.display().to_string();
        let sensitive_content = "secret PDF content";
        write_file(&path, format!("not a PDF: {sensitive_content}").as_bytes());

        let error = parse_pdf_file(document_id("file:invalid-pdf"), &path, 1024, 1024, 1024)
            .expect_err("无效 PDF 不应解析成功");

        assert!(matches!(error, ParseError::InvalidPdf { .. }));
        assert_eq!(error.kind(), "parse_pdf_invalid");
        assert_eq!(error.user_message(), "PDF 格式无效。");
        assert!(!error.to_string().contains(&sensitive_path));
        assert!(!error.to_string().contains(sensitive_content));
    }

    #[test]
    fn enforces_pdf_input_stream_and_output_limits() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("limited.pdf");
        write_pdf_file(&path, &["First page", "Second page"]);

        let output_limit = parse_pdf_file(
            document_id("file:pdf-output-limit"),
            &path,
            16 * 1024,
            16 * 1024,
            12,
        )
        .expect_err("超过 PDF 输出上限不应解析成功");
        assert!(matches!(
            output_limit,
            ParseError::ExtractedTextTooLarge { limit: 12 }
        ));
        assert_eq!(output_limit.kind(), "parse_output_too_large");

        let stream_limit = parse_pdf_file(
            document_id("file:pdf-stream-limit"),
            &path,
            16 * 1024,
            4,
            1024,
        )
        .expect_err("超过 PDF 解压流上限不应解析成功");
        assert!(matches!(
            stream_limit,
            ParseError::PdfContentTooLarge { limit: 4, .. }
        ));
        assert_eq!(stream_limit.kind(), "parse_pdf_content_too_large");
        assert_eq!(
            stream_limit.user_message(),
            "PDF 解压后的内容超过解析大小限制。"
        );

        let oversized_path = temporary_directory.child_path("oversized.pdf");
        write_pdf_file(&oversized_path, &["oversized PDF input"]);
        let input_limit = parse_pdf_file(
            document_id("file:pdf-input-limit"),
            &oversized_path,
            1,
            1024,
            1024,
        )
        .expect_err("超过 PDF 输入上限不应解析成功");
        assert!(matches!(input_limit, ParseError::TooLarge { limit: 1 }));
    }

    #[test]
    fn rejects_pdf_with_invalid_limits_or_extension() {
        let temporary_directory = TemporaryDirectory::new();
        let path = temporary_directory.child_path("document.pdf");
        write_pdf_file(&path, &["valid PDF"]);

        let zero_input = parse_pdf_file(document_id("file:pdf-zero-input"), &path, 0, 1024, 1024)
            .expect_err("零 PDF 输入限制不应被接受");
        assert!(matches!(zero_input, ParseError::InvalidSizeLimit));

        let zero_stream = parse_pdf_file(document_id("file:pdf-zero-stream"), &path, 1024, 0, 1024)
            .expect_err("零 PDF 解压流限制不应被接受");
        assert!(matches!(zero_stream, ParseError::InvalidSizeLimit));

        let zero_output = parse_pdf_file(document_id("file:pdf-zero-output"), &path, 1024, 1024, 0)
            .expect_err("零 PDF 输出限制不应被接受");
        assert!(matches!(zero_output, ParseError::InvalidSizeLimit));

        let unsupported_path = temporary_directory.child_path("document.txt");
        let unsupported = parse_pdf_file(
            document_id("file:pdf-unsupported-extension"),
            &unsupported_path,
            1024,
            1024,
            1024,
        )
        .expect_err("非 PDF 扩展名不应由 PDF 解析器处理");
        assert!(matches!(unsupported, ParseError::UnsupportedExtension));
    }
}
