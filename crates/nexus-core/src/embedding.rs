//! 本地文本向量提供者边界。
//!
//! M5.0 先固定一个不依赖网络、运行时模型文件或第三方推理引擎的确定性特征
//! 向量基线。它用于验证向量索引和混合检索的数据流，不宣称具备通用的预训练
//! 语言模型语义能力。后续替换为真正的本地模型时，数据库通过 model_id 和
//! model_version 区分不同向量，不覆盖旧版本数据。

use std::{error::Error, fmt};

/// 当前本地特征向量提供者的稳定标识。
pub const LOCAL_EMBEDDING_MODEL_ID: &str = "nexus-local-feature-hash";

/// 当前本地特征向量提供者的版本。
pub const LOCAL_EMBEDDING_MODEL_VERSION: &str = "1";

/// 当前基线向量的固定维度。
pub const LOCAL_EMBEDDING_DIMENSIONS: usize = 256;

const TITLE_WEIGHT: f32 = 2.0;
const BODY_WEIGHT: f32 = 1.0;
const FEATURE_HASH_SEED: u64 = 0xcbf2_9ce4_8422_2325;
const SIGN_HASH_SEED: u64 = 0x9e37_79b9_7f4a_7c15;

/// 向量提供者的统一能力边界。
///
/// 提供者只接收调用方明确传入的文本，不读取文件、不访问网络，也不负责存储。
/// 生产调用方应同时使用 [`EmbeddingProvider::model_id`]、版本和维度保存向量
/// 的身份信息。
pub trait EmbeddingProvider {
    /// 返回稳定的模型/算法标识。
    fn model_id(&self) -> &'static str;

    /// 返回提供者版本；版本变化表示已有向量不可直接复用。
    fn model_version(&self) -> &'static str;

    /// 返回提供者类别，用于本地存储中的诊断信息。
    fn provider_kind(&self) -> &'static str {
        "local"
    }

    /// 返回输出向量维度。
    fn dimensions(&self) -> usize;

    /// 将一段文本转换为 L2 归一化向量。
    fn embed(&self, text: &str) -> Result<EmbeddingVector, EmbeddingError>;

    /// 将统一文档的标题和正文转换为向量。
    ///
    /// 文档范围固定为标题和正文；来源路径、文件名和访问时间不进入向量，避免
    /// 把路径偶然相似误当成内容相似，也方便未来对同一文档模型做版本化重建。
    fn embed_document(&self, title: &str, body: &str) -> Result<EmbeddingVector, EmbeddingError> {
        let mut text = String::with_capacity(title.len() + body.len() + 1);
        text.push_str(title);
        text.push('\n');
        text.push_str(body);
        self.embed(&text)
    }
}

/// 一个已经通过维度和有限数值校验的向量。
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingVector {
    values: Vec<f32>,
}

impl EmbeddingVector {
    /// 从调用方提供的数值创建向量并执行严格校验。
    pub fn new(values: Vec<f32>) -> Result<Self, EmbeddingError> {
        if values.is_empty() {
            return Err(EmbeddingError::InvalidDimensions {
                expected: 1,
                actual: values.len(),
            });
        }

        if values.iter().any(|value| !value.is_finite()) {
            return Err(EmbeddingError::NonFiniteValue);
        }

        let norm = squared_norm(&values).sqrt();
        if norm <= f32::EPSILON {
            return Err(EmbeddingError::ZeroNorm);
        }

        Ok(Self { values })
    }

    fn new_for_dimensions(
        values: Vec<f32>,
        expected_dimensions: usize,
    ) -> Result<Self, EmbeddingError> {
        if values.len() != expected_dimensions {
            return Err(EmbeddingError::InvalidDimensions {
                expected: expected_dimensions,
                actual: values.len(),
            });
        }

        Self::new(values)
    }

    /// 返回向量的只读数值切片。
    pub fn as_slice(&self) -> &[f32] {
        &self.values
    }

    /// 返回向量维度。
    pub fn dimensions(&self) -> usize {
        self.values.len()
    }

    /// 计算两个同维归一化向量的余弦相似度。
    pub fn cosine_similarity(&self, other: &Self) -> Result<f32, EmbeddingError> {
        if self.dimensions() != other.dimensions() {
            return Err(EmbeddingError::InvalidDimensions {
                expected: self.dimensions(),
                actual: other.dimensions(),
            });
        }

        let left_norm = squared_norm(&self.values).sqrt();
        let right_norm = squared_norm(&other.values).sqrt();
        if left_norm <= f32::EPSILON || right_norm <= f32::EPSILON {
            return Err(EmbeddingError::ZeroNorm);
        }

        Ok((self
            .values
            .iter()
            .zip(&other.values)
            .map(|(left, right)| left * right)
            .sum::<f32>()
            / (left_norm * right_norm))
            .clamp(-1.0, 1.0))
    }
}

/// 当前默认的本地确定性特征向量提供者。
///
/// 它对词和 Unicode 字符三元组做稳定的 signed hashing，再做 L2 归一化。这个
/// 选择没有模型下载、网络访问或平台推理依赖，适合作为 M5 的管线基线；它不会
/// 像预训练语言模型那样理解未共享词汇的同义关系。
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalFeatureEmbedding;

impl LocalFeatureEmbedding {
    /// 创建默认本地特征向量提供者。
    pub const fn new() -> Self {
        Self
    }
}

impl EmbeddingProvider for LocalFeatureEmbedding {
    fn model_id(&self) -> &'static str {
        LOCAL_EMBEDDING_MODEL_ID
    }

    fn model_version(&self) -> &'static str {
        LOCAL_EMBEDDING_MODEL_VERSION
    }

    fn provider_kind(&self) -> &'static str {
        "local_feature_hash"
    }

    fn dimensions(&self) -> usize {
        LOCAL_EMBEDDING_DIMENSIONS
    }

    fn embed(&self, text: &str) -> Result<EmbeddingVector, EmbeddingError> {
        if text.trim().is_empty() {
            return Err(EmbeddingError::EmptyText);
        }

        let mut values = vec![0.0; self.dimensions()];
        add_text_features(&mut values, text, BODY_WEIGHT);

        let norm = squared_norm(&values).sqrt();
        if norm <= f32::EPSILON {
            return Err(EmbeddingError::ZeroNorm);
        }
        for value in &mut values {
            *value /= norm;
        }

        EmbeddingVector::new_for_dimensions(values, self.dimensions())
    }

    fn embed_document(&self, title: &str, body: &str) -> Result<EmbeddingVector, EmbeddingError> {
        if title.trim().is_empty() {
            return Err(EmbeddingError::EmptyTitle);
        }
        let mut values = vec![0.0; self.dimensions()];
        add_text_features(&mut values, title, TITLE_WEIGHT);
        add_text_features(&mut values, body, BODY_WEIGHT);

        let norm = squared_norm(&values).sqrt();
        if norm <= f32::EPSILON {
            return Err(EmbeddingError::ZeroNorm);
        }
        for value in &mut values {
            *value /= norm;
        }

        EmbeddingVector::new_for_dimensions(values, self.dimensions())
    }
}

/// 向量生成失败时的安全错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingError {
    EmptyText,
    EmptyTitle,
    InvalidDimensions { expected: usize, actual: usize },
    NonFiniteValue,
    ZeroNorm,
}

impl EmbeddingError {
    /// 返回不包含输入正文的稳定错误分类。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::EmptyText => "embedding_text_empty",
            Self::EmptyTitle => "embedding_title_empty",
            Self::InvalidDimensions { .. } => "embedding_dimensions_invalid",
            Self::NonFiniteValue => "embedding_value_non_finite",
            Self::ZeroNorm => "embedding_zero_norm",
        }
    }

    /// 返回可直接展示给用户的安全说明。
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::EmptyText => "没有可用于生成向量的文本。",
            Self::EmptyTitle => "文档标题不能为空。",
            Self::InvalidDimensions { .. } | Self::NonFiniteValue | Self::ZeroNorm => {
                "本地向量数据无效。"
            }
        }
    }
}

impl fmt::Display for EmbeddingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "本地向量生成失败: {}", self.kind())
    }
}

impl Error for EmbeddingError {}

fn add_text_features(values: &mut [f32], text: &str, weight: f32) {
    let mut token = String::new();

    for character in text.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() || character == '_' {
            token.push(character);
        } else {
            add_token_features(values, &token, weight);
            token.clear();
        }
    }
    add_token_features(values, &token, weight);
}

fn add_token_features(values: &mut [f32], token: &str, weight: f32) {
    if token.is_empty() {
        return;
    }

    add_feature(values, token.as_bytes(), weight);

    let characters: Vec<char> = token.chars().collect();
    if characters.len() < 3 {
        return;
    }

    for window in characters.windows(3) {
        let mut feature =
            String::with_capacity(window.iter().map(|character| character.len_utf8()).sum());
        feature.extend(window.iter().copied());
        add_feature(values, feature.as_bytes(), weight * 0.5);
    }
}

fn add_feature(values: &mut [f32], feature: &[u8], weight: f32) {
    let bucket = (fnv1a(FEATURE_HASH_SEED, feature) as usize) % values.len();
    let sign = if fnv1a(SIGN_HASH_SEED, feature) & 1 == 0 {
        1.0
    } else {
        -1.0
    };
    values[bucket] += sign * weight;
}

fn fnv1a(seed: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(seed, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x1000_0000_01b3)
    })
}

/// 计算标题和正文输入的稳定、非加密指纹。
///
/// 指纹只用于判断某个向量对应的输入版本，不用于安全认证，也不包含路径或文件名。
pub fn document_input_fingerprint(title: &str, body: &str) -> [u8; 16] {
    let first = fnv1a_parts(FEATURE_HASH_SEED, title, body);
    let second = fnv1a_parts(SIGN_HASH_SEED, title, body);
    let mut fingerprint = [0_u8; 16];
    fingerprint[..8].copy_from_slice(&first.to_le_bytes());
    fingerprint[8..].copy_from_slice(&second.to_le_bytes());
    fingerprint
}

fn fnv1a_parts(seed: u64, title: &str, body: &str) -> u64 {
    let hash = title.as_bytes().iter().fold(seed, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x1000_0000_01b3)
    });
    let hash = hash.wrapping_mul(0x1000_0000_01b3);
    body.as_bytes().iter().fold(hash, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x1000_0000_01b3)
    })
}

fn squared_norm(values: &[f32]) -> f32 {
    values.iter().map(|value| value * value).sum()
}

#[cfg(test)]
mod tests {
    use super::{
        EmbeddingError, EmbeddingProvider, EmbeddingVector, LocalFeatureEmbedding,
        LOCAL_EMBEDDING_DIMENSIONS, LOCAL_EMBEDDING_MODEL_ID, LOCAL_EMBEDDING_MODEL_VERSION,
    };

    #[test]
    fn produces_stable_normalized_vectors_without_network_access() {
        let provider = LocalFeatureEmbedding::new();
        let first = provider
            .embed("Local-first data search")
            .expect("生成本地向量失败");
        let second = provider
            .embed("Local-first data search")
            .expect("重复生成本地向量失败");

        assert_eq!(provider.model_id(), LOCAL_EMBEDDING_MODEL_ID);
        assert_eq!(provider.model_version(), LOCAL_EMBEDDING_MODEL_VERSION);
        assert_eq!(provider.dimensions(), LOCAL_EMBEDDING_DIMENSIONS);
        assert_eq!(first, second);
        let norm = first
            .as_slice()
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        assert!((norm - 1.0).abs() < 0.0001);
    }

    #[test]
    fn document_embedding_weights_title_and_ignores_path_metadata() {
        let provider = LocalFeatureEmbedding::new();
        let first = provider
            .embed_document("Quarterly Plan", "Owners and dates")
            .expect("生成第一条文档向量失败");
        let second = provider
            .embed_document("Quarterly Plan", "Owners and dates")
            .expect("生成第二条文档向量失败");

        assert_eq!(first, second);
        assert!(first.cosine_similarity(&second).expect("计算相似度失败") > 0.9999);
    }

    #[test]
    fn rejects_empty_inputs_and_invalid_vectors() {
        let provider = LocalFeatureEmbedding::new();

        assert_eq!(
            provider.embed("   ").expect_err("空文本不应生成向量"),
            EmbeddingError::EmptyText
        );
        assert_eq!(
            provider
                .embed_document("", "body")
                .expect_err("空标题不应生成文档向量"),
            EmbeddingError::EmptyTitle
        );
        assert_eq!(
            EmbeddingVector::new(vec![0.0; LOCAL_EMBEDDING_DIMENSIONS])
                .expect_err("零向量不应通过校验"),
            EmbeddingError::ZeroNorm
        );
        assert_eq!(
            EmbeddingVector::new(Vec::new()).expect_err("空向量不应通过校验"),
            EmbeddingError::InvalidDimensions {
                expected: 1,
                actual: 0,
            }
        );

        let first = EmbeddingVector::new(vec![2.0, 0.0]).expect("创建非归一化向量失败");
        let second = EmbeddingVector::new(vec![4.0, 0.0]).expect("创建第二个向量失败");
        assert!(
            (first
                .cosine_similarity(&second)
                .expect("计算余弦相似度失败")
                - 1.0)
                .abs()
                < 0.0001
        );
    }

    #[test]
    fn supports_unicode_subword_features() {
        let provider = LocalFeatureEmbedding::new();
        let first = provider
            .embed("项目计划与恢复")
            .expect("中文文本应生成向量");
        let second = provider.embed("项目计划").expect("中文短文本应生成向量");

        assert!(
            first
                .cosine_similarity(&second)
                .expect("计算中文相似度失败")
                > 0.0
        );
    }
}
