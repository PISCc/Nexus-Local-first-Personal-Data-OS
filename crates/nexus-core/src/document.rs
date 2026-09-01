//! M2.0 的最小统一文档模型。

use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

use nexus_db::{normalize_path, FileMetadataError};

/// 文档的稳定标识。
///
/// ID 是由来源适配器提供的 opaque 值。M2.0 只保证它非空并在模型中原样保留，
/// 不在模型层擅自决定哈希、数据库主键或具体来源的 ID 生成算法。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DocumentId(String);

impl DocumentId {
    /// 创建一个非空文档标识。
    pub fn new(value: impl Into<String>) -> Result<Self, DocumentError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DocumentError::EmptyId);
        }

        Ok(Self(value))
    }

    /// 以字符串视图读取标识。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 当前 M2.0 支持的文档来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentSource {
    /// 一份本地文件；路径只做绝对化，不解析符号链接。
    LocalFile { path: PathBuf },
}

impl DocumentSource {
    /// 从本地文件路径创建来源引用。
    pub fn local_file(path: impl AsRef<Path>) -> Result<Self, DocumentError> {
        let path = normalize_path(path.as_ref())
            .map_err(|source| DocumentError::InvalidSourcePath { source })?;
        Ok(Self::LocalFile { path })
    }

    /// 返回来源对应的本地路径。
    pub fn path(&self) -> &Path {
        match self {
            Self::LocalFile { path } => path,
        }
    }
}

/// 文档在来源中的基础位置元数据。
///
/// 行号从 1 开始；无法提供行范围时使用 [`DocumentLocation::whole_document`]。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DocumentLocation {
    line_start: Option<u64>,
    line_end: Option<u64>,
}

impl DocumentLocation {
    /// 创建没有具体行范围的位置，表示整个文档。
    pub const fn whole_document() -> Self {
        Self {
            line_start: None,
            line_end: None,
        }
    }

    /// 创建一个包含起止行号的位置。
    pub const fn lines(line_start: u64, line_end: u64) -> Result<Self, DocumentError> {
        if line_start == 0 || line_end == 0 || line_end < line_start {
            return Err(DocumentError::InvalidLineRange);
        }

        Ok(Self {
            line_start: Some(line_start),
            line_end: Some(line_end),
        })
    }

    /// 返回起始行号。
    pub const fn line_start(&self) -> Option<u64> {
        self.line_start
    }

    /// 返回结束行号。
    pub const fn line_end(&self) -> Option<u64> {
        self.line_end
    }
}

/// 可交给后续搜索层使用的统一文本文档。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub id: DocumentId,
    pub source: DocumentSource,
    pub title: String,
    pub body: String,
    pub location: DocumentLocation,
}

impl Document {
    /// 创建并校验一份统一文档。
    ///
    /// 正文允许为空，以支持只有标题或尚未提取正文的来源；标题必须存在，
    /// 这样后续列表和检索结果不需要猜测展示名称。
    pub fn new(
        id: DocumentId,
        source: DocumentSource,
        title: impl Into<String>,
        body: impl Into<String>,
        location: DocumentLocation,
    ) -> Result<Self, DocumentError> {
        let title = title.into();
        if title.trim().is_empty() {
            return Err(DocumentError::EmptyTitle);
        }

        Ok(Self {
            id,
            source,
            title,
            body: body.into(),
            location,
        })
    }
}

/// 统一文档模型错误。
#[derive(Debug)]
pub enum DocumentError {
    EmptyId,
    EmptyTitle,
    InvalidSourcePath { source: FileMetadataError },
    InvalidLineRange,
}

impl DocumentError {
    /// 返回不包含用户路径或正文的安全错误分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::EmptyId => "document_id_empty",
            Self::EmptyTitle => "document_title_empty",
            Self::InvalidSourcePath { .. } => "document_source_path_invalid",
            Self::InvalidLineRange => "document_location_invalid",
        }
    }

    /// 返回可以直接展示给用户的非敏感中文说明。
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::EmptyId => "文档标识不能为空。",
            Self::EmptyTitle => "文档标题不能为空。",
            Self::InvalidSourcePath { .. } => "文档来源路径无效。",
            Self::InvalidLineRange => "文档位置范围无效。",
        }
    }
}

impl fmt::Display for DocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "文档模型无效: {}", self.kind())
    }
}

impl Error for DocumentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSourcePath { source } => Some(source),
            Self::EmptyId | Self::EmptyTitle | Self::InvalidLineRange => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Document, DocumentId, DocumentLocation, DocumentSource};

    #[test]
    fn builds_document_with_local_file_source_and_line_location() {
        let source =
            DocumentSource::local_file(Path::new("notes/readme.md")).expect("创建本地文件来源失败");
        let location = DocumentLocation::lines(2, 5).expect("创建文档行范围失败");
        let document = Document::new(
            DocumentId::new("file:notes/readme.md").expect("创建文档 ID 失败"),
            source,
            "README",
            "# Nexus",
            location,
        )
        .expect("创建文档失败");

        assert_eq!(document.id.as_str(), "file:notes/readme.md");
        assert!(document.source.path().is_absolute());
        assert_eq!(document.title, "README");
        assert_eq!(document.body, "# Nexus");
        assert_eq!(document.location.line_start(), Some(2));
        assert_eq!(document.location.line_end(), Some(5));
    }

    #[test]
    fn allows_empty_body_for_metadata_only_document() {
        let source =
            DocumentSource::local_file(Path::new("empty.txt")).expect("创建空正文来源失败");
        let document = Document::new(
            DocumentId::new("file:empty.txt").expect("创建文档 ID 失败"),
            source,
            "空文档",
            "",
            DocumentLocation::whole_document(),
        )
        .expect("创建空正文文档失败");

        assert!(document.body.is_empty());
        assert_eq!(document.location, DocumentLocation::whole_document());
    }

    #[test]
    fn rejects_invalid_document_fields_without_exposing_input() {
        let empty_id = DocumentId::new("  ").expect_err("空 ID 不应成功");
        assert_eq!(empty_id.kind(), "document_id_empty");
        assert_eq!(empty_id.user_message(), "文档标识不能为空。");

        let empty_title = Document::new(
            DocumentId::new("file:test.txt").expect("创建测试文档 ID 失败"),
            DocumentSource::local_file(Path::new("test.txt")).expect("创建测试来源失败"),
            "\t",
            "body",
            DocumentLocation::whole_document(),
        )
        .expect_err("空标题不应成功");
        assert_eq!(empty_title.kind(), "document_title_empty");

        let invalid_location = DocumentLocation::lines(5, 2).expect_err("倒置行范围不应成功");
        assert_eq!(invalid_location.kind(), "document_location_invalid");
        assert!(!invalid_location.to_string().contains("5"));
    }

    #[test]
    fn rejects_empty_source_path_with_safe_error() {
        let error = DocumentSource::local_file(Path::new("")).expect_err("空来源路径不应成功");

        assert_eq!(error.kind(), "document_source_path_invalid");
        assert_eq!(error.user_message(), "文档来源路径无效。");
        assert!(!error.to_string().contains("notes"));
    }
}
