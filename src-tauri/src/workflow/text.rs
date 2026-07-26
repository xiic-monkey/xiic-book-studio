#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Han,
    AsciiWord,
}

fn token_kind(ch: char) -> Option<TokenKind> {
    if is_han(ch) {
        Some(TokenKind::Han)
    } else if ch.is_ascii_alphanumeric() || ch == '-' {
        Some(TokenKind::AsciiWord)
    } else {
        None
    }
}

pub(super) fn split_query_tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_kind: Option<TokenKind> = None;

    for ch in query.chars() {
        let kind = token_kind(ch);
        match kind {
            Some(kind) => {
                if current_kind == Some(kind) {
                    current.push(ch);
                } else {
                    if !current.is_empty() {
                        tokens.push(current.clone());
                        current.clear();
                    }
                    current.push(ch);
                    current_kind = Some(kind);
                }
            }
            None => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
                current_kind = None;
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

pub(super) fn is_han(ch: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&ch)
}

pub(super) fn is_noise_term(term: &str) -> bool {
    const NOISE: &[&str] = &[
        "主角",
        "当前章节",
        "章节目标",
        "核心冲突",
        "信息释放",
        "结尾钩子",
        "这一章",
        "不要",
        "需要",
        "继续",
        "直接",
        "控制",
        "通过",
        "问题",
        "线索",
        "章节",
        "规则",
        "身份",
        "编号",
        "自己",
        "东西",
        "一个",
        "两个",
        "三年前",
        "本章",
        "上一章",
        "前文",
        "任务",
        "目标",
    ];

    term.chars().count() <= 1 || NOISE.contains(&term) || term.chars().all(|ch| ch.is_ascii_digit())
}

pub(super) fn score_term(term: &str) -> usize {
    let has_digits = term.chars().any(|ch| ch.is_ascii_digit());
    let has_hyphen = term.contains('-');
    let len = term.chars().count();
    (if has_digits { 5 } else { 0 }) + (if has_hyphen { 4 } else { 0 }) + len.min(8)
}

pub(super) fn excerpt_around(
    text: &str,
    match_byte_index: usize,
    term_chars: usize,
    window: usize,
) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let match_char_index = text[..match_byte_index].chars().count();
    let start = match_char_index.saturating_sub(window / 2);
    let end = (match_char_index + term_chars + window / 2).min(chars.len());
    let excerpt = chars[start..end].iter().collect::<String>();
    excerpt.split_whitespace().collect::<Vec<_>>().join(" ")
}
