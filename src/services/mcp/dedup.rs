//! 변경성 툴의 중복 요청 흡수.
//!
//! HTTP 클라이언트는 응답을 받기 전에 연결이 끊기면 같은 POST를 재시도할 수 있다. 읽기 툴에서
//! 재시도는 무해하지만 실행 툴에서는 프로세스가 두 번 뜨고, 편집 툴에서는 구성이 두 번 생긴다.
//! 호출자가 `request_id`를 실어 보내면 창 안에서 재도착한 요청을 첫 호출의 결과로 되돌려,
//! 두 번째 부작용을 만들지 않는다.
//!
//! `request_id`가 없는 호출은 이 기록을 거치지 않는다 — 툴 설명이 그 경우의 재시도 위험을
//! 명시하고, 판단을 호출자에게 남긴다.

use std::collections::VecDeque;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// 같은 `request_id`를 같은 요청으로 인정하는 시간 창. 클라이언트 재시도는 초 단위에서
/// 일어나므로 1분이면 넉넉하고, 그보다 길면 사용자가 의도적으로 다시 돌리려는 호출까지 삼킨다.
pub const DEDUP_WINDOW: Duration = Duration::from_secs(60);

/// 보관할 최대 항목 수. 창이 만료되기 전에도 상한을 두는 것은, 매번 다른 `request_id`로
/// 호출을 쏟아붓는 클라이언트가 앱 메모리를 무제한 늘리지 못하게 하기 위한 것이다.
///
/// 상한을 지키는 것은 `lookup`의 거절이다. 축출로 자리를 만들면 아직 유효한 기록이 사라져,
/// 그 `request_id`의 재시도가 조용히 두 번 실행된다 — 멱등성이 부하 상태에서 슬그머니
/// best-effort가 되는 대신, 가득 찼을 때는 명시적으로 거절한다.
///
/// 그 거절은 창이 비기까지 지속되므로, 상한은 **정상 사용이 만들어 내는 속도를 넘어야
/// 한다**. `send_session_input`의 대상에는 줄 지문이 접혀 있어(`input_target`) 멱등성을
/// 요구하는 호출자는 줄마다 새 `request_id`를 쓰는 것이 정해진 사용법이고, 대화형 프로그램에
/// 줄 단위로 답하는 것은 병리적 사용이 아니다 — 한 줄이 항목 하나를 창 내내 점유한다. 상한이
/// 창의 초 수(60)와 같은 자리수면 초당 한 줄에서 거절이 발화한다. 항목은 `request_id`(≤128자)
/// ·대상 문자열·`Instant`·`Vec<Uuid>`이므로 이 상한에서도 메모리는 세션 출력 버퍼
/// (`max_output_lines`)에 비해 무시할 크기다.
pub const MAX_ENTRIES: usize = 512;

/// 중복 흡수 창을 공유하는 변경성 툴. 툴이 다르면 같은 `request_id`도 다른 요청이다 —
/// 한 창에 섞으면 `run_configuration`이 만든 세션이 `rerun_session` 재시도의 답으로
/// 되돌아간다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupScope {
    RunConfiguration,
    RerunSession,
    SendSessionInput,
    CreateConfiguration,
    UpdateConfiguration,
    DeleteConfiguration,
}

/// 창 조회 결과.
///
/// "있음/없음" 두 갈래가 아닌 것은, 같은 `request_id`가 다른 대상으로 오는 경우를 어느 한쪽으로
/// 접으면 둘 다 틀리기 때문이다 — 첫 결과를 돌려주면 호출자가 요청하지 않은 대상의 세션을 받고,
/// 새로 실행하면 `request_id`가 약속한 멱등성이 깨진다. 그 경우는 호출자에게 알려야 한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DedupLookup {
    /// 창에 같은 요청이 없다 — 그대로 실행한다.
    Fresh,
    /// 창 안의 같은 요청. 첫 호출이 만들어 낸 id들(실행 툴은 세션, 편집 툴은 구성).
    Duplicate(Vec<Uuid>),
    /// 같은 `request_id`가 다른 대상으로 왔다.
    TargetMismatch { recorded_target: String },
    /// 아직 유효한 기록만으로 창이 가득 찼다 — 이 요청은 실행하지 않고 거절한다.
    WindowFull,
}

/// 최근 변경 요청의 `request_id` → 그 요청이 만들어 낸 id 기록.
#[derive(Debug, Default)]
pub struct RequestLog {
    /// 기록 순서대로 쌓이므로 앞쪽이 항상 가장 오래됐다 — 만료 정리가 앞에서 끝난다.
    entries: VecDeque<Entry>,
}

#[derive(Debug)]
struct Entry {
    scope: DedupScope,
    request_id: String,
    /// 이 `request_id`가 가리켰던 대상(구성 식별자 또는 세션 id). 기록은 대상이 해석된
    /// **뒤에만** 남으므로, `MAX_REQUEST_ID_CHARS`가 막으려는 것과 같은 메모리 붙잡기가
    /// 이 필드로 되돌아오지 않는다 — 해석되지 않는 긴 문자열은 애초에 기록에 닿지 못한다.
    target: String,
    recorded_at: Instant,
    /// 이 요청이 만들어 낸 id들. 실행 툴은 세션 id, 편집 툴은 대상 구성 id를 남긴다 —
    /// 재시도가 첫 호출과 같은 답을 받게 하는 것이 목적이므로 종류는 scope가 정한다.
    produced: Vec<Uuid>,
}

impl RequestLog {
    /// 창 안에 같은 요청이 있는지 판정한다.
    pub fn lookup(
        &mut self,
        scope: DedupScope,
        request_id: &str,
        target: &str,
        now: Instant,
    ) -> DedupLookup {
        self.drop_expired(now);
        let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.scope == scope && entry.request_id == request_id)
        else {
            // 이미 아는 요청은 창이 가득 차 있어도 흡수한다 — 거절이 흡수보다 앞서면 이 창이
            // 존재하는 이유가 부하 상태에서 사라진다. 새 요청만 거절한다.
            if self.entries.len() >= MAX_ENTRIES {
                return DedupLookup::WindowFull;
            }
            return DedupLookup::Fresh;
        };
        if entry.target != target {
            return DedupLookup::TargetMismatch {
                recorded_target: entry.target.clone(),
            };
        }
        DedupLookup::Duplicate(entry.produced.clone())
    }

    /// 방금 수행한 요청을 기록한다. `lookup`이 `Fresh`를 준 요청만 이리 오므로, 여기서는
    /// 자리가 있다고 전제한다 — 상한의 근거는 `MAX_ENTRIES`의 문서에 있다.
    pub fn record(
        &mut self,
        scope: DedupScope,
        request_id: String,
        target: String,
        produced: Vec<Uuid>,
        now: Instant,
    ) {
        self.drop_expired(now);
        // 창이 만료된 뒤 같은 id가 다시 오면 옛 기록을 남겨 두지 않는다 — 두 항목이 공존하면
        // `lookup`이 어느 쪽을 먼저 찾을지가 저장 순서에 달린다.
        self.forget(scope, &request_id);
        self.entries.push_back(Entry {
            scope,
            request_id,
            target,
            recorded_at: now,
            produced,
        });
    }

    /// 기록을 지운다. 부작용이 **확실히** 일어나지 않았다고 판정된 실패에만 쓴다 — 남겨 두면
    /// 재시도가 "이미 했다"로 흡수되어, 아무것도 일어나지 않은 요청이 성공으로 굳는다.
    pub fn forget(&mut self, scope: DedupScope, request_id: &str) {
        self.entries
            .retain(|entry| entry.scope != scope || entry.request_id != request_id);
    }

    fn drop_expired(&mut self, now: Instant) {
        while let Some(front) = self.entries.front() {
            if now.saturating_duration_since(front.recorded_at) <= DEDUP_WINDOW {
                return;
            }
            self.entries.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: &str = "build";

    fn record(log: &mut RequestLog, request_id: &str, produced: Vec<Uuid>, now: Instant) {
        log.record(
            DedupScope::RunConfiguration,
            request_id.to_string(),
            TARGET.to_string(),
            produced,
            now,
        );
    }

    fn lookup(log: &mut RequestLog, request_id: &str, now: Instant) -> DedupLookup {
        log.lookup(DedupScope::RunConfiguration, request_id, TARGET, now)
    }

    #[test]
    fn an_unseen_request_id_is_not_a_duplicate() {
        let mut log = RequestLog::default();
        assert_eq!(
            lookup(&mut log, "req-1", Instant::now()),
            DedupLookup::Fresh
        );
    }

    #[test]
    fn the_same_request_id_returns_the_first_sessions() {
        let mut log = RequestLog::default();
        let now = Instant::now();
        let session = Uuid::new_v4();

        record(&mut log, "req-1", vec![session], now);

        assert_eq!(
            lookup(&mut log, "req-1", now),
            DedupLookup::Duplicate(vec![session])
        );
    }

    #[test]
    fn a_different_tool_does_not_share_the_window() {
        // 같은 request_id를 두 툴에 쓰는 클라이언트가 있어도 서로의 답을 받아선 안 된다.
        let mut log = RequestLog::default();
        let now = Instant::now();
        record(&mut log, "req-1", vec![Uuid::new_v4()], now);

        assert_eq!(
            log.lookup(DedupScope::RerunSession, "req-1", TARGET, now),
            DedupLookup::Fresh,
            "rerun must not receive the run's session"
        );
    }

    #[test]
    fn a_reused_request_id_with_another_target_is_a_conflict() {
        // 첫 결과를 돌려주면 호출자가 요청하지 않은 대상의 세션을 받는다.
        let mut log = RequestLog::default();
        let now = Instant::now();
        record(&mut log, "req-1", vec![Uuid::new_v4()], now);

        assert_eq!(
            log.lookup(DedupScope::RunConfiguration, "req-1", "deploy", now),
            DedupLookup::TargetMismatch {
                recorded_target: TARGET.to_string(),
            }
        );
    }

    #[test]
    fn a_forgotten_request_id_runs_again() {
        // 부작용이 일어나지 않았다고 판정된 실패는 같은 id로 다시 시도할 수 있어야 한다.
        let mut log = RequestLog::default();
        let now = Instant::now();
        record(&mut log, "req-1", vec![Uuid::new_v4()], now);

        log.forget(DedupScope::RunConfiguration, "req-1");

        assert_eq!(lookup(&mut log, "req-1", now), DedupLookup::Fresh);
    }

    #[test]
    fn forgetting_leaves_other_entries() {
        let mut log = RequestLog::default();
        let now = Instant::now();
        let kept = Uuid::new_v4();
        record(&mut log, "req-1", vec![Uuid::new_v4()], now);
        record(&mut log, "req-2", vec![kept], now);

        log.forget(DedupScope::RunConfiguration, "req-1");

        assert_eq!(
            lookup(&mut log, "req-2", now),
            DedupLookup::Duplicate(vec![kept])
        );
    }

    #[test]
    fn the_window_expires() {
        let mut log = RequestLog::default();
        let now = Instant::now();
        record(&mut log, "req-1", vec![Uuid::new_v4()], now);

        let later = now + DEDUP_WINDOW + Duration::from_millis(1);
        assert_eq!(
            lookup(&mut log, "req-1", later),
            DedupLookup::Fresh,
            "past the window the caller means a new run"
        );
    }

    #[test]
    fn the_window_edge_is_still_a_duplicate() {
        let mut log = RequestLog::default();
        let now = Instant::now();
        record(&mut log, "req-1", vec![Uuid::new_v4()], now);

        assert!(matches!(
            lookup(&mut log, "req-1", now + DEDUP_WINDOW),
            DedupLookup::Duplicate(_)
        ));
    }

    #[test]
    fn a_window_full_of_valid_records_refuses_a_new_id() {
        // 자리를 비우려 축출하면 아직 유효한 기록이 사라지고, 그 id의 재시도가 조용히 두 번
        // 실행된다. 거절은 그 경로를 없앤다.
        let mut log = RequestLog::default();
        let now = Instant::now();
        for i in 0..MAX_ENTRIES {
            record(&mut log, &format!("req-{i}"), vec![Uuid::new_v4()], now);
        }

        assert_eq!(lookup(&mut log, "req-new", now), DedupLookup::WindowFull);
    }

    /// 대화형 응답의 지속 속도 가정. 측정값이 아니라 명시된 기준선이다 — 상한이 정상 사용에서
    /// 발화하지 않는지 재기 위한 것이므로, 실제 사용이 이보다 빠르다고 관측되면 이 값을 올린다.
    const SUSTAINED_LINES_PER_SECOND: usize = 4;

    #[test]
    fn the_cap_clears_a_windows_worth_of_line_by_line_input() {
        // `input_target`이 줄 지문을 대상에 접어 넣으므로 멱등성을 요구하는 호출자는 줄마다 새
        // request_id를 쓴다 — 한 줄이 항목 하나를 창 내내 점유한다. 상한이 이 속도에 못 미치면
        // 만석 거절이 병리적 상태가 아니라 정상 사용에서 발화한다.
        let mut log = RequestLog::default();
        let now = Instant::now();
        let lines = SUSTAINED_LINES_PER_SECOND * DEDUP_WINDOW.as_secs() as usize;

        for line in 0..lines {
            let request_id = format!("line-{line}");
            assert_eq!(
                lookup(&mut log, &request_id, now),
                DedupLookup::Fresh,
                "line {line} of one window's worth of input was refused"
            );
            record(&mut log, &request_id, vec![Uuid::new_v4()], now);
        }
    }

    #[test]
    fn a_full_window_still_answers_a_duplicate() {
        // 거절이 흡수보다 앞서면 이 창이 존재하는 이유가 부하 상태에서 사라진다.
        let mut log = RequestLog::default();
        let now = Instant::now();
        let first = Uuid::new_v4();
        record(&mut log, "req-0", vec![first], now);
        for i in 1..MAX_ENTRIES {
            record(&mut log, &format!("req-{i}"), vec![Uuid::new_v4()], now);
        }

        assert_eq!(
            lookup(&mut log, "req-0", now),
            DedupLookup::Duplicate(vec![first])
        );
    }

    #[test]
    fn expiry_frees_room_in_a_full_window() {
        let mut log = RequestLog::default();
        let now = Instant::now();
        for i in 0..MAX_ENTRIES {
            record(&mut log, &format!("req-{i}"), vec![Uuid::new_v4()], now);
        }

        let later = now + DEDUP_WINDOW + Duration::from_millis(1);
        assert_eq!(lookup(&mut log, "req-new", later), DedupLookup::Fresh);
    }

    #[test]
    fn the_log_stops_growing_at_its_cap() {
        // 상한을 지키는 것은 `lookup`의 거절이다 — `record`는 축출하지 않는다.
        let mut log = RequestLog::default();
        let now = Instant::now();
        for i in 0..MAX_ENTRIES * 2 {
            let request_id = format!("req-{i}");
            if lookup(&mut log, &request_id, now) == DedupLookup::Fresh {
                record(&mut log, &request_id, vec![Uuid::new_v4()], now);
            }
        }

        assert_eq!(log.entries.len(), MAX_ENTRIES);
    }

    #[test]
    fn re_recording_the_same_id_replaces_the_old_result() {
        let mut log = RequestLog::default();
        let now = Instant::now();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();

        record(&mut log, "req-1", vec![first], now);
        let later = now + DEDUP_WINDOW + Duration::from_secs(1);
        record(&mut log, "req-1", vec![second], later);

        assert_eq!(
            lookup(&mut log, "req-1", later),
            DedupLookup::Duplicate(vec![second])
        );
    }
}
