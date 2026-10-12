use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// 환경변수 한 항목. `enabled == false`이면 저장만 되고 실행에는 주입되지 않는다.
///
/// `deny_unknown_fields`: `"enable": false` 같은 오타가 기본값(활성)으로 읽히면 사용자가 꺼 둔
/// 값이 조용히 실행에 주입된다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvEntry {
    pub key: String,
    pub value: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl EnvEntry {
    pub fn enabled(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            enabled: true,
        }
    }
}

/// 구성의 환경변수 목록. 입력 순서를 유지하며, 키는 활성·비활성을 통틀어 유일하다.
///
/// 키 유일성을 이 타입이 지키므로 모든 생성 경로(역직렬화 포함)가 `from_entries`의 dedupe를
/// 거친다 — 같은 키가 둘이면 어느 쪽이 실행에 쓰이는지가 호출 경로마다 달라진다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct EnvVars(Vec<EnvEntry>);

impl EnvVars {
    /// 중복 키는 첫 등장 위치에 마지막 항목의 값·활성 상태로 합친다 — `HashMap`으로 모으던
    /// 이전 동작(마지막 값 우선)과 같은 결과를 내면서 순서는 보존한다.
    pub fn from_entries(entries: impl IntoIterator<Item = EnvEntry>) -> Self {
        let mut merged: Vec<EnvEntry> = Vec::new();
        for entry in entries {
            match merged.iter_mut().find(|e| e.key == entry.key) {
                Some(existing) => *existing = entry,
                None => merged.push(entry),
            }
        }
        Self(merged)
    }

    pub fn entries(&self) -> &[EnvEntry] {
        &self.0
    }

    /// 실행에 주입되는 항목만.
    pub fn active(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .filter(|e| e.enabled)
            .map(|e| (e.key.as_str(), e.value.as_str()))
    }

    pub fn get(&self, key: &str) -> Option<&EnvEntry> {
        self.0.iter().find(|e| e.key == key)
    }

    /// 값을 설정하고 바뀌었는지 돌려준다. 기존 키의 활성 상태는 유지하고, 새 키는 활성으로 넣는다.
    pub fn set(&mut self, key: String, value: String) -> bool {
        match self.0.iter_mut().find(|e| e.key == key) {
            Some(existing) if existing.value == value => false,
            Some(existing) => {
                existing.value = value;
                true
            }
            None => {
                self.0.push(EnvEntry::enabled(key, value));
                true
            }
        }
    }

    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.0.len();
        self.0.retain(|e| e.key != key);
        self.0.len() != before
    }

    /// 없는 키는 무시한다. 활성 상태가 실제로 바뀌었는지 돌려준다.
    pub fn set_enabled(&mut self, key: &str, enabled: bool) -> bool {
        match self.0.iter_mut().find(|e| e.key == key) {
            Some(entry) if entry.enabled != enabled => {
                entry.enabled = enabled;
                true
            }
            _ => false,
        }
    }

    /// 메인 한 줄 input의 내용으로 활성 항목을 교체한다. 비활성 항목은 손대지 않는다.
    ///
    /// 비활성 키와 같은 키가 입력되면 그 입력은 버린다. 입력을 활성으로 승격하면 타이핑 도중의
    /// 중간 상태(`AB=1`을 지우다 지나가는 `A=1`)가 비활성 항목을 덮어쓴 뒤 다음 키 입력에서
    /// 지워 버린다 — 키 입력마다 이 함수가 다시 불리기 때문이다(`handle_env_bulk_input_changed`).
    /// 충돌은 편집기가 경고 테두리로 보여 준다(`collides_with_disabled`).
    pub fn replace_active(&mut self, pairs: Vec<(String, String)>) {
        let mut typed = Self::from_iter(pairs).0;
        typed.retain(|t| !self.0.iter().any(|e| !e.enabled && e.key == t.key));
        let mut result = Vec::with_capacity(self.0.len().max(typed.len()));
        for entry in self.0.drain(..) {
            if !entry.enabled {
                result.push(entry);
            } else if let Some(pos) = typed.iter().position(|t| t.key == entry.key) {
                result.push(typed.remove(pos));
            }
        }
        result.extend(typed);
        self.0 = result;
    }

    pub fn collides_with_disabled(&self, pairs: &[(String, String)]) -> bool {
        pairs
            .iter()
            .any(|(key, _)| self.get(key).is_some_and(|e| !e.enabled))
    }
}

/// 모달 staging entries 중 동일 키가 둘 이상 있는 row index 집합.
/// trim 후 빈 키는 중복 검사에서 제외 (placeholder/입력 중인 row 보호).
/// 뷰(경고 테두리·OK 비활성)와 Confirm 핸들러가 같은 판정을 쓰도록 여기 둔다.
pub fn duplicate_key_indices(entries: &[EnvEntry]) -> std::collections::HashSet<usize> {
    use std::collections::HashMap;
    let mut by_key: HashMap<&str, Vec<usize>> = HashMap::new();
    for (idx, entry) in entries.iter().enumerate() {
        let trimmed = entry.key.trim();
        if !trimmed.is_empty() {
            by_key.entry(trimmed).or_default().push(idx);
        }
    }
    by_key
        .into_values()
        .filter(|indices| indices.len() > 1)
        .flatten()
        .collect()
}

impl FromIterator<(String, String)> for EnvVars {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(pairs: I) -> Self {
        Self::from_entries(pairs.into_iter().map(|(k, v)| EnvEntry::enabled(k, v)))
    }
}

/// v2 이하 파일은 `{"KEY": "value"}` 맵이었다. 그 항목은 모두 활성이고, 맵에는 순서가 없으므로
/// 키 순으로 정렬해 매 로드가 같은 순서를 내게 한다.
impl<'de> Deserialize<'de> for EnvVars {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Entries(Vec<EnvEntry>),
            LegacyMap(HashMap<String, String>),
        }
        // untagged의 기본 오류("did not match any variant of untagged enum Wire")는 어느 필드인지
        // 말하지 않아, 구성 파일 전체가 열리지 않을 때 사용자가 고칠 곳을 찾을 수 없다.
        let wire = Wire::deserialize(deserializer).map_err(|_| {
            serde::de::Error::custom(
                "environment_variables must be a list of {key, value, enabled} entries \
                 (or a legacy {\"KEY\": \"value\"} map)",
            )
        })?;
        Ok(match wire {
            Wire::Entries(entries) => Self::from_entries(entries),
            Wire::LegacyMap(map) => {
                let mut pairs: Vec<(String, String)> = map.into_iter().collect();
                pairs.sort_by(|a, b| a.0.cmp(&b.0));
                pairs.into_iter().collect()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(k: &str, v: &str) -> (String, String) {
        (k.to_string(), v.to_string())
    }

    fn disabled(k: &str, v: &str) -> EnvEntry {
        EnvEntry {
            enabled: false,
            ..EnvEntry::enabled(k, v)
        }
    }

    #[test]
    fn legacy_map_loads_as_enabled_entries_sorted_by_key() {
        let env: EnvVars = serde_json::from_str(r#"{"ZED":"z","ALPHA":"a"}"#).unwrap();
        assert_eq!(
            env.entries(),
            &[
                EnvEntry::enabled("ALPHA", "a"),
                EnvEntry::enabled("ZED", "z")
            ]
        );
    }

    #[test]
    fn entries_round_trip_and_missing_enabled_defaults_to_true() {
        let env = EnvVars::from_entries([EnvEntry::enabled("B", "1"), disabled("A", "2")]);
        let json = serde_json::to_string(&env).unwrap();
        assert_eq!(serde_json::from_str::<EnvVars>(&json).unwrap(), env);

        let loose: EnvVars = serde_json::from_str(r#"[{"key":"K","value":"v"}]"#).unwrap();
        assert!(loose.entries()[0].enabled);
    }

    #[test]
    fn duplicate_keys_merge_into_first_position_with_last_value() {
        let env: EnvVars = serde_json::from_str(
            r#"[{"key":"A","value":"1"},{"key":"B","value":"2"},{"key":"A","value":"3","enabled":false}]"#,
        )
        .unwrap();
        assert_eq!(
            env.entries(),
            &[disabled("A", "3"), EnvEntry::enabled("B", "2")]
        );
    }

    #[test]
    fn malformed_env_is_rejected_with_a_readable_error() {
        for bad in [
            r#"[{"key":"A"}]"#,
            r#"{"A":1}"#,
            r#"[{"key":"A","value":"x","enable":false}]"#,
        ] {
            let err = serde_json::from_str::<EnvVars>(bad).expect_err(bad);
            assert!(err.to_string().contains("environment_variables"), "{err}");
        }
        assert!(
            serde_json::from_str::<EnvVars>("{}")
                .unwrap()
                .entries()
                .is_empty()
        );
        assert!(
            serde_json::from_str::<EnvVars>("[]")
                .unwrap()
                .entries()
                .is_empty()
        );
    }

    #[test]
    fn active_skips_disabled_entries() {
        let env = EnvVars::from_entries([EnvEntry::enabled("A", "1"), disabled("B", "2")]);
        assert_eq!(env.active().collect::<Vec<_>>(), vec![("A", "1")]);
    }

    #[test]
    fn set_keeps_the_existing_enabled_state() {
        let mut env = EnvVars::from_entries([disabled("A", "1")]);
        assert!(env.set("A".into(), "2".into()));
        assert_eq!(env.get("A"), Some(&disabled("A", "2")));
        assert!(!env.set("A".into(), "2".into()));
        assert!(env.set("NEW".into(), "x".into()));
        assert!(env.get("NEW").unwrap().enabled);
    }

    #[test]
    fn set_enabled_reports_only_real_changes() {
        let mut env = EnvVars::from_iter([pair("A", "1")]);
        assert!(!env.set_enabled("A", true));
        assert!(env.set_enabled("A", false));
        assert!(!env.set_enabled("MISSING", false));
    }

    #[test]
    fn replace_active_keeps_disabled_entries_in_place() {
        let mut env = EnvVars::from_entries([
            EnvEntry::enabled("A", "1"),
            disabled("OFF", "secret"),
            EnvEntry::enabled("B", "2"),
        ]);
        env.replace_active(vec![pair("B", "20"), pair("C", "3")]);
        assert_eq!(
            env.entries(),
            &[
                disabled("OFF", "secret"),
                EnvEntry::enabled("B", "20"),
                EnvEntry::enabled("C", "3"),
            ]
        );
    }

    #[test]
    fn replace_active_never_overwrites_a_disabled_entry() {
        // 회귀 대상: 키 입력마다 호출되므로, 중간 상태가 비활성 항목을 건드리면 값이 사라진다.
        let mut env = EnvVars::from_entries([disabled("A", "keep")]);
        env.replace_active(vec![pair("A", "1")]);
        env.replace_active(vec![pair("AB", "1")]);
        assert_eq!(
            env.entries(),
            &[disabled("A", "keep"), EnvEntry::enabled("AB", "1")]
        );
        assert!(env.collides_with_disabled(&[pair("A", "x")]));
        assert!(!env.collides_with_disabled(&[pair("AB", "x")]));
    }
}
