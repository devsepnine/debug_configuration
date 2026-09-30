//! 구성별로 감지한 후보(모듈·main class·JAR) 캐시.
//!
//! 후보의 표기는 빌드 도구·작업 디렉터리·구성 타입에 묶여 있어 그 값이 바뀌면 함께 버려야 한다.
//! 종류마다 따로 필드를 두면 삭제·복제·전체 로드·MCP 변경 같은 생명주기 지점마다 종류별 호출이
//! 늘어 하나를 빠뜨리기 쉬우므로, 구성 id 기준으로 모든 종류를 한꺼번에 다루는 헬퍼로 묶는다.

use std::collections::HashMap;
use uuid::Uuid;

/// 감지 후보의 종류. 같은 구성이라도 종류마다 목록이 독립이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CandidateKind {
    /// Spring Boot 실행 모듈
    SpringModule,
    /// Kotlin main class
    KotlinMainClass,
    /// Kotlin JAR 경로
    KotlinJar,
}

#[derive(Default)]
pub struct DetectedCandidates {
    by_config: HashMap<(Uuid, CandidateKind), Vec<String>>,
}

impl DetectedCandidates {
    /// 후보 목록. 아직 감지하지 않았으면 빈 목록이다.
    pub fn get(&self, config_id: Uuid, kind: CandidateKind) -> &[String] {
        self.by_config
            .get(&(config_id, kind))
            .map_or(&[][..], Vec::as_slice)
    }

    /// 테스트에서 "감지된 적이 있는지"를 확인하는 용도.
    #[cfg(test)]
    pub fn contains(&self, config_id: Uuid, kind: CandidateKind) -> bool {
        self.by_config.contains_key(&(config_id, kind))
    }

    pub fn set(&mut self, config_id: Uuid, kind: CandidateKind, items: Vec<String>) {
        self.by_config.insert((config_id, kind), items);
    }

    /// 구성의 모든 종류 후보를 버린다.
    pub fn forget(&mut self, config_id: Uuid) {
        self.by_config.retain(|(id, _), _| *id != config_id);
    }

    /// 복제한 구성이 원본의 후보를 그대로 물려받게 한다.
    pub fn copy(&mut self, from: Uuid, to: Uuid) {
        let copied: Vec<_> = self
            .by_config
            .iter()
            .filter(|((id, _), _)| *id == from)
            .map(|((_, kind), items)| ((to, *kind), items.clone()))
            .collect();
        self.by_config.extend(copied);
    }

    pub fn clear(&mut self) {
        self.by_config.clear();
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.by_config.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_string()).collect()
    }

    #[test]
    fn kinds_of_one_configuration_are_independent() {
        let id = Uuid::new_v4();
        let mut cache = DetectedCandidates::default();

        cache.set(id, CandidateKind::SpringModule, items(&[":app"]));

        assert_eq!(cache.get(id, CandidateKind::SpringModule), [":app"]);
        assert!(cache.get(id, CandidateKind::KotlinJar).is_empty());
        assert!(!cache.contains(id, CandidateKind::KotlinJar));
    }

    #[test]
    fn forget_drops_every_kind_of_only_that_configuration() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let mut cache = DetectedCandidates::default();
        cache.set(a, CandidateKind::SpringModule, items(&[":app"]));
        cache.set(a, CandidateKind::KotlinMainClass, items(&["MainKt"]));
        cache.set(b, CandidateKind::KotlinJar, items(&["build/libs/x.jar"]));

        cache.forget(a);

        assert!(!cache.contains(a, CandidateKind::SpringModule));
        assert!(!cache.contains(a, CandidateKind::KotlinMainClass));
        assert!(cache.contains(b, CandidateKind::KotlinJar));
    }

    #[test]
    fn copy_gives_the_clone_every_kind_without_touching_the_source() {
        let (from, to) = (Uuid::new_v4(), Uuid::new_v4());
        let mut cache = DetectedCandidates::default();
        cache.set(from, CandidateKind::SpringModule, items(&[":app"]));
        cache.set(from, CandidateKind::KotlinJar, items(&["a.jar"]));

        cache.copy(from, to);

        assert_eq!(cache.get(to, CandidateKind::SpringModule), [":app"]);
        assert_eq!(cache.get(to, CandidateKind::KotlinJar), ["a.jar"]);
        assert_eq!(cache.get(from, CandidateKind::SpringModule), [":app"]);
    }

    #[test]
    fn clear_empties_the_cache() {
        let mut cache = DetectedCandidates::default();
        cache.set(Uuid::new_v4(), CandidateKind::SpringModule, items(&["x"]));

        cache.clear();

        assert!(cache.is_empty());
    }
}
