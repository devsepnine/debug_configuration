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

/// 터미널 출력 버퍼의 최대 라인 수
/// 이 제한을 초과하면 오래된 라인이 자동으로 제거됨
/// 라인별 렌더링 관리로 성능 최적화
const MAX_OUTPUT_LINES: usize = 2000;

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
            is_running: true,
            exit_code: None,
            finished_at: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            scroll_progress: 1.0, // 기본값: 맨 아래
            auto_scroll: true,    // 기본값: 자동 스크롤 활성화
            process_pid: None,
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
            // 라인 제한 초과 시 오래된 라인 제거 (FIFO).
            // VecDeque이므로 앞에서 제거가 O(1) — 출력 폭주 시 Vec::remove(0)의
            // O(n) 시프트 비용을 제거한다.
            if self.output_lines.len() >= MAX_OUTPUT_LINES {
                self.output_lines.pop_front();
            }

            // ANSI 색상 코드를 파싱하여 텍스트 세그먼트로 변환
            let segments = parse_ansi_text(single_line);

            // 고유 ID와 함께 새 라인 추가
            let line_id = self.next_line_id;
            self.next_line_id += 1;
            self.output_lines.push_back((line_id, segments));
        }
    }

    /// 출력 버퍼 초기화
    pub fn clear_output(&mut self) {
        self.output_lines.clear();
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
    fn format_duration_is_human_readable() {
        assert_eq!(format_duration(Duration::from_millis(820)), "820ms");
        assert_eq!(format_duration(Duration::from_millis(1200)), "1.2s");
        assert_eq!(format_duration(Duration::from_secs(75)), "1m 15s");
    }
}
