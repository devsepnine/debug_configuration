//! IntelliJ-호환 환경변수 문자열 파싱/직렬화.
//!
//! 형식: `KEY1=value1;KEY2=value2;...`
//! 구분자: `;` 또는 개행
//! Escape: `\;`, `\n`, `\\`
//! 첫 번째 `=` 이전이 키, 이후가 값.

use std::collections::HashMap;

/// 환경변수 문자열을 (key, value) 쌍의 Vec으로 파싱.
///
/// - 빈 항목, 빈 키, `=`가 없는 항목은 무시
/// - 같은 키 중복 시 모두 포함 (호출자가 처리)
/// - key/value 공백 trim
pub fn parse_env_string(input: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    for raw in split_unescaped(input) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((key_part, value_part)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key_part.trim().to_string();
        if key.is_empty() {
            continue;
        }
        let value = unescape(value_part.trim());
        entries.push((key, value));
    }
    entries
}

/// `HashMap`을 환경변수 문자열로 직렬화. 키는 알파벳 정렬.
pub fn serialize_env_map(map: &HashMap<String, String>) -> String {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    keys.iter()
        .map(|k| {
            let value = map.get(*k).map(String::as_str).unwrap_or("");
            format!("{}={}", k, escape(value))
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn split_unescaped(input: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            current.push('\\');
            if let Some(&next) = chars.peek() {
                current.push(next);
                chars.next();
            }
        } else if c == ';' || c == '\n' {
            result.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    result.push(current);
    result
}

fn unescape(value: &str) -> String {
    let mut result = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(';') => result.push(';'),
                Some('n') => result.push('\n'),
                Some('\\') => result.push('\\'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn escape(value: &str) -> String {
    let mut result = String::new();
    for c in value.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            ';' => result.push_str("\\;"),
            '\n' => result.push_str("\\n"),
            _ => result.push(c),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn parses_basic_pairs() {
        assert_eq!(
            parse_env_string("KEY1=value1;KEY2=value2"),
            entries(&[("KEY1", "value1"), ("KEY2", "value2")]),
        );
    }

    #[test]
    fn ignores_trailing_and_empty_entries() {
        assert_eq!(
            parse_env_string("KEY1=v1;;KEY2=v2;"),
            entries(&[("KEY1", "v1"), ("KEY2", "v2")]),
        );
    }

    #[test]
    fn preserves_value_with_equals_sign() {
        assert_eq!(
            parse_env_string("URL=http://a.com/?x=1"),
            entries(&[("URL", "http://a.com/?x=1")]),
        );
    }

    #[test]
    fn trims_key_and_value_whitespace() {
        assert_eq!(
            parse_env_string("  KEY  =  value  "),
            entries(&[("KEY", "value")]),
        );
    }

    #[test]
    fn handles_escaped_semicolon_in_value() {
        assert_eq!(
            parse_env_string("PATH=a\\;b;NEXT=c"),
            entries(&[("PATH", "a;b"), ("NEXT", "c")]),
        );
    }

    #[test]
    fn handles_escaped_backslash_and_newline() {
        assert_eq!(
            parse_env_string("X=a\\\\b;Y=line\\nbreak"),
            entries(&[("X", "a\\b"), ("Y", "line\nbreak")]),
        );
    }

    #[test]
    fn supports_newline_separator() {
        assert_eq!(
            parse_env_string("A=1\nB=2"),
            entries(&[("A", "1"), ("B", "2")]),
        );
    }

    #[test]
    fn ignores_empty_key_and_no_equals() {
        assert_eq!(
            parse_env_string("=value;BAREKEY;OK=ok"),
            entries(&[("OK", "ok")]),
        );
    }

    #[test]
    fn serializes_alphabetically() {
        let mut map = HashMap::new();
        map.insert("ZED".to_string(), "z".to_string());
        map.insert("ALPHA".to_string(), "a".to_string());
        map.insert("MID".to_string(), "m".to_string());
        assert_eq!(serialize_env_map(&map), "ALPHA=a;MID=m;ZED=z");
    }

    #[test]
    fn serializes_empty_map_to_empty_string() {
        assert_eq!(serialize_env_map(&HashMap::new()), "");
    }

    #[test]
    fn serializes_escapes_special_chars() {
        let mut map = HashMap::new();
        map.insert("PATH".to_string(), "a;b".to_string());
        map.insert("WIN".to_string(), "C:\\Users".to_string());
        assert_eq!(serialize_env_map(&map), "PATH=a\\;b;WIN=C:\\\\Users",);
    }

    #[test]
    fn round_trip_preserves_values() {
        let mut map = HashMap::new();
        map.insert("A".to_string(), "simple".to_string());
        map.insert("B".to_string(), "has;semicolon".to_string());
        map.insert("C".to_string(), "back\\slash".to_string());
        let text = serialize_env_map(&map);
        let parsed: HashMap<String, String> = parse_env_string(&text).into_iter().collect();
        assert_eq!(parsed, map);
    }
}
