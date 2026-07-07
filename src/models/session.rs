use crate::ansi::TextSegment;
use std::collections::VecDeque;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

/// 실행 스트림이 앱으로 전달하는 출력 이벤트 하나.
///
/// `Replace`는 직전에 추가된 라인을 통째로 교체한다 — PTY 환경에서 진행바가
/// `\r`로 같은 줄을 되감아 재그리는 것을 라이브로 렌더하기 위한 시맨틱.
/// (pipes 경로는 `Line`만 방출한다.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputEvent {
    /// 새 라인 추가 (확정된 라인, 또는 새로 열린 라이브 라인)
    Line(String),
    /// 가장 최근에 추가된 라인의 내용 교체 (라이브 진행바 갱신)
    Replace(String),
}

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
pub const DEFAULT_MAX_OUTPUT_LINES: usize = 50_000;

/// 세션당 출력 버퍼의 최대 누적 바이트 (보관 상한). 줄 수 상한만으로는 메모리가 묶이지
/// 않으므로(병적으로 긴 줄들), 이 바이트 예산을 함께 적용해 폭주하는 로그 생산자가 앱을
/// OOM으로 죽이지 못하게 한다. 줄 수/바이트 중 하나라도 넘으면 앞에서 제거한다.
const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

/// 단일 줄의 최대 저장 바이트. 개행 없는 초대형 줄 하나가 바이트 예산·폭 계산을 한 번에
/// 폭증시키지 못하도록, 이 크기를 넘는 줄은 char 경계에서 잘라 마커를 붙여 저장한다.
/// (executor의 1 MiB per-line read 상한보다 작은 표시/저장 상한.)
const MAX_STORED_LINE_BYTES: usize = 64 * 1024;

/// 잘린 줄 끝에 붙는 마커.
const TRUNCATED_MARKER: &str = "…[truncated]";

/// 세그먼트들의 텍스트 바이트 길이 합 (바이트 예산 계산용).
fn segments_bytes(segments: &[TextSegment]) -> usize {
    segments.iter().map(|s| s.text.len()).sum()
}

/// 유니코드 소문자의 첫 char (대소문자 무시 비교용). 대부분 1:1이며, 한 글자가 여러 글자로
/// 소문자화되는 드문 경우(예: 'ß'→"ss")는 첫 char만 쓴다. needle도 같은 정규화를 거치므로
/// 매칭 결과·강조 위치가 이 근사를 따른다(실용상 ASCII·한글 등에서 정확).
fn lower_char(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// 라인 chars에서 `needle`(미리 소문자화된 char들)의 겹치지 않는 모든 출현을 char 범위로
/// 수집한다. char 인덱스 기준이라 wide char(한글 등)도 위치가 정확하다.
fn append_substring_matches(
    out: &mut Vec<SearchMatch>,
    line_idx: usize,
    chars: &[char],
    needle: &[char],
) {
    if needle.is_empty() || chars.len() < needle.len() {
        return;
    }
    let mut i = 0;
    while i + needle.len() <= chars.len() {
        let hit = chars[i..i + needle.len()]
            .iter()
            .zip(needle)
            .all(|(c, n)| lower_char(*c) == *n);
        if hit {
            out.push(SearchMatch {
                line_idx,
                start: i,
                end: i + needle.len(),
            });
            i += needle.len(); // 겹치는 매치는 세지 않는다
        } else {
            i += 1;
        }
    }
}

/// 한 검색 매치의 위치: 출력 라인 인덱스 + 라인 내 char 범위(`start..end`, end exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch {
    /// `output_lines` 내 라인 인덱스
    pub line_idx: usize,
    /// 라인 내 매치 시작 char 인덱스 (포함)
    pub start: usize,
    /// 라인 내 매치 끝 char 인덱스 (제외)
    pub end: usize,
}

/// 세션 출력 검색/필터 상태. 검색바가 열려 있을 때만 `Some`.
/// 기본은 대소문자 무시 부분일치, `regex`면 대소문자 무시 정규식. 매치 위치(`matches`)는
/// 출력/검색어 변경 시 `refresh_search_matches`로 갱신해 캐시한다(뷰는 캐시만 읽음).
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
    /// 매치 위치 캐시(라인 인덱스 + 라인 내 char 범위). 매 프레임 재스캔을 피하려고
    /// 검색어/출력이 바뀔 때만 `refresh_search_matches`로 갱신한다(뷰는 읽기만).
    pub matches: Vec<SearchMatch>,
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
    /// 검색 점프 1회성 목표 (**안정 line_id** — `output_lines`의 튜플 첫 요소).
    /// 뷰 빌드 시점에 현재 canvas 행으로 해석되므로, 스트리밍 중 앞쪽 줄이 evict돼도
    /// 목표가 어긋나지 않는다. 터미널 뷰가 wrapped offset으로 변환해 스크롤한 뒤
    /// `SessionScrollChanged` 핸들러에서 `None`으로 클리어한다.
    /// (app→terminal 역방향 스크롤 명령 경로 — 비율 기반으론 매치로 점프가 안 됐다)
    pub scroll_target: Option<usize>,
    /// 컨트롤 오버플로 메뉴(⋯) 열림 여부. pane이 좁아 전체 컨트롤 버튼이 들어가지
    /// 않을 때 `⋯` 버튼으로 펼치는 세로 액션 메뉴의 토글 상태(터미널 위에 표시).
    pub controls_menu_open: bool,
    /// 이 세션의 출력 버퍼 최대 라인 수. 생성 시 앱 설정값으로 고정된다. 초과 시
    /// 오래된 라인이 FIFO로 제거된다(전역 기본은 `DEFAULT_MAX_OUTPUT_LINES`).
    pub max_output_lines: usize,
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
            controls_menu_open: false,
            max_output_lines: DEFAULT_MAX_OUTPUT_LINES,
        }
    }

    /// 검색 매치 캐시(`search.matches`)를 현재 출력/검색어로 갱신하고 `current`를
    /// 범위 내로 클램프한다. 출력 추가/검색어 변경 등 상태 변화 시 `update()`에서
    /// 호출한다. 검색바가 닫혀 있으면(`search` None) no-op.
    pub fn refresh_search_matches(&mut self) {
        let Some((query, regex)) = self.search.as_ref().map(|s| (s.query.clone(), s.regex)) else {
            return;
        };
        let matches = self.search_matches(&query, regex);
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
    /// 모든 매치를 (라인, char 범위)로 계산한다. 한 라인에 매치가 여러 개면 각각 별도
    /// 항목으로 수집된다. 위치는 char 인덱스(디스플레이 폭 아님)이며, 뷰가 wrap·wide char를
    /// 고려해 픽셀로 변환한다.
    pub fn search_matches(&self, query: &str, regex: bool) -> Vec<SearchMatch> {
        if query.is_empty() {
            return Vec::new();
        }

        let line_text = |segments: &[TextSegment]| -> String {
            segments.iter().map(|seg| seg.text.as_str()).collect()
        };
        let mut out = Vec::new();

        if regex {
            // 잘못된 패턴은 빈 결과(패닉/크래시 방지). 컴파일은 refresh 시점에만 일어난다.
            let Ok(re) = regex::RegexBuilder::new(query)
                .case_insensitive(true)
                .build()
            else {
                return Vec::new();
            };
            for (line_idx, (_, segments)) in self.output_lines.iter().enumerate() {
                let text = line_text(segments);
                for m in re.find_iter(&text) {
                    if m.start() == m.end() {
                        continue; // 빈 매치는 강조 대상 아님
                    }
                    // 정규식은 byte offset을 주므로 char 인덱스로 변환한다.
                    let start = text[..m.start()].chars().count();
                    let end = text[..m.end()].chars().count();
                    out.push(SearchMatch {
                        line_idx,
                        start,
                        end,
                    });
                }
            }
        } else {
            let needle: Vec<char> = query.chars().map(lower_char).collect();
            for (line_idx, (_, segments)) in self.output_lines.iter().enumerate() {
                let chars: Vec<char> = line_text(segments).chars().collect();
                append_substring_matches(&mut out, line_idx, &chars, &needle);
            }
        }
        out
    }

    /// 출력 라인을 추가하고 필요시 오래된 라인 제거
    ///
    /// # Arguments
    /// * `line` - 추가할 출력 라인 (\n이 포함된 경우 여러 줄로 분리됨)
    pub fn add_output_line(&mut self, line: &str) {
        // \n으로 분리하여 각 줄을 별도로 추가
        for single_line in line.split('\n') {
            let segments = Self::normalize_line(single_line);
            let bytes = segments_bytes(&segments);

            // 고유 ID와 함께 새 라인 추가
            let line_id = self.next_line_id;
            self.next_line_id += 1;
            self.output_lines.push_back((line_id, segments));
            self.total_bytes += bytes;
            self.evict_over_budget();
        }
        // 콘텐츠가 바뀌었으니 wrap 캐시 무효화 키를 올린다.
        self.content_version = self.content_version.wrapping_add(1);
    }

    /// 실행 스트림 이벤트 1건 적용 (`Line`=추가, `Replace`=마지막 라인 교체).
    pub fn apply_output_event(&mut self, event: &OutputEvent) {
        match event {
            OutputEvent::Line(text) => self.add_output_line(text),
            OutputEvent::Replace(text) => self.replace_last_line(text),
        }
    }

    /// 가장 최근에 추가된 라인의 내용을 교체한다 (라이브 진행바 갱신).
    /// line_id를 **재사용**해 keyed 렌더/wrap 캐시의 증분 판정("마지막 줄만 변경")이
    /// 성립하게 한다. 버퍼가 비어 있으면 추가로 폴백한다 (방어적).
    pub fn replace_last_line(&mut self, line: &str) {
        let Some((line_id, old_segments)) = self.output_lines.pop_back() else {
            self.add_output_line(line);
            return;
        };
        self.total_bytes = self
            .total_bytes
            .saturating_sub(segments_bytes(&old_segments));
        let segments = Self::normalize_line(line);
        self.total_bytes += segments_bytes(&segments);
        self.output_lines.push_back((line_id, segments));
        self.evict_over_budget();
        self.content_version = self.content_version.wrapping_add(1);
    }

    /// 단일 라인 정규화 공통 경로: CR 붕괴 → 크기 제한 → ANSI 파싱.
    /// add/replace 양쪽이 공유해 방어(특히 `\r` collapse)가 대칭이 되게 한다.
    fn normalize_line(single_line: &str) -> Vec<TextSegment> {
        use crate::ansi::parse_ansi_text;

        // CR 덮어쓰기 시맨틱: 진행바(cargo/npm/pip)는 "10%\r50%\r100%"처럼 같은 줄을
        // \r로 되감아 다시 그린다. 마지막 \r 이후 내용(=최종 상태)만 남긴다 — 전체 라인
        // 교체 근사(문자단위 col0 덮어쓰기 아님). LineAssembler가 \r를 소비하는 PTY
        // 경로에서도 방어선으로 유지한다. 끝의 bare \r은 "아직 아무것도 덮어쓰지 않음"
        // 이므로 먼저 제거한다 ("42%\r"가 빈 줄로 붕괴하는 것 방지).
        let single_line = single_line.trim_end_matches('\r');
        let single_line = single_line.rsplit('\r').next().unwrap_or(single_line);
        // 초대형 단일 줄은 char 경계에서 잘라 저장(바이트 예산·폭 계산 폭증 방지).
        let truncated;
        let stored: &str = if single_line.len() > MAX_STORED_LINE_BYTES {
            let mut end = MAX_STORED_LINE_BYTES;
            while end > 0 && !single_line.is_char_boundary(end) {
                end -= 1;
            }
            truncated = format!("{}{TRUNCATED_MARKER}", &single_line[..end]);
            &truncated
        } else {
            single_line
        };
        parse_ansi_text(stored)
    }

    /// 줄 수/바이트 예산 초과분을 앞에서 제거(FIFO, O(1) per pop).
    /// 마지막 1줄은 유지 — worst-case 초과 폭은 MAX_OUTPUT_BYTES + 줄 상한(의도적 트레이드오프).
    fn evict_over_budget(&mut self) {
        while self.output_lines.len() > self.max_output_lines
            || (self.total_bytes > MAX_OUTPUT_BYTES && self.output_lines.len() > 1)
        {
            if let Some((_, evicted)) = self.output_lines.pop_front() {
                self.total_bytes = self.total_bytes.saturating_sub(segments_bytes(&evicted));
            } else {
                break;
            }
        }
    }

    /// 출력 버퍼 초기화
    pub fn clear_output(&mut self) {
        self.output_lines.clear();
        self.total_bytes = 0;
        // 점프할 대상이 사라졌으므로 1회성 목표도 함께 버린다 — 남겨두면 빈 버퍼에선
        // 소비(clamp)가 게이트돼 영영 Some으로 남아 auto_scroll 판정을 계속 우회시킨다.
        self.scroll_target = None;
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

    /// 실행 시작 후 현재까지의 경과 시간 (라이브 표시용). 시스템 시계가 뒤로 간
    /// 경우(NTP 보정 등) 0으로 처리한다.
    pub fn elapsed_since_start(&self) -> Duration {
        SystemTime::now()
            .duration_since(self.started_at)
            .unwrap_or_default()
    }

    /// 사이드바(고정 폭 배지)용 압축 라벨. 실행 중엔 경과시간만("12.3s"), 종료 상태는
    /// 소요시간 없는 짧은 형태 — 전체 정보는 pane 타이틀의 `status_badge_label`이 담당.
    pub fn status_badge_label_compact(&self) -> String {
        match self.status_kind() {
            SessionStatusKind::Running => format_duration(self.elapsed_since_start()),
            SessionStatusKind::Succeeded => self.run_duration().map_or_else(
                || String::from("✓ done"),
                |d| format!("✓ {}", format_duration(d)),
            ),
            SessionStatusKind::Failed(code) => format!("✕ exit {code}"),
            SessionStatusKind::Stopped => String::from("Stopped"),
        }
    }

    /// 상태 배지에 표시할 라벨 (pane 타이틀 등 폭 여유가 있는 표면용).
    /// 실행 중엔 라이브 경과 시간을, 종료 후엔 결과와 소요 시간을 함께 표시한다
    /// (예: "Running 12.3s", "✓ 1.2s", "✕ exit 1 · 3.4s", "Stopped · 3.4s").
    pub fn status_badge_label(&self) -> String {
        match self.status_kind() {
            SessionStatusKind::Running => {
                format!("Running {}", format_duration(self.elapsed_since_start()))
            }
            SessionStatusKind::Succeeded => self.run_duration().map_or_else(
                || String::from("✓ done"),
                |d| format!("✓ {}", format_duration(d)),
            ),
            SessionStatusKind::Failed(code) => self.run_duration().map_or_else(
                || format!("✕ exit {code}"),
                |d| format!("✕ exit {code} · {}", format_duration(d)),
            ),
            SessionStatusKind::Stopped => self.run_duration().map_or_else(
                || String::from("Stopped"),
                |d| format!("Stopped · {}", format_duration(d)),
            ),
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
    fn custom_max_output_lines_caps_buffer() {
        // 세션별 max_output_lines를 작게 설정하면 그 한도로 evict된다(설정 주입 검증).
        let mut session = RunSession::new("test".to_string());
        session.max_output_lines = 3;
        for i in 0..10 {
            session.add_output_line(&format!("line {i}"));
        }
        assert_eq!(session.output_lines.len(), 3);
    }

    #[test]
    fn test_max_lines_limit() {
        let mut session = RunSession::new("test".to_string());

        // DEFAULT_MAX_OUTPUT_LINES를 초과하는 라인 추가
        for i in 0..DEFAULT_MAX_OUTPUT_LINES + 100 {
            session.add_output_line(&format!("Line {i}"));
        }

        // 라인 수가 DEFAULT_MAX_OUTPUT_LINES로 제한되었는지 확인
        assert_eq!(session.output_lines.len(), DEFAULT_MAX_OUTPUT_LINES);

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
        let last_line = format!("Line {}", DEFAULT_MAX_OUTPUT_LINES + 99);
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
            session.output_lines[DEFAULT_MAX_OUTPUT_LINES - 1].0,
            DEFAULT_MAX_OUTPUT_LINES + 99
        ); // ID는 2099
        let last_line_text: String = session.output_lines[DEFAULT_MAX_OUTPUT_LINES - 1]
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
        assert_eq!(
            session.total_bytes, 0,
            "clear는 바이트 카운터도 0으로 되돌려야 함"
        );
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
        let huge = "a".repeat(200 * 1024); // 200KB > MAX_STORED_LINE_BYTES(64KB)
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
            stored.len() <= MAX_STORED_LINE_BYTES + TRUNCATED_MARKER.len(),
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
        assert!(!session.output_lines.is_empty(), "최소 1줄은 유지되어야 함");
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

    /// 저장된 라인의 순수 텍스트 (세그먼트 join) — CR 붕괴 검증용.
    fn line_text_at(session: &RunSession, idx: usize) -> String {
        session.output_lines[idx]
            .1
            .iter()
            .map(|seg| seg.text.as_str())
            .collect()
    }

    #[test]
    fn replace_last_line_swaps_content_keeping_line_id() {
        let mut session = RunSession::new("x".to_string());
        session.add_output_line("building 10%");
        let (id_before, _) = session.output_lines[0].clone();
        let bytes_before = session.total_bytes;
        let version_before = session.content_version;

        session.replace_last_line("building 100% done");

        assert_eq!(session.output_lines.len(), 1);
        let (id_after, _) = &session.output_lines[0];
        assert_eq!(
            *id_after, id_before,
            "line_id must be reused (keyed render)"
        );
        assert_eq!(line_text_at(&session, 0), "building 100% done");
        assert_ne!(session.total_bytes, bytes_before);
        assert!(session.content_version > version_before);
    }

    #[test]
    fn replace_last_line_on_empty_appends() {
        let mut session = RunSession::new("x".to_string());
        session.replace_last_line("hello");
        assert_eq!(session.output_lines.len(), 1);
        assert_eq!(line_text_at(&session, 0), "hello");
    }

    #[test]
    fn replace_last_line_applies_cr_collapse_defense() {
        // 방어 대칭: Replace 텍스트에 \r가 섞여 와도 add와 동일하게 붕괴한다.
        let mut session = RunSession::new("x".to_string());
        session.add_output_line("start");
        session.replace_last_line("10%\r99%\r");
        assert_eq!(line_text_at(&session, 0), "99%");
    }

    #[test]
    fn apply_output_event_routes_line_and_replace() {
        use super::OutputEvent;
        let mut session = RunSession::new("x".to_string());
        session.apply_output_event(&OutputEvent::Line(String::from("a")));
        session.apply_output_event(&OutputEvent::Line(String::from("b")));
        session.apply_output_event(&OutputEvent::Replace(String::from("B")));
        assert_eq!(session.output_lines.len(), 2);
        assert_eq!(line_text_at(&session, 0), "a");
        assert_eq!(line_text_at(&session, 1), "B");
    }

    #[test]
    fn cr_collapses_progress_bar_to_final_state() {
        // cargo/npm 진행바 형태: 같은 줄을 \r로 되감아 재그림 → 최종 상태만 남는다.
        let mut session = RunSession::new("x".to_string());
        session.add_output_line("Downloading 10%\rDownloading 55%\rDownloading 100%");
        assert_eq!(session.output_lines.len(), 1);
        assert_eq!(line_text_at(&session, 0), "Downloading 100%");
    }

    #[test]
    fn cr_collapse_respects_newline_boundaries() {
        let mut session = RunSession::new("x".to_string());
        session.add_output_line("a\rb\nc");
        assert_eq!(session.output_lines.len(), 2);
        assert_eq!(line_text_at(&session, 0), "b");
        assert_eq!(line_text_at(&session, 1), "c");
    }

    #[test]
    fn trailing_bare_cr_preserves_last_content() {
        // 청크 경계/EOF가 \r 직후에 떨어진 경우 — 아직 덮어쓴 내용이 없으므로
        // 마지막 상태를 보존해야 한다 (빈 줄로 붕괴 금지).
        let mut session = RunSession::new("x".to_string());
        session.add_output_line("Downloading 42%\r");
        assert_eq!(line_text_at(&session, 0), "Downloading 42%");

        let mut session = RunSession::new("x".to_string());
        session.add_output_line("a\rb\r\r\r");
        assert_eq!(line_text_at(&session, 0), "b");
    }

    #[test]
    fn clear_output_drops_pending_scroll_target() {
        // Clear Log 시 1회성 점프 목표도 함께 버린다 — 남으면 빈 버퍼에서 소비되지
        // 못해 auto_scroll 판정을 계속 우회시킨다 (리뷰 회귀 테스트).
        let mut session = RunSession::new("x".to_string());
        session.add_output_line("hello");
        session.scroll_target = Some(0);
        session.clear_output();
        assert!(session.scroll_target.is_none());
    }

    #[test]
    fn cr_with_erase_sequence_keeps_clean_final_text() {
        // "\r\x1b[K" (줄 되감기 + 지우기) — 지우기 시퀀스는 ANSI 파서가 소비해
        // 최종 텍스트만 남는다.
        let mut session = RunSession::new("x".to_string());
        session.add_output_line("building 3/10\r\x1b[Kbuilding 10/10 done");
        assert_eq!(session.output_lines.len(), 1);
        assert_eq!(line_text_at(&session, 0), "building 10/10 done");
    }

    #[test]
    fn status_badge_includes_duration_for_all_terminal_states() {
        let mut session = RunSession::new("x".to_string());
        session.is_running = false;
        session.finished_at = Some(session.started_at + Duration::from_millis(3400));

        session.exit_code = Some(0);
        assert_eq!(session.status_badge_label(), "✓ 3.4s");
        session.exit_code = Some(1);
        assert_eq!(session.status_badge_label(), "✕ exit 1 · 3.4s");
        session.exit_code = None; // 사용자 중지
        assert_eq!(session.status_badge_label(), "Stopped · 3.4s");
    }

    #[test]
    fn compact_badge_stays_short_for_fixed_width_sidebar() {
        let mut session = RunSession::new("x".to_string());
        // 실행 중: 경과시간만 (접두사 없음 — 고정 폭 배지에 맞춤).
        assert!(!session.status_badge_label_compact().starts_with("Running"));

        session.is_running = false;
        session.finished_at = Some(session.started_at + Duration::from_millis(3400));
        session.exit_code = Some(130);
        // 종료 상태는 소요시간 없이 짧게 (풀 라벨은 pane 타이틀 담당).
        assert_eq!(session.status_badge_label_compact(), "✕ exit 130");
        session.exit_code = None;
        assert_eq!(session.status_badge_label_compact(), "Stopped");
        session.exit_code = Some(0);
        assert_eq!(session.status_badge_label_compact(), "✓ 3.4s");
    }

    #[test]
    fn status_badge_shows_live_elapsed_while_running() {
        let session = RunSession::new("x".to_string());
        let label = session.status_badge_label();
        // 경과 시간은 비결정적이므로 형식만 검증 ("Running <duration>").
        assert!(
            label.starts_with("Running ") && label.len() > "Running ".len(),
            "unexpected running label: {label}"
        );
    }

    #[test]
    fn status_badge_without_finish_time_omits_duration() {
        let mut session = RunSession::new("x".to_string());
        session.is_running = false;
        session.exit_code = Some(1);
        assert_eq!(session.status_badge_label(), "✕ exit 1");
        session.exit_code = None;
        assert_eq!(session.status_badge_label(), "Stopped");
    }

    #[test]
    fn search_matches_substring_returns_line_and_char_ranges() {
        let mut session = RunSession::new("x".to_string());
        session.add_output_line("Starting build");
        session.add_output_line("ERROR: boom"); // line 1
        session.add_output_line("warning: minor");
        session.add_output_line("error again"); // line 3

        assert!(session.search_matches("", false).is_empty());

        let lines = |q: &str| {
            session
                .search_matches(q, false)
                .iter()
                .map(|m| m.line_idx)
                .collect::<Vec<_>>()
        };
        assert_eq!(lines("error"), vec![1, 3]);
        assert_eq!(lines("WARN"), vec![2]);
        assert!(lines("zzz").is_empty());

        // 대소문자 무시 + char 범위: 두 라인 모두 "error"가 0..5
        let m = session.search_matches("error", false);
        assert_eq!((m[0].line_idx, m[0].start, m[0].end), (1, 0, 5));
        assert_eq!((m[1].line_idx, m[1].start, m[1].end), (3, 0, 5));
    }

    #[test]
    fn search_matches_finds_multiple_non_overlapping_per_line() {
        let mut session = RunSession::new("x".to_string());
        session.add_output_line("aXaXa"); // "a" at char 0, 2, 4

        let m = session.search_matches("a", false);
        assert_eq!(
            m.iter().map(|m| (m.start, m.end)).collect::<Vec<_>>(),
            vec![(0, 1), (2, 3), (4, 5)]
        );
    }

    #[test]
    fn search_matches_char_index_accounts_for_wide_chars() {
        let mut session = RunSession::new("x".to_string());
        session.add_output_line("가나error다"); // "error" begins at char index 2

        let m = session.search_matches("error", false);
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].start, m[0].end), (2, 7));
    }

    #[test]
    fn search_matches_supports_regex_case_insensitive() {
        let mut session = RunSession::new("x".to_string());
        session.add_output_line("error 12"); // line 0
        session.add_output_line("warn 3");
        session.add_output_line("ERROR 999"); // line 2

        // 대소문자 무시 정규식: "err...<공백><숫자>"
        let m = session.search_matches(r"err\w* \d+", true);
        assert_eq!(m.iter().map(|m| m.line_idx).collect::<Vec<_>>(), vec![0, 2]);
        assert_eq!((m[0].start, m[0].end), (0, 8)); // "error 12" 전체

        // 잘못된 패턴은 매치 없음 (패닉 없음)
        assert!(session.search_matches("[", true).is_empty());
    }

    #[test]
    fn format_duration_is_human_readable() {
        assert_eq!(format_duration(Duration::from_millis(820)), "820ms");
        assert_eq!(format_duration(Duration::from_millis(1200)), "1.2s");
        assert_eq!(format_duration(Duration::from_secs(75)), "1m 15s");
    }
}
