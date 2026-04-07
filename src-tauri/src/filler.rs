//! 纯语气词过滤 — 基于 segment 的保守过滤策略。
//!
//! 只删除整个 segment（标点/空白分隔的片段）全为语气词的部分，
//! 不做子串级替换，避免误删正常文本（如"嗯按钮"中的"嗯"）。

const FILLERS: &[&str] = &[
    "就是说呃",
    "怎么说呢",
    "就是说",
    "呃呃呃",
    "嗯嗯嗯",
    "啊啊啊",
    "呃呃",
    "嗯嗯",
    "啊啊",
    "呃",
    "嗯",
    "额",
    "唔",
    "噢",
];

const DELIMITERS: &[char] = &[
    '，', '。', '！', '？', '、', '；', '：', // 中文标点
    ',', '.', '!', '?', ';', ':',             // ASCII 标点
    ' ', '\n', '\t', '\u{3000}',              // 空白字符（含全角空格）
];

/// Returns true if the segment (after removing filler words) is empty,
/// meaning the entire segment consists only of filler words.
fn is_pure_filler(segment: &str) -> bool {
    let mut remaining = segment.to_string();
    for filler in FILLERS {
        remaining = remaining.replace(filler, "");
    }
    remaining.trim().is_empty()
}

pub fn clean_pure_fillers(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    // Split text into segments by delimiters, preserving delimiters
    let mut segments: Vec<&str> = Vec::new();
    let mut last = 0;
    for (i, c) in text.char_indices() {
        if DELIMITERS.contains(&c) {
            if i > last {
                segments.push(&text[last..i]);
            }
            segments.push(&text[i..i + c.len_utf8()]);
            last = i + c.len_utf8();
        }
    }
    if last < text.len() {
        segments.push(&text[last..]);
    }

    let mut result = String::with_capacity(text.len());
    let mut skip_next_delimiter = false;

    for seg in &segments {
        let first_char = seg.chars().next().unwrap_or(' ');

        if DELIMITERS.contains(&first_char) {
            if skip_next_delimiter {
                skip_next_delimiter = false;
                continue;
            }
            result.push_str(seg);
            continue;
        }

        if is_pure_filler(seg) {
            skip_next_delimiter = true;
            continue;
        }

        skip_next_delimiter = false;
        result.push_str(seg);
    }

    // Trim trailing delimiters and whitespace
    let trimmed = result.trim();
    let trimmed = trimmed.trim_end_matches(|c: char| DELIMITERS.contains(&c));
    trimmed.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_pure_filler_segments() {
        assert_eq!(clean_pure_fillers("嗯，今天天气不错"), "今天天气不错");
        assert_eq!(clean_pure_fillers("呃呃呃，你好"), "你好");
        assert_eq!(clean_pure_fillers("嗯嗯 好的"), "好的");
    }

    #[test]
    fn preserves_normal_text_with_filler_chars() {
        // "嗯" is part of a larger word — should NOT be removed
        assert_eq!(clean_pure_fillers("嗯哼，好的"), "嗯哼，好的");
    }

    #[test]
    fn preserves_connectors() {
        assert_eq!(
            clean_pure_fillers("然后，我们去吃饭"),
            "然后，我们去吃饭"
        );
        assert_eq!(clean_pure_fillers("但是这个不行"), "但是这个不行");
    }

    #[test]
    fn handles_empty_and_all_filler() {
        assert_eq!(clean_pure_fillers(""), "");
        assert_eq!(clean_pure_fillers("嗯"), "");
        assert_eq!(clean_pure_fillers("嗯嗯嗯"), "");
    }

    #[test]
    fn handles_mixed_content() {
        assert_eq!(
            clean_pure_fillers("嗯，今天，呃，天气不错，嗯"),
            "今天，天气不错"
        );
    }
}
