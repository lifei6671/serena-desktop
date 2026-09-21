//! Source Write 的纯文本换行策略；不读取文件，也不提供 public Tool handler。
#![allow(
    dead_code,
    reason = "P2C-005 establishes the future text primitive before P2C-008 through P2C-011 handlers consume it."
)]

/// 目标文件或待写入正文使用的两种受支持换行风格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NewlineStyle {
    Lf,
    CrLf,
}

impl NewlineStyle {
    /// 返回该风格的唯一换行 token，供 normalization 写入结果使用。
    fn token(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

/// 识别既有 UTF-8 文本的 dominant newline；孤立 CR 不属于 newline token。
pub(crate) fn detect_newline_style(existing: &str) -> NewlineStyle {
    let bytes = existing.as_bytes();
    let mut index = 0;
    let mut lf_count = 0;
    let mut crlf_count = 0;
    let mut first_observed = None;

    while index < bytes.len() {
        let style = match bytes[index] {
            // CRLF 必须整体消耗，避免其中的 LF 被当作第二个 token 计数。
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                index += 2;
                NewlineStyle::CrLf
            }
            b'\n' => {
                index += 1;
                NewlineStyle::Lf
            }
            // 孤立 CR 与其他所有文本都不参与 newline detection。
            _ => {
                index += 1;
                continue;
            }
        };

        if first_observed.is_none() {
            first_observed = Some(style);
        }
        match style {
            NewlineStyle::Lf => lf_count += 1,
            NewlineStyle::CrLf => crlf_count += 1,
        }
    }

    match crlf_count.cmp(&lf_count) {
        std::cmp::Ordering::Greater => NewlineStyle::CrLf,
        std::cmp::Ordering::Less => NewlineStyle::Lf,
        std::cmp::Ordering::Equal => first_observed.unwrap_or(NewlineStyle::Lf),
    }
}

/// 将 CRLF 与孤立 LF 统一为目标风格，不改变孤立 CR、普通字符或 final newline 状态。
pub(crate) fn normalize_newlines(content: &str, style: NewlineStyle) -> String {
    let bytes = content.as_bytes();
    let mut normalized = String::with_capacity(content.len());
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            // CRLF 作为一个 token 替换，确保不会遗留 CR 或再次处理其 LF。
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                normalized.push_str(style.token());
                index += 2;
            }
            b'\n' => {
                normalized.push_str(style.token());
                index += 1;
            }
            _ => {
                // index 始终位于原 UTF-8 code point 边界；仅逐字符复制非换行文本。
                let character = content[index..]
                    .chars()
                    .next()
                    .expect("index is within the source text");
                normalized.push(character);
                index += character.len_utf8();
            }
        }
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::{NewlineStyle, detect_newline_style, normalize_newlines};

    /// 纯 LF 文本保持 LF 作为 dominant style。
    #[test]
    fn detects_lf_only() {
        assert_eq!(detect_newline_style("one\ntwo\n"), NewlineStyle::Lf);
    }

    /// CRLF 中的 LF 不能被再次计数，否则这个输入会错误地判为 LF dominant。
    #[test]
    fn detects_crlf_only_without_double_counting_its_lf() {
        assert_eq!(detect_newline_style("one\r\ntwo\r\n"), NewlineStyle::CrLf);
        assert_eq!(
            detect_newline_style("one\r\ntwo\r\nthree\n"),
            NewlineStyle::CrLf
        );
    }

    /// 混合文本按出现次数选择 LF dominant style。
    #[test]
    fn detects_lf_dominant_mixed_text() {
        assert_eq!(
            detect_newline_style("one\ntwo\nthree\r\n"),
            NewlineStyle::Lf
        );
    }

    /// 混合文本按出现次数选择 CRLF dominant style。
    #[test]
    fn detects_crlf_dominant_mixed_text() {
        assert_eq!(
            detect_newline_style("one\r\ntwo\r\nthree\n"),
            NewlineStyle::CrLf
        );
    }

    /// 相同数量的两种 token 使用第一个实际出现的风格作为确定性 tie-break。
    #[test]
    fn detects_tie_using_first_lf() {
        assert_eq!(detect_newline_style("one\ntwo\r\n"), NewlineStyle::Lf);
    }

    /// 相同数量的两种 token 使用第一个实际出现的风格作为确定性 tie-break。
    #[test]
    fn detects_tie_using_first_crlf() {
        assert_eq!(detect_newline_style("one\r\ntwo\n"), NewlineStyle::CrLf);
    }

    /// 没有任何受支持 token 的文本采用 LF 默认值，孤立 CR 不例外。
    #[test]
    fn defaults_to_lf_without_newline_tokens_including_isolated_cr() {
        assert_eq!(detect_newline_style("one\rtwo"), NewlineStyle::Lf);
    }

    /// normalization 将 LF 统一为 CRLF，且不额外补 final newline。
    #[test]
    fn normalizes_lf_to_crlf() {
        assert_eq!(
            normalize_newlines("one\ntwo", NewlineStyle::CrLf),
            "one\r\ntwo"
        );
    }

    /// normalization 将 CRLF 统一为 LF，且不遗留 CR。
    #[test]
    fn normalizes_crlf_to_lf() {
        assert_eq!(
            normalize_newlines("one\r\ntwo\r\n", NewlineStyle::Lf),
            "one\ntwo\n"
        );
    }

    /// 混合 newline 统一为单一风格，同时保留 Unicode 与孤立 CR 的原始字节序列。
    #[test]
    fn normalizes_mixed_content_without_changing_unicode_or_isolated_cr() {
        assert_eq!(
            normalize_newlines("中文\nemoji😀\r\nkeep\rcr", NewlineStyle::CrLf),
            "中文\r\nemoji😀\r\nkeep\rcr"
        );
    }

    /// 空正文保持为空，不引入任何 newline。
    #[test]
    fn normalizes_empty_content_without_adding_final_newline() {
        assert_eq!(normalize_newlines("", NewlineStyle::Lf), "");
        assert_eq!(normalize_newlines("", NewlineStyle::CrLf), "");
    }
}
