use crate::ansi::TextSegment;
use std::collections::VecDeque;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

/// 세션의 현재 상태 (배지 표시용).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatusKind {
    /// 실행 중
    Running,
    /// 정상 종료 (exit 0)
    Succeeded,
    /// 비정상 종료 (exit code != 0)
    Failed(i32),
    /// 사용자가 중지 (종료 코드 없음)
    Stopped,
}

/// 터미널 출력 버퍼의 최대 라인 수 (보관 상한). 이 제한을 초과하면 오래된 라인이 FIFO로
/// 영구 제거된다. 터미널 뷰는 가상화로 이 버퍼 전체를 스크롤해서 볼 수 있다(가시 영역만 렌더).
const MAX_OUTPUT_LINES: usize = 50_000;

/// 세션당 출력 버퍼의 최대 누적 바이트 (보관 상한). 줄 수 상한만으로는 메모리가 묶이지
/// 않으므로(병적으로 긴 줄들), 이 바이트 예산을 함께 적용해 폭주하는 로그 생산자가 앱을
/// OOM으로 죽이지 못하게 한다. 줄 수/바이트 중 하나라도 넘으면 앞에서 제거한다.
const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

/// 단일 줄의 최대 저장 바이트. 개행 없는 초대형 줄 하나가 바이트 예산·폭 계산을 한 번에
/// 폭증시키지 못하도록, 이 크기를 넘는 줄은 char 경계에서 잘라 마커를 붙여 저장한다.
/// (executor의 1 MiB per-line read 상한보다 작은 표시/저장 상한.)
const MAX_LINE_BYTES: usize = 64 * 1024;

/// 잘린 줄 끝에 붙는 마커.
const TRUNCATED_MARKER: &str = "…[truncated]";

/// 세그먼트들의 텍스트 바이트 길이 합 (바이트 예산 계산용).
fn segments_bytes(segments: &[TextSegment]) -> usize {
    segments.iter().map(|s| s.text.len()).sum()
}

/// 세션 출력 검색/필터 상태. 검색바가 열려 있을 때만 `Some`.
/// 매칭은 라인 단위이며 기본은 대소문자 무시 부분일치, `regex`면 대소문자 무시
/// 정규식. 매치 인덱스(`matches`)는 출력/검색어 변경 시 `refresh_search_matches`로
/// 갱신해 캐시한다(뷰는 매 프레임 재스캔 대신 캐시만 읽음).
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    /// 검색어
    pub query: String,
    /// 매치 라인만 표시(필터 모드)
    pub filter: bool,
    /// 현재 매치 순번 (매치 목록 기준 0-based; 매치가 있을 때만 의미)
    pub current: usize,
    /// 정규식 모드 (off면 대소문자 무시 부분일치)
    pub regex: bool,
    /// 매치 라인의 `output_lines` 위치 인덱스 캐시. 매 프레임 재스캔을 피하려고
    /// 검색어/출력이 바뀔 때만 `refresh_search_matches`로 갱신한다(뷰는 읽기만).
    pub matches: Vec<usize>,
}

/// 실행 세션을 나타내는 구조체
/// 프로세스 실행 상태와 출력을 추적
#[derive(Clone)]
pub struct RunSession {
    /// 고유 세션 식별자
    pub id: Uuid,
    /// 실행 중인 구성의 이름
    pub config_name: String,
    /// 세션 시작 시간
    pub started_at: SystemTime,
    /// 프로세스의 표준 출력/에러 라인 목록 (ID와 함께 저장하여 키 기반 렌더링)
    /// 각 라인은 색상 정보가 포함된 텍스트 세그먼트 벡터로 저장됨
    pub output_lines: VecDeque<(usize, Vec<TextSegment>)>,
    /// 다음 라인에 할당할 ID (증가만 하여 제거되어도 키 안정성 보장)
    next_line_id: usize,
    /// 출력 콘텐츠 변경 카운터. 줄 추가/초기화 시 증가하며, 터미널 뷰의 가상화 wrap
    /// 캐시(per-line 래핑 행 수)를 언제 재생성할지 판단하는 키로 쓰인다. 콘텐츠가
    /// 안 바뀐 프레임에서는 값이 그대로라 캐시를 재사용한다.
    pub content_version: u64,
    /// 현재 보관 중인 출력의 누적 바이트(세그먼트 텍스트 길이 합). 바이트 예산 eviction용으로
    /// 추가/제거와 lockstep 유지한다.
    total_bytes: usize,
    /// 프로세스 실행 여부
    pub is_running: bool,
    /// 프로세스 종료 코드 (종료되지 않았으면 None)
    pub exit_code: Option<i32>,
    /// 프로세스 종료 시각 (실행 중이면 None) — 소요 시간 계산용
    pub finished_at: Option<SystemTime>,
    /// 프로세스 취소를 위한 플래그 (멀티스레드 안전)
    pub cancel_flag: Arc<AtomicBool>,
    /// 스크롤 위치 (0.0 = 맨 위, 1.0 = 맨 아래)
    pub scroll_progress: f32,
    /// 자동 스크롤 활성화 여부 (새 출력 시 자동으로 맨 아래로 스크롤)
    pub auto_scroll: bool,
    /// 실행 중인 프로세스의 PID
    /// 앱 종료 시 OS 레벨에서 직접 프로세스를 kill하기 위해 추적
    pub process_pid: Option<u32>,
    /// 출력 검색/필터 상태 (검색바가 열려 있으면 `Some`)
    pub search: Option<SearchState>,
    /// 검색 점프 1회성 목표 (논리줄 인덱스). 터미널 뷰가 이 값을 읽어 wrapped offset으로
    /// 변환해 스크롤한 뒤, `SessionScrollChanged` 핸들러에서 `None`으로 클리어한다.
    /// (app→terminal 역방향 스크롤 명령 경로 — 비율 기반으론 매치로 점프가 안 됐다)
    pub scroll_target: Option<usize>,
}

impl std::fmt::Debug for RunSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunSession")
            .field("id", &self.id)
            .field("config_name", &self.config_name)
            .field("started_at", &self.started_at)
            .field("output_lines_count", &self.output_lines.len())
            .field("next_line_id", &self.next_line_id)
            .field("is_running", &self.is_running)
            .field("exit_code", &self.exit_code)
            .field("finished_at", &self.finished_at)
            .field("cancel_flag", &"<AtomicBool>")
            .field("scroll_progress", &self.scroll_progress)
            .field("auto_scroll", &self.auto_scroll)
            .field("process_pid", &self.process_pid)
            .finish_non_exhaustive()
    }
}

impl RunSession {
    /// 새로운 실행 세션을 생성
    ///
    /// # Arguments
    /// * `config_name` - 실행할 구성의 이름
    pub fn new(config_name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            config_name,
            started_at: SystemTime::now(),
            output_lines: VecDeque::new(),
            next_line_id: 0,
            content_version: 0,
            total_bytes: 0,
            is_running: true,
            exit_code: None,
            finished_at: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            scroll_progress: 1.0, // 기본값: 맨 아래
            auto_scroll: true,    // 기본값: 자동 스크롤 활성화
            process_pid: None,
            search: None,
            scroll_target: None,
        }
    }

    /// 검색 매치 캐시(`search.matches`)를 현재 출력/검색어로 갱신하고 `current`를
    /// 범위 내로 클램프한다. 출력 추가/검색어 변경 등 상태 변화 시 `update()`에서
    /// 호출한다. 검색바가 닫혀 있으면(`search` None) no-op.
    pub fn refresh_search_matches(&mut self) {
        let Some((query, regex)) = self.search.as_ref().map(|s| (s.query.clone(), s.regex)) else {
            return;
        };
        let matches = self.search_match_indices(&query, regex);
        if let Some(search) = self.search.as_mut() {
            let len = matches.len();
            search.matches = matches;
            search.current = if len == 0 {
                0
            } else {
                search.current.min(len - 1)
            };
        }
    }

    /// 검색어에 매치하는 `output_lines`의 위치 인덱스 목록. `regex`면 대소문자 무시
    /// 정규식(잘못된 패턴은 매치 없음으로 처리), 아니면 대소문자 무시 부분일치.
    /// 빈 검색어면 빈 목록. FIFO 제거로 인덱스가 변할 수 있어 매번 즉석 계산한다.
    pub fn search_match_indices(&self, query: &str, regex: bool) -> Vec<usize> {
        if query.is_empty() {
            return Vec::new();
        }

        let line_text = |segments: &[TextSegment]| -> String {
            segments.iter().map(|seg| seg.text.as_str()).collect()
        };

        if regex {
            // 잘못된 패턴은 빈 결과(패닉/크래시 방지). 컴파일은 refresh 시점에만 일어난다.
            let Ok(re) = regex::RegexBuilder::new(query)
                .case_insensitive(true)
                .build()
            else {
                return Vec::new();
            };
            self.output_lines
                .iter()
                .enumerate()
                .filter(|(_, (_, segments))| re.is_match(&line_text(segments)))
                .map(|(idx, _)| idx)
                .collect()
        } else {
            let needle = query.to_lowercase();
            self.output_lines
                .iter()
                .enumerate()
                .filter(|(_, (_, segments))| line_text(segments).to_lowercase().contains(&needle))
                .map(|(idx, _)| idx)
                .collect()
        }
    }

    /// 출력 라인을 추가하고 필요시 오래된 라인 제거
    ///
    /// # Arguments
    /// * `line` - 추가할 출력 라인 (\n이 포함된 경우 여러 줄로 분리됨)
    pub fn add_output_line(&mut self, line: &str) {
        use crate::ansi::parse_ansi_text;

        // \n으로 분리하여 각 줄을 별도로 추가
        for single_line in line.split('\n') {
            // 초대형 단일 줄은 char 경계에서 잘라 저장(바이트 예산·폭 계산 폭증 방지).
            let truncated;
            let stored: &str = if single_line.len() > MAX_LINE_BYTES {
                let mut end = MAX_LINE_BYTES;
                while end > 0 && !single_line.is_char_boundary(end) {
                    end -= 1;
                }
                truncated = format!("{}{TRUNCATED_MARKER}", &single_line[..end]);
                &truncated
            } else {
                single_line
            };

            // ANSI 색상 코드를 파싱하여 텍스트 세그먼트로 변환
            let segments = parse_ansi_text(stored);
            let bytes = segments_bytes(&segments);

            // 고유 ID와 함께 새 라인 추가
            let line_id = self.next_line_id;
            self.next_line_id += 1;
            self.output_lines.push_back((line_id, segments));
            self.total_bytes += bytes;

            // 줄 수와 바이트 예산을 모두 만족할 때까지 앞에서 제거(FIFO, O(1) per pop).
            // 방금 추가한 줄 하나만 남을 때까지는 비우지 않는다(최소 1줄 유지).
            // worst-case: 단일 줄이 예산을 넘으면 total_bytes가 MAX_OUTPUT_BYTES + 마지막 줄
            // 크기(≤ MAX_LINE_BYTES + 마커)까지 일시 초과한다 — 버퍼를 완전히 비우는 것보다
            // 1줄 유지가 낫다는 의도적 트레이드오프이며, 메모리는 여전히 상수로 묶인다.
            while self.output_lines.len() > MAX_OUTPUT_LINES
                || (self.total_bytes > MAX_OUTPUT_BYTES && self.output_lines.len() > 1)
            {
                if let Some((_, evicted)) = self.output_lines.pop_front() {
                    self.total_bytes = self.total_bytes.saturating_sub(segments_bytes(&evicted));
                } else {
                    break;
                }
            }
        }
        // 콘텐츠가 바뀌었으니 wrap 캐시 무효화 키를 올린다.
        self.content_version = self.content_version.wrapping_add(1);
    }

    /// 출력 버퍼 초기화
    pub fn clear_output(&mut self) {
        self.output_lines.clear();
        self.total_bytes = 0;
        self.content_version = self.content_version.wrapping_add(1);
    }

    /// 현재 세션 상태(배지용)를 판별.
    pub fn status_kind(&self) -> SessionStatusKind {
        if self.is_running {
            SessionStatusKind::Running
        } else {
            match self.exit_code {
                Some(0) => SessionStatusKind::Succeeded,
                Some(code) => SessionStatusKind::Failed(code),
                None => SessionStatusKind::Stopped,
            }
        }
    }

    /// 완료된 세션의 실행 소요 시간 (실행 중이거나 종료 시각이 없으면 None).
    pub fn run_duration(&self) -> Option<Duration> {
        self.finished_at
            .and_then(|finished| finished.duration_since(self.started_at).ok())
    }

    /// 상태 배지에 표시할 짧은 라벨 (예: "✓ 1.2s", "✕ exit 1", "Stopped", "Running").
    pub fn status_badge_label(&self) -> String {
        match self.status_kind() {
            SessionStatusKind::Running => String::from("Running"),
            SessionStatusKind::Succeeded => self.run_duration().map_or_else(
                || String::from("✓ done"),
                |d| format!("✓ {}", format_duration(d)),
            ),
            SessionStatusKind::Failed(code) => format!("✕ exit {code}"),
            SessionStatusKind::Stopped => String::from("Stopped"),
        }
    }
}

/// `Duration`을 짧은 사람용 문자열로 변환 ("820ms" / "1.2s" / "3m 04s").
pub fn format_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1000 {
        format!("{millis}ms")
    } else if duration.as_secs() < 60 {
        format!("{:.1}s", duration.as_secs_f64())
    } else {
        let secs = duration.as_secs();
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_output() {
        let mut session = RunSession::new("test".to_string());

        session.add_output_line("Line 1");
        session.add_output_line("Line 2");
        session.add_output_line("Line 3");

        assert_eq!(session.output_lines.len(), 3);
        assert_eq!(session.output_lines[0].0, 0);
        assert_eq!(session.output_lines[1].0, 1);
        assert_eq!(session.output_lines[2].0, 2);

        // 세그먼트 텍스트 검증
        let line1_text: String = session.output_lines[0]
            .1
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        let line2_text: String = session.output_lines[1]
            .1
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        let line3_text: String = session.output_lines[2]
            .1
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(line1_text, "Line 1");
        assert_eq!(line2_text, "Line 2");
        assert_eq!(line3_text, "Line 3");
    }

    #[test]
    fn test_max_lines_limit() {
        let mut session = RunSession::new("test".to_string());

        // MAX_OUTPUT_LINES를 초과하는 라인 추가
        for i in 0..MAX_OUTPUT_LINES + 100 {
            session.add_output_line(&format!("Line {i}"));
        }

        // 라인 수가 MAX_OUTPUT_LINES로 제한되었는지 확인
        assert_eq!(session.output_lines.len(), MAX_OUTPUT_LINES);

        // 첫 100개 라인이 제거되었는지 확인 (ID 0~99)
        let has_line_0 = session.output_lines.iter().any(|(_, segs)| {
            let text: String = segs.iter().map(|s| s.text.as_str()).collect();
            text == "Line 0"
        });
        assert!(!has_line_0);

        let has_line_early = session.output_lines.iter().any(|(_, segs)| {
            let text: String = segs.iter().map(|s| s.text.as_str()).collect();
            text == "Line 50"
        });
        assert!(!has_line_early);

        let has_line_99 = session.output_lines.iter().any(|(_, segs)| {
            let text: String = segs.iter().map(|s| s.text.as_str()).collect();
            text == "Line 99"
        });
        assert!(!has_line_99);

        // 100번째 라인부터는 남아있어야 함 (ID 100부터)
        let has_line_100 = session.output_lines.iter().any(|(_, segs)| {
            let text: String = segs.iter().map(|s| s.text.as_str()).collect();
            text == "Line 100"
        });
        assert!(has_line_100);

        // 마지막 라인도 남아있어야 함
        let last_line = format!("Line {}", MAX_OUTPUT_LINES + 99);
        let has_last_line = session.output_lines.iter().any(|(_, segs)| {
            let text: String = segs.iter().map(|s| s.text.as_str()).collect();
            text == last_line
        });
        assert!(has_last_line);

        // 첫 번째와 마지막 라인 확인 - ID는 연속적
        assert_eq!(session.output_lines[0].0, 100); // ID는 100부터
        let first_line_text: String = session.output_lines[0]
            .1
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(first_line_text, "Line 100");
        assert_eq!(
            session.output_lines[MAX_OUTPUT_LINES - 1].0,
            MAX_OUTPUT_LINES + 99
        ); // ID는 2099
        let last_line_text: String = session.output_lines[MAX_OUTPUT_LINES - 1]
            .1
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(last_line_text, last_line);
    }

    #[test]
    fn test_clear_output() {
        let mut session = RunSession::new("test".to_string());

        session.add_output_line("Line 1");
        session.add_output_line("Line 2");

        session.clear_output();

        assert_eq!(session.output_lines.len(), 0);
        assert!(session.output_lines.is_empty());
        assert_eq!(session.total_bytes, 0, "clear는 바이트 카운터도 0으로 되돌려야 함");
    }

    #[test]
    fn total_bytes_tracks_current_lines() {
        let mut session = RunSession::new("x".to_string());
        session.add_output_line("hello");
        session.add_output_line("world!");
        // 누적 바이트는 현재 보관 중인 줄들의 세그먼트 바이트 합과 정확히 일치해야 한다.
        let sum: usize = session
            .output_lines
            .iter()
            .map(|(_, segs)| segs.iter().map(|s| s.text.len()).sum::<usize>())
            .sum();
        assert_eq!(session.total_bytes, sum);
    }

    #[test]
    fn long_line_is_truncated_with_marker() {
        let mut session = RunSession::new("x".to_string());
        let huge = "a".repeat(200 * 1024); // 200KB > MAX_LINE_BYTES(64KB)
        session.add_output_line(&huge);

        let stored: String = session
            .output_lines
            .back()
            .unwrap()
            .1
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert!(
            stored.ends_with(TRUNCATED_MARKER),
            "초대형 줄은 truncation 마커로 끝나야 함"
        );
        assert!(
            stored.len() <= MAX_LINE_BYTES + TRUNCATED_MARKER.len(),
            "저장 길이가 상한 + 마커 이내여야 함 (got {})",
            stored.len()
        );
    }

    #[test]
    fn byte_budget_evicts_before_line_cap() {
        let mut session = RunSession::new("x".to_string());
        // 각 줄 64KB. 줄 수 상한(50000)엔 한참 못 미치지만 바이트 예산(16MiB)이 먼저 차서
        // 오래된 줄이 제거되어야 한다.
        let big = "x".repeat(64 * 1024);
        for _ in 0..400 {
            session.add_output_line(&big);
        }
        assert!(
            session.total_bytes <= MAX_OUTPUT_BYTES,
            "총 바이트가 예산 이하로 유지되어야 함 (got {})",
            session.total_bytes
        );
        assert!(
            session.output_lines.len() < 400,
            "바이트 예산으로 오래된 줄이 제거되어야 함 (len={})",
            session.output_lines.len()
        );
        assert!(
            !session.output_lines.is_empty(),
            "최소 1줄은 유지되어야 함"
        );
    }

    #[test]
    fn status_kind_reflects_state() {
        let mut session = RunSession::new("x".to_string());
        assert_eq!(session.status_kind(), SessionStatusKind::Running);

        session.is_running = false;
        session.exit_code = Some(0);
        assert_eq!(session.status_kind(), SessionStatusKind::Succeeded);

        session.exit_code = Some(2);
        assert_eq!(session.status_kind(), SessionStatusKind::Failed(2));

        session.exit_code = None; // 사용자 중지
        assert_eq!(session.status_kind(), SessionStatusKind::Stopped);
    }

    #[test]
    fn search_match_indices_is_case_insensitive_substring() {
        let mut session = RunSession::new("x".to_string());
        session.add_output_line("Starting build");
        session.add_output_line("ERROR: boom");
        session.add_output_line("warning: minor");
        session.add_output_line("error again");

        assert_eq!(session.search_match_indices("", false), Vec::<usize>::new());
        assert_eq!(session.search_match_indices("error", false), vec![1, 3]);
        assert_eq!(session.search_match_indices("WARN", false), vec![2]);
        assert_eq!(
            session.search_match_indices("zzz", false),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn search_match_indices_supports_regex_case_insensitive() {
        let mut session = RunSession::new("x".to_string());
        session.add_output_line("error 12");
        session.add_output_line("warn 3");
        session.add_output_line("ERROR 999");

        // 대소문자 무시 정규식: "err...<공백><숫자>"
        assert_eq!(
            session.search_match_indices(r"err\w* \d+", true),
            vec![0, 2]
        );
        // 잘못된 패턴은 매치 없음 (패닉 없음)
        assert_eq!(session.search_match_indices("[", true), Vec::<usize>::new());
    }

    #[test]
    fn format_duration_is_human_readable() {
        assert_eq!(format_duration(Duration::from_millis(820)), "820ms");
        assert_eq!(format_duration(Duration::from_millis(1200)), "1.2s");
        assert_eq!(format_duration(Duration::from_secs(75)), "1m 15s");
    }
}
