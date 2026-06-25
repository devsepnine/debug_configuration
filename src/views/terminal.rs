use crate::messages::Message;
use crate::models::RunSession;
use iced::{
    Border, Color, Element, Event, Length, Point, Rectangle, Renderer, Theme, border, keyboard,
    mouse, window,
    widget::{Canvas, canvas, container},
};
use std::collections::HashSet;
use unicode_width::UnicodeWidthChar;
use uuid::Uuid;

/// D2 Coding 폰트 사용
const FONT: iced::Font = crate::D2CODING;

// ============================================================================
// Constants
// ============================================================================

/// 라인 높이 (픽셀)
const LINE_HEIGHT: f32 = 16.0;

/// 폰트 크기
const FONT_SIZE: f32 = 12.0;

/// 마우스 휠 스크롤 감도 (Lines 모드)
const SCROLL_SPEED: f32 = 3.0;

/// 좌우 패딩 (픽셀)
const HORIZONTAL_PADDING: f32 = 10.0;

/// D2 Coding 12pt 기준 실제 문자 너비 (픽셀)
/// 런타임에 ttf-parser로 정확히 계산됨
fn get_char_width() -> f32 {
    // 간단한 캐싱: 한 번 계산된 값 재사용
    use std::sync::OnceLock;
    static CHAR_WIDTH: OnceLock<f32> = OnceLock::new();
    *CHAR_WIDTH.get_or_init(|| crate::utils::monospace_char_width(FONT_SIZE))
}

/// 스크롤바 너비 (픽셀)
const SCROLLBAR_WIDTH: f32 = 4.0;

/// 스크롤바 여백 (픽셀)
const SCROLLBAR_MARGIN: f32 = 12.0;

/// 스크롤바 히트 영역 추가 패딩 (픽셀) - 클릭하기 쉽게
const SCROLLBAR_HIT_PADDING: f32 = 5.0;

/// 텍스트 색상
const TEXT_COLOR: Color = Color::WHITE;

// ============================================================================
// Canvas State
// ============================================================================

/// 텍스트 선택 위치
#[derive(Debug, Clone, Copy, PartialEq)]
struct TextPosition {
    /// 라인 인덱스 (lines 벡터 기준)
    line_idx: usize,
    /// 문자 인덱스 (해당 라인 내)
    char_idx: usize,
}

/// 한 줄 안의 URL 위치(문자 인덱스 기준). 어느 줄인지는 소유자가 알고 있으므로 line_idx 없음.
/// 가상화: URL은 전체 버퍼를 미리 스캔하지 않고, 보이는 줄(렌더)·커서 아래 줄(히트테스트)에
/// 한해 즉석 계산한다 — O(visible).
#[derive(Debug, Clone)]
struct UrlSpan {
    /// URL 시작 문자 인덱스 (줄 내)
    start_char: usize,
    /// URL 끝 문자 인덱스 (줄 내)
    end_char: usize,
    /// URL 문자열
    url: String,
}

/// wrap 캐시 무효화 키. `prepare_lines`가 만드는 줄 집합·순서가 바뀌는 입력만 담는다.
/// 해시(u64) 대신 값 비교로 충돌 가능성을 원천 제거한다(충돌 시 stale 캐시 → 렌더 오정렬).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RenderKey {
    /// 세션 출력 변경 카운터.
    content_version: u64,
    /// 필터(매치만 표시) 모드 여부 — 줄 집합 자체를 바꾼다.
    filter: bool,
    /// 필터 모드일 때만 의미 있는 검색어(필터 OFF면 줄 집합이 검색어와 무관하므로 빈 문자열).
    query: String,
}

/// Canvas 스크롤 상태
#[derive(Debug, Default)]
struct ScrollState {
    /// 현재 스크롤 오프셋 (래핑된 라인 단위)
    offset: f32,
    /// 스크롤바 드래그 중 여부
    is_dragging_scrollbar: bool,
    /// 드래그 시작 시 마우스 Y 좌표
    drag_start_y: f32,
    /// 드래그 시작 시 스크롤 오프셋
    drag_start_offset: f32,
    /// 텍스트 선택 시작 위치
    selection_start: Option<TextPosition>,
    /// 텍스트 선택 끝 위치
    selection_end: Option<TextPosition>,
    /// 텍스트 선택 드래그 중 여부
    is_selecting: bool,
    /// 마지막으로 본 세션 ID (세션 전환 감지용)
    last_session_id: Option<Uuid>,
    /// 초기화 완료 여부
    is_initialized: bool,

    // ── 가상화 wrap 캐시 (S3) ─────────────────────────────────────────────
    // 매 프레임 모든 줄의 래핑 행 수를 per-char로 재계산하던 것을, 캐시된 정수 배열로
    // 대체한다. (max_chars=가로폭, content_version+필터=콘텐츠 키) 가 바뀔 때만 재생성.
    /// `lines`와 1:1 대응하는 줄별 래핑 행 수(캐시).
    line_counts: Vec<u32>,
    /// `line_counts`의 합(총 래핑 행 수) 캐시.
    cached_total: usize,
    /// 캐시를 만든 max_chars(가로폭). 불일치 시 재생성.
    cached_max_chars: usize,
    /// 캐시를 만든 콘텐츠/필터 키. 불일치 시 재생성.
    cached_render_key: RenderKey,
}

#[derive(Debug, Clone, Copy)]
struct ScrollMetrics {
    available_width: f32,
    visible_lines_f: f32,
    visible_lines: usize,
    total_wrapped_lines: usize,
    max_scroll: f32,
}

type DisplaySegment = (usize, usize, bool);

// ============================================================================
// Terminal Canvas
// ============================================================================

/// Canvas로 터미널 출력을 그리는 Program
///
/// Virtual scrolling과 자동 줄바꿈을 지원하여 대량의 로그를 효율적으로 렌더링
struct TerminalCanvas<'a> {
    /// 렌더링할 줄들. 대부분 세션 버퍼를 빌린다(zero-copy) — `prepare_lines`가 매 view마다
    /// 모든 줄을 deep-clone하던 비용을 제거한다. 합성 줄("No lines match")만 Owned.
    lines: Vec<LineRef<'a>>,
    /// `lines`와 1:1 대응하는 검색 하이라이트 종류 (매치/현재 매치/없음)
    highlights: Vec<LineHighlight>,
    session_id: Uuid,
    initial_scroll_progress: f32,
    auto_scroll: bool,
    /// 검색 점프 1회성 목표(논리줄 인덱스). `Some`이면 update()에서 wrapped offset으로
    /// 변환해 스크롤하고, publish로 app측 목표를 클리어한다(이슈 1).
    scroll_target: Option<usize>,
    /// 가상화 wrap 캐시 무효화 키. 이 키가 같으면 `prepare_lines` 결과(=`lines`)도
    /// 동일하므로 캐시를 재사용한다.
    render_key: RenderKey,
}

/// 터미널에 렌더링할 한 줄. 대부분은 세션 버퍼를 빌려(zero-copy) 클론 비용을 없앤다.
/// "No lines match" 같은 합성 줄만 Owned. `Deref<Target=[TextSegment]>`라 기존 렌더/측정
/// 코드(`&[TextSegment]`를 받는)들을 그대로 둘 수 있다.
enum LineRef<'a> {
    Ref(&'a Vec<crate::ansi::TextSegment>),
    Owned(Vec<crate::ansi::TextSegment>),
}

impl std::ops::Deref for LineRef<'_> {
    type Target = [crate::ansi::TextSegment];
    fn deref(&self) -> &Self::Target {
        match self {
            LineRef::Ref(segments) => segments,
            LineRef::Owned(segments) => segments,
        }
    }
}

/// 검색 시 라인 배경 하이라이트 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LineHighlight {
    /// 하이라이트 없음
    #[default]
    None,
    /// 검색어 매치 라인
    Match,
    /// 현재 선택된 매치 라인 (강조)
    Current,
}

impl<'a> TerminalCanvas<'a> {
    fn usize_to_f32(value: usize) -> f32 {
        // f32는 2^24까지 정수를 정확히 표현하므로 그 지점에서 clamp해 정밀도 손실을 막는다.
        // 과거에는 u16::MAX(65535)로 clamp했는데, 5만 줄 + 줄바꿈이면 래핑 행 수가 65535를
        // 넘어 total_wrapped가 조용히 잘리고 max_scroll/스크롤바/꼬리추적이 어긋났다.
        const MAX_EXACT: usize = 1 << 24;
        value.min(MAX_EXACT) as f32
    }

    fn f32_floor_to_usize(value: f32) -> usize {
        if !value.is_finite() || value <= 0.0 {
            return 0;
        }

        // f32 → usize 캐스팅은 Rust에서 saturating이라 거대한 값도 안전하다.
        value.floor() as usize
    }

    fn line_display_width_f32(text: &str) -> f32 {
        Self::usize_to_f32(Self::line_display_width(text))
    }

    // ============================================================================
    // URL 감지 및 처리
    // ============================================================================

    /// 한 줄의 텍스트에서 URL 위치를 추출한다 (RFC 3986: http/https/localhost, 금지문자
    /// 제외, trailing punctuation 제거). 전체 버퍼를 미리 스캔하지 않고 보이는 줄(렌더) 또는
    /// 커서 아래 줄(히트테스트)에 대해서만 호출되어 비용이 O(visible)이다.
    fn urls_in_line(line: &str) -> Vec<UrlSpan> {
        // RFC 3986 기반 URL 패턴
        // 금지 문자: whitespace, < > " { } | \ ^ ` (RFC 3986 Section 2.4)
        // Regex 컴파일은 비용이 크므로 최초 1회만 컴파일하여 재사용.
        static URL_PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(r#"https?://[^\s<>"{}|\\^`]+|localhost:\d+[^\s<>"{}|\\^`]*"#).unwrap()
        });

        let mut urls = Vec::new();
        for mat in URL_PATTERN.find_iter(line) {
            let raw_url = mat.as_str();

            // Trailing punctuation 제거 (URL이 문장 끝에 올 때 마침표/쉼표 등 포함 방지)
            let cleaned_url = Self::trim_trailing_punctuation(raw_url);
            if cleaned_url.is_empty() {
                continue;
            }

            let start_char = line[..mat.start()].chars().count();
            let end_char = start_char + cleaned_url.chars().count();
            urls.push(UrlSpan {
                start_char,
                end_char,
                url: cleaned_url.to_string(),
            });
        }
        urls
    }

    /// 세그먼트 벡터를 한 줄 문자열로 합쳐 URL을 추출한다.
    fn urls_in_segments(segments: &[crate::ansi::TextSegment]) -> Vec<UrlSpan> {
        let line: String = segments.iter().map(|s| s.text.as_str()).collect();
        Self::urls_in_line(&line)
    }

    /// URL 끝에서 일반적인 구두점 제거
    ///
    /// URL이 문장 끝에 오는 경우 (예: "Visit <http://example.com>.")
    /// 마침표가 URL의 일부로 인식되는 것을 방지
    fn trim_trailing_punctuation(url: &str) -> &str {
        let mut end = url.len();
        let bytes = url.as_bytes();

        // 뒤에서부터 구두점 제거: . , ; : ! ?
        // 단, URL path의 일부일 수 있는 경우는 제외
        while end > 0 {
            match bytes[end - 1] {
                b'.' | b',' | b';' | b':' | b'!' | b'?' => {
                    // 콜론(:)의 경우 포트 번호일 수 있으므로 체크
                    if bytes[end - 1] == b':' {
                        // "http://localhost:" 같은 형태면 제거하지 않음
                        // 하지만 문장 끝의 콜론은 제거
                        if end > 1 && bytes[end - 2].is_ascii_digit() {
                            break; // 포트 번호로 보임
                        }
                    }
                    end -= 1;
                }
                _ => break,
            }
        }

        &url[..end]
    }

    /// 주어진 텍스트 위치가 URL 영역인지 확인
    fn find_url_at_position(&self, pos: TextPosition) -> Option<String> {
        // 커서 아래 줄에 대해서만 URL을 즉석 계산해 위치를 판정한다(O(해당 줄 길이)).
        let segments = self.lines.get(pos.line_idx)?;
        Self::urls_in_segments(segments)
            .into_iter()
            .find(|url| pos.char_idx >= url.start_char && pos.char_idx < url.end_char)
            .map(|url| url.url)
    }

    // ============================================================================
    // Unicode Display Width 헬퍼 함수들
    // ============================================================================

    /// 라인의 총 display width 계산
    ///
    /// 반각 문자(ASCII) = 1, 전각 문자(한자, 한글 등) = 2
    fn line_display_width(line: &str) -> usize {
        line.chars().map(|c| c.width().unwrap_or(1)).sum()
    }

    /// 세그먼트 벡터의 총 display width 계산
    fn segments_display_width(segments: &[crate::ansi::TextSegment]) -> usize {
        segments
            .iter()
            .map(|s| Self::line_display_width(&s.text))
            .sum()
    }

    /// Display width offset을 문자 인덱스로 변환
    ///
    /// # Example
    /// "AB漢字" 에서 `display_offset=4` → `char_idx=3`
    fn display_to_char_index(line: &str, display_offset: usize) -> usize {
        let mut current_width = 0;
        for (idx, c) in line.chars().enumerate() {
            if current_width >= display_offset {
                return idx;
            }
            current_width += c.width().unwrap_or(1);
        }
        line.chars().count()
    }

    /// 라인을 display width 기반으로 잘라서 chunk 정보 반환
    ///
    /// # Returns
    /// `Vec<(start_char_idx, end_char_idx)>` - 각 chunk의 시작/끝 문자 인덱스
    fn split_line_by_display_width(line: &str, max_display_width: usize) -> Vec<(usize, usize)> {
        let chars: Vec<char> = line.chars().collect();
        let mut chunks = Vec::new();
        let mut chunk_start = 0;
        let mut current_idx = 0;
        let mut current_width = 0;

        for (idx, c) in chars.iter().enumerate() {
            let char_width = c.width().unwrap_or(1);

            if current_width + char_width > max_display_width && current_idx > chunk_start {
                chunks.push((chunk_start, current_idx));
                chunk_start = current_idx;
                current_width = char_width;
                current_idx = idx + 1;
            } else {
                current_width += char_width;
                current_idx = idx + 1;
            }
        }

        if chunk_start < chars.len() {
            chunks.push((chunk_start, chars.len()));
        }

        if chunks.is_empty() {
            chunks.push((0, 0));
        }

        chunks
    }

    /// 화면 너비에 맞게 줄바꿈했을 때의 총 렌더링 줄 수 계산
    ///
    /// # Arguments
    /// * `available_width` - 렌더링 가능한 화면 너비 (픽셀)
    /// * `char_width` - 실제 측정된 문자 너비
    ///
    /// # Returns
    /// 줄바꿈을 적용한 총 줄 수
    fn calculate_total_wrapped_lines(
        &self,
        state: &ScrollState,
        available_width: f32,
        char_width: f32,
    ) -> usize {
        let max_chars = Self::calculate_max_chars(available_width, char_width);
        if self.wrap_cache_valid(state, max_chars) {
            return state.cached_total;
        }
        // 캐시 미스(폭/콘텐츠 변경 직후 또는 update 미선행 등): 이 호출만 정확히 계산.
        // calculate_wrapped_count가 max_chars==0을 1행으로 처리하므로 0폭 분기는 불필요.
        self.lines
            .iter()
            .map(|line| Self::calculate_wrapped_count(line, max_chars))
            .sum()
    }

    /// 현재 캐시가 `(self.lines, max_chars, render_version)`과 정합하는지.
    fn wrap_cache_valid(&self, state: &ScrollState, max_chars: usize) -> bool {
        state.cached_max_chars == max_chars
            && state.cached_render_key == self.render_key
            && state.line_counts.len() == self.lines.len()
    }

    /// 키(가로폭/콘텐츠·필터 버전)가 바뀌었을 때만 줄별 래핑 행 수 캐시를 재생성한다.
    /// `update()`에서 `&mut State`로 매 프레임 호출되며, 키가 같으면 즉시 반환한다.
    fn rebuild_wrap_cache_if_needed(&self, state: &mut ScrollState, max_chars: usize) {
        if self.wrap_cache_valid(state, max_chars) {
            return;
        }
        state.line_counts = self
            .lines
            .iter()
            .map(|line| Self::calculate_wrapped_count(line, max_chars) as u32)
            .collect();
        state.cached_total = Self::wrapped_total(&state.line_counts);
        state.cached_max_chars = max_chars;
        state.cached_render_key = self.render_key.clone();
    }

    /// 한 줄의 최대 display width 계산
    ///
    /// # Returns
    /// 한 줄에 들어갈 수 있는 최대 display width (반각=1, 전각=2 기준)
    fn calculate_max_chars(available_width: f32, char_width: f32) -> usize {
        if char_width <= 0.0 {
            return 0;
        }
        Self::f32_floor_to_usize(available_width / char_width)
    }

    /// 한 라인이 줄바꿈될 경우 몇 줄이 되는지 계산 (display width 기반)
    ///
    /// # Arguments
    /// * `segments` - 텍스트 세그먼트 벡터
    /// * `max_display_width` - 한 줄의 최대 display width
    ///
    /// # Example
    /// "AB漢字" (display width = 6) / `max_display_width` = 4 → 2줄
    fn calculate_wrapped_count(
        segments: &[crate::ansi::TextSegment],
        max_display_width: usize,
    ) -> usize {
        let display_width = Self::segments_display_width(segments);
        // max_display_width==0(0폭/초기화 전)이면 줄바꿈 불가로 보고 1행 처리(0 나눗셈 패닉 방지).
        if display_width == 0 || max_display_width == 0 {
            1
        } else {
            display_width.div_ceil(max_display_width)
        }
    }

    /// 소스 줄별 래핑 행 수 배열로부터 총 래핑 행 수를 구한다.
    /// (가상화: 매 프레임 줄을 다시 순회해 per-char 폭을 더하는 대신, 캐시된 행 수를 합산.)
    fn wrapped_total(counts: &[u32]) -> usize {
        counts.iter().map(|&c| c as usize).sum()
    }
}

impl<'a> canvas::Program<Message> for TerminalCanvas<'a> {
    type State = ScrollState;

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if let Some(cursor_position) = cursor.position_in(bounds) {
            let available_width = bounds.width - (HORIZONTAL_PADDING * 2.0);
            let visible_lines =
                Self::f32_floor_to_usize(Self::calculate_visible_lines(bounds.height));
            let char_width = get_char_width();
            let total_wrapped_lines =
                self.calculate_total_wrapped_lines(state, available_width, char_width);

            // 스크롤바 드래그 중
            if state.is_dragging_scrollbar {
                return mouse::Interaction::Idle;
            }

            // 텍스트 선택 드래그 중
            if state.is_selecting {
                return mouse::Interaction::Text;
            }

            // 스크롤바 영역 확인
            if total_wrapped_lines > visible_lines {
                let scrollbar_bounds = self.calculate_scrollbar_bounds(
                    state,
                    bounds,
                    available_width,
                    visible_lines,
                    state.offset,
                );

                if Self::is_point_in_scrollbar(cursor_position, scrollbar_bounds) {
                    return mouse::Interaction::Pointer;
                }
            }

            // 텍스트 영역 - URL 확인
            if cursor_position.x >= HORIZONTAL_PADDING
                && cursor_position.x <= bounds.width - SCROLLBAR_MARGIN - SCROLLBAR_HIT_PADDING
            {
                // URL 위에 마우스가 있으면 포인터 커서
                if let Some(text_pos) =
                    self.position_to_text_coord(cursor_position, state, available_width)
                    && self.find_url_at_position(text_pos).is_some()
                {
                    return mouse::Interaction::Pointer;
                }
                // 일반 텍스트 영역은 I-beam 커서
                return mouse::Interaction::Text;
            }
        }

        mouse::Interaction::default()
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        // 가상화 wrap 캐시 갱신: 가로폭/콘텐츠/필터가 바뀐 프레임에만 재생성한다. update()는
        // 매 프레임(합성 RedrawRequested 포함) 호출되고 &mut State를 가지므로 여기가 유일한
        // 재생성 지점이다(draw()는 &State라 갱신 불가, 캐시를 읽기만 한다).
        //
        // initialize_scroll_state보다 먼저 둔다: 세션 최초 마운트/전환 시 init이 내부에서
        // scroll_metrics로 max_scroll을 읽어 초기 offset을 정하므로, 그 전에 캐시가 새 세션
        // 기준으로 준비돼 있어야 정확하다(아니면 그 프레임만 naive fallback으로 O(n) 이중 계산).
        let max_chars = {
            let available_width = bounds.width - (HORIZONTAL_PADDING * 2.0);
            Self::calculate_max_chars(available_width, get_char_width())
        };
        self.rebuild_wrap_cache_if_needed(state, max_chars);

        if let Some(action) = self.initialize_scroll_state(state, bounds) {
            return Some(action);
        }

        let metrics = self.scroll_metrics(state, bounds);

        // iced 0.14는 매 프레임 합성 RedrawRequested 이벤트로 Program::update를 호출한다
        // (iced_winit가 주입 → iced_widget Canvas가 모든 이벤트에 program.update 호출).
        // 따라서 auto-scroll 꼬리 추적(맨 아래 재고정)은 이 프레임 틱에서만 수행하고,
        // 실제 사용자 입력(휠/클릭/드래그/선택/Ctrl+C)은 아래 match로 먼저 라우팅해야 한다.
        //
        // 회귀 방지: 과거에는 apply_auto_scroll를 update 선두에서 무조건 실행하고 Some이면
        // 조기 반환했다. RedrawRequested가 매 프레임 도착하는 데다 출력 스트리밍 중에는
        // auto_scroll=true이고 새 줄마다 max_scroll이 커져 항상 Some(재고정)을 반환했기에,
        // 휠 등 입력 이벤트가 match에 닿기 전에 영구히 선점되어 스크롤 자체가 불가능했다.
        if let Event::Window(window::Event::RedrawRequested(_)) = event {
            // 검색 점프 목표가 있으면 auto_scroll 재고정보다 먼저 적용한다(이슈 1).
            if let Some(action) = self.apply_scroll_target(state, metrics) {
                return Some(action);
            }
            return self.apply_auto_scroll(state, metrics);
        }

        match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                return self.handle_wheel_scrolled(state, delta, bounds, cursor, metrics);
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                return self.handle_left_button_pressed(state, bounds, cursor, metrics);
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                return self.handle_left_button_released(state, metrics);
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                return self.handle_cursor_moved(state, *position, bounds, metrics);
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(c),
                modifiers,
                ..
            }) if c.as_str() == "c" && modifiers.command() => {
                if let (Some(start), Some(end)) = (state.selection_start, state.selection_end) {
                    let text = self.extract_selected_text(start, end);
                    if !text.is_empty() {
                        return Some(canvas::Action::publish(Message::CopyToClipboard(text)));
                    }
                }
            }
            _ => {}
        }

        None
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        // D2 Coding 폰트의 정확한 문자 너비 계산
        let char_width = crate::utils::monospace_char_width(FONT_SIZE);

        let available_width = bounds.width - (HORIZONTAL_PADDING * 2.0);
        let max_chars = Self::calculate_max_chars(available_width, char_width);

        if max_chars == 0 {
            return vec![frame.into_geometry()];
        }

        let visible_lines = Self::f32_floor_to_usize(Self::calculate_visible_lines(bounds.height));

        // theme에서 색상들 가져오기
        let url_color = theme.extended_palette().primary.base.color;
        let selection_color = {
            let base = theme.extended_palette().primary.weak.color;
            iced::Color {
                r: base.r,
                g: base.g,
                b: base.b,
                a: 0.4,
            }
        };
        let scrollbar_color = theme.extended_palette().secondary.weak.color;
        // 검색 매치 라인 배경 (warning=노랑 계열). 현재 매치는 더 진하게.
        let warning = theme.extended_palette().warning.base.color;
        let match_bg = Color { a: 0.16, ..warning };
        let current_match_bg = Color { a: 0.42, ..warning };

        // Z-order: 매치 배경 → 선택 → 텍스트(render_lines). 매치 배경을 먼저 깔아야
        // 선택 배경이 그 위에 보인다(검색 중 텍스트 선택 시).
        self.render_line_highlights(
            &mut frame,
            state,
            max_chars,
            visible_lines,
            match_bg,
            current_match_bg,
        );
        self.render_selection(
            &mut frame,
            state,
            max_chars,
            visible_lines,
            char_width,
            selection_color,
        );
        self.render_lines(&mut frame, state, max_chars, visible_lines, url_color);
        self.render_scrollbar(
            &mut frame,
            state,
            bounds,
            available_width,
            visible_lines,
            scrollbar_color,
        );

        vec![frame.into_geometry()]
    }
}

// ============================================================================
// Terminal Canvas - Private Methods
// ============================================================================

impl<'a> TerminalCanvas<'a> {
    /// 스크롤바의 위치와 크기 계산
    ///
    /// # Returns
    /// (x, y, width, height)
    fn calculate_scrollbar_bounds(
        &self,
        state: &ScrollState,
        bounds: Rectangle,
        available_width: f32,
        visible_lines: usize,
        offset: f32,
    ) -> (f32, f32, f32, f32) {
        let total_wrapped_lines =
            self.calculate_total_wrapped_lines(state, available_width, get_char_width());
        let total_lines_f = Self::usize_to_f32(total_wrapped_lines);
        let visible_lines_f = Self::usize_to_f32(visible_lines);

        let scrollbar_height = (visible_lines_f / total_lines_f) * bounds.height;
        let max_scroll = total_lines_f - visible_lines_f;
        let scrollbar_y = if max_scroll > 0.0 {
            (offset / max_scroll) * (bounds.height - scrollbar_height)
        } else {
            0.0
        };

        (
            bounds.width - SCROLLBAR_MARGIN,
            scrollbar_y,
            SCROLLBAR_WIDTH,
            scrollbar_height,
        )
    }

    fn scroll_metrics(&self, state: &ScrollState, bounds: Rectangle) -> ScrollMetrics {
        let available_width = bounds.width - (HORIZONTAL_PADDING * 2.0);
        let visible_lines_f = Self::calculate_visible_lines(bounds.height);
        let visible_lines = Self::f32_floor_to_usize(visible_lines_f);
        let total_wrapped_lines =
            self.calculate_total_wrapped_lines(state, available_width, get_char_width());
        let max_scroll = (Self::usize_to_f32(total_wrapped_lines) - visible_lines_f).max(0.0);

        ScrollMetrics {
            available_width,
            visible_lines_f,
            visible_lines,
            total_wrapped_lines,
            max_scroll,
        }
    }

    fn scroll_progress(offset: f32, max_scroll: f32) -> f32 {
        if max_scroll > 0.0 {
            (offset / max_scroll).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    fn initialize_scroll_state(
        &self,
        state: &mut ScrollState,
        bounds: Rectangle,
    ) -> Option<canvas::Action<Message>> {
        if state.last_session_id == Some(self.session_id) && state.is_initialized {
            return None;
        }

        state.last_session_id = Some(self.session_id);
        state.is_initialized = true;

        let metrics = self.scroll_metrics(state, bounds);
        state.offset =
            (self.initial_scroll_progress * metrics.max_scroll).clamp(0.0, metrics.max_scroll);

        Some(canvas::Action::request_redraw())
    }

    fn apply_auto_scroll(
        &self,
        state: &mut ScrollState,
        metrics: ScrollMetrics,
    ) -> Option<canvas::Action<Message>> {
        if !self.auto_scroll || (state.offset - metrics.max_scroll).abs() <= 0.01 {
            return None;
        }

        state.offset = metrics.max_scroll;
        Some(canvas::Action::request_redraw())
    }

    /// 검색 점프 1회성 목표를 적용한다(이슈 1). 논리줄 인덱스를 `line_counts`로 wrapped
    /// offset에 변환해 `offset`을 설정하고, app측 목표를 클리어하도록 `SessionScrollChanged`를
    /// 발행한다. 목표가 없으면 None.
    ///
    /// 불변식: `line_counts`는 update 선두의 `rebuild_wrap_cache_if_needed`로 항상 `lines`와
    /// 길이가 일치하므로 슬라이스 경계가 안전하다.
    /// 한계: `scroll_target`은 `output_lines`의 현재 인덱스라, 실행 중 스트리밍으로 앞쪽 줄이
    /// evict되면 stale해져 점프가 어긋날 수 있다(드묾·비치명적). 근본 해결은 line_id 기반
    /// 앵커(deferred)로 다룬다.
    fn apply_scroll_target(
        &self,
        state: &mut ScrollState,
        metrics: ScrollMetrics,
    ) -> Option<canvas::Action<Message>> {
        let target_line = self.scroll_target?;
        // 목표 줄 앞쪽 줄들의 래핑 행 수 합 = 목표 줄이 시작하는 wrapped offset.
        let clamped_line = target_line.min(state.line_counts.len());
        let wrapped_offset: usize = state.line_counts[..clamped_line]
            .iter()
            .map(|&c| c as usize)
            .sum();
        state.offset = (wrapped_offset as f32).clamp(0.0, metrics.max_scroll);
        Some(self.publish_scroll_progress(state.offset, metrics.max_scroll))
    }

    /// offset이 바닥(맨 아래)에 충분히 가까운지 — 절대 거리로 판정한다(이슈 2). 비율
    /// 임계치는 버퍼가 커질수록 바닥 근처 수백 줄을 "바닥"으로 오판해 auto_scroll이 풀리지
    /// 않았다. 절대 거리면 바닥에서 한 줄만 올려도 false가 된다.
    ///
    /// `apply_auto_scroll`의 0.01과는 목적이 다르다: 여기 `BOTTOM_EPSILON`(0.5)은 "사용자가
    /// 바닥을 벗어났는가"(auto_scroll 해제) 판정이고, 0.01은 "이미 바닥이라 재고정 불필요"
    /// (부동소수점 흡수)라 값이 다르다.
    fn is_at_bottom(offset: f32, max_scroll: f32) -> bool {
        // 바닥으로 간주할 최대 거리(래핑 행).
        const BOTTOM_EPSILON: f32 = 0.5;
        max_scroll <= 0.0 || (max_scroll - offset).abs() <= BOTTOM_EPSILON
    }

    fn publish_scroll_progress(&self, offset: f32, max_scroll: f32) -> canvas::Action<Message> {
        canvas::Action::publish(Message::SessionScrollChanged(
            self.session_id,
            Self::scroll_progress(offset, max_scroll),
            Self::is_at_bottom(offset, max_scroll),
        ))
    }

    fn handle_wheel_scrolled(
        &self,
        state: &mut ScrollState,
        delta: &mouse::ScrollDelta,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        metrics: ScrollMetrics,
    ) -> Option<canvas::Action<Message>> {
        cursor.position_in(bounds)?;

        let scroll_delta = Self::calculate_scroll_delta(delta);
        if scroll_delta == 0.0 {
            return None;
        }

        self.update_scroll_offset(state, scroll_delta, bounds);
        Some(self.publish_scroll_progress(state.offset, metrics.max_scroll))
    }

    fn handle_left_button_pressed(
        &self,
        state: &mut ScrollState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        metrics: ScrollMetrics,
    ) -> Option<canvas::Action<Message>> {
        let cursor_position = cursor.position_in(bounds)?;

        if metrics.total_wrapped_lines > metrics.visible_lines {
            let scrollbar_bounds = self.calculate_scrollbar_bounds(
                state,
                bounds,
                metrics.available_width,
                metrics.visible_lines,
                state.offset,
            );

            if Self::is_point_in_scrollbar(cursor_position, scrollbar_bounds) {
                state.is_dragging_scrollbar = true;
                state.drag_start_y = cursor_position.y;
                state.drag_start_offset = state.offset;
                return Some(canvas::Action::request_redraw());
            }
        }

        let text_pos =
            self.position_to_text_coord(cursor_position, state, metrics.available_width)?;
        state.is_selecting = true;
        state.selection_start = Some(text_pos);
        state.selection_end = Some(text_pos);
        Some(canvas::Action::request_redraw())
    }

    fn handle_left_button_released(
        &self,
        state: &mut ScrollState,
        metrics: ScrollMetrics,
    ) -> Option<canvas::Action<Message>> {
        if state.is_dragging_scrollbar {
            state.is_dragging_scrollbar = false;
            return Some(self.publish_scroll_progress(state.offset, metrics.max_scroll));
        }

        if !state.is_selecting {
            return None;
        }

        state.is_selecting = false;

        if let (Some(start), Some(end)) = (state.selection_start, state.selection_end)
            && start == end
            && let Some(url) = self.find_url_at_position(start)
        {
            return Some(canvas::Action::publish(Message::OpenUrl(url)));
        }

        Some(canvas::Action::request_redraw())
    }

    fn handle_cursor_moved(
        &self,
        state: &mut ScrollState,
        position: Point,
        bounds: Rectangle,
        metrics: ScrollMetrics,
    ) -> Option<canvas::Action<Message>> {
        let cursor_position = Point::new(position.x - bounds.x, position.y - bounds.y);

        if state.is_dragging_scrollbar && metrics.total_wrapped_lines > metrics.visible_lines {
            let scrollbar_height = (metrics.visible_lines_f
                / Self::usize_to_f32(metrics.total_wrapped_lines))
                * bounds.height;
            let scrollable_height = bounds.height - scrollbar_height;
            let mouse_delta = cursor_position.y - state.drag_start_y;
            let scroll_delta = (mouse_delta / scrollable_height) * metrics.max_scroll;

            state.offset = (state.drag_start_offset + scroll_delta)
                .max(0.0)
                .min(metrics.max_scroll);

            return Some(canvas::Action::request_redraw());
        }

        if !state.is_selecting {
            return None;
        }

        let text_pos =
            self.position_to_text_coord(cursor_position, state, metrics.available_width)?;
        state.selection_end = Some(text_pos);
        Some(canvas::Action::request_redraw())
    }

    /// 포인트가 스크롤바 영역 안에 있는지 확인 (히트 영역 패딩 포함)
    fn is_point_in_scrollbar(point: Point, scrollbar: (f32, f32, f32, f32)) -> bool {
        let (x, y, width, height) = scrollbar;
        // 히트 영역을 더 넓게 (좌측과 상하에 패딩 추가)
        point.x >= x - SCROLLBAR_HIT_PADDING
            && point.x <= x + width + SCROLLBAR_HIT_PADDING
            && point.y >= y - SCROLLBAR_HIT_PADDING
            && point.y <= y + height + SCROLLBAR_HIT_PADDING
    }

    /// 상하 패딩을 제외한 실제 표시 가능한 라인 수 계산
    fn calculate_visible_lines(bounds_height: f32) -> f32 {
        let available_height = bounds_height - (HORIZONTAL_PADDING * 2.0);
        (available_height / LINE_HEIGHT).floor()
    }

    /// 스크롤 델타 계산
    fn calculate_scroll_delta(delta: &mouse::ScrollDelta) -> f32 {
        match delta {
            mouse::ScrollDelta::Lines { y, .. } => -y * SCROLL_SPEED,
            mouse::ScrollDelta::Pixels { y, .. } => -y / LINE_HEIGHT,
        }
    }

    /// 스크롤 오프셋 업데이트
    fn update_scroll_offset(&self, state: &mut ScrollState, scroll_delta: f32, bounds: Rectangle) {
        let visible_lines = Self::calculate_visible_lines(bounds.height);
        let available_width = bounds.width - (HORIZONTAL_PADDING * 2.0);
        let total_wrapped_lines =
            self.calculate_total_wrapped_lines(state, available_width, get_char_width());

        // 스크롤 범위 제한
        state.offset = (state.offset + scroll_delta)
            .max(0.0)
            .min((Self::usize_to_f32(total_wrapped_lines) - visible_lines).max(0.0));
    }

    /// 선택 영역 정규화 (시작이 끝보다 앞에 오도록)
    fn normalize_selection(start: TextPosition, end: TextPosition) -> (TextPosition, TextPosition) {
        match start.line_idx.cmp(&end.line_idx) {
            std::cmp::Ordering::Less => (start, end),
            std::cmp::Ordering::Equal if start.char_idx <= end.char_idx => (start, end),
            std::cmp::Ordering::Greater | std::cmp::Ordering::Equal => (end, start),
        }
    }

    /// 선택된 텍스트 추출
    fn extract_selected_text(&self, start: TextPosition, end: TextPosition) -> String {
        let (start, end) = Self::normalize_selection(start, end);

        if start == end {
            return String::new();
        }

        let mut result = String::new();

        for line_idx in start.line_idx..=end.line_idx {
            if let Some(segments_vec) = self.lines.get(line_idx) {
                // 세그먼트들을 문자열로 결합
                let line: String = segments_vec.iter().map(|s| s.text.as_str()).collect();
                let chars: Vec<char> = line.chars().collect();

                let start_char = if line_idx == start.line_idx {
                    start.char_idx
                } else {
                    0
                };

                let end_char = if line_idx == end.line_idx {
                    end.char_idx.min(chars.len())
                } else {
                    chars.len()
                };

                if end_char > start_char {
                    let selected: String = chars[start_char..end_char].iter().collect();
                    result.push_str(&selected);
                }

                // 멀티라인 선택인 경우 개행 추가 (빈 줄도 포함)
                if line_idx < end.line_idx {
                    result.push('\n');
                }
            }
        }

        result
    }

    /// 텍스트 선택 하이라이트 렌더링
    fn render_selection(
        &self,
        frame: &mut canvas::Frame,
        state: &ScrollState,
        max_chars: usize,
        visible_lines: usize,
        char_width: f32,
        selection_color: Color,
    ) {
        // 선택 영역이 있는지 확인
        let (start, end) = match (state.selection_start, state.selection_end) {
            (Some(s), Some(e)) if s != e => Self::normalize_selection(s, e),
            _ => return,
        };
        let partial_offset = state.offset.fract();
        let target_start_line = Self::f32_floor_to_usize(state.offset);

        // 각 라인을 순회하며 선택 영역 렌더링
        let mut current_wrapped_line = 0;
        let mut rendered_lines = 0; // 실제 렌더링 카운터 추가

        let cache_ok = self.wrap_cache_valid(state, max_chars);
        for (line_idx, segments_vec) in self.lines.iter().enumerate() {
            let wrapped_count = if cache_ok {
                state.line_counts[line_idx] as usize
            } else {
                Self::calculate_wrapped_count(segments_vec, max_chars)
            };

            // 가시성 체크: 완전히 위에 있으면 스킵 (render_lines와 동일)
            if current_wrapped_line + wrapped_count <= target_start_line {
                current_wrapped_line += wrapped_count;
                continue;
            }

            // 가시성 체크: 완전히 아래에 있으면 종료 (render_lines와 동일)
            if current_wrapped_line > target_start_line + visible_lines {
                break;
            }

            // 가시 영역 라인만 문자열로 결합 (스킵된 윗부분 라인은 할당하지 않음)
            let line: String = segments_vec.iter().map(|s| s.text.as_str()).collect();
            let line_len = line.chars().count();

            // 빈 줄 처리 (render_empty_line과 동일)
            if line_len == 0 {
                if current_wrapped_line >= target_start_line {
                    rendered_lines += 1;
                }
                current_wrapped_line += 1;
                continue;
            }

            // Display width 기반으로 줄바꿈된 chunk 정보 얻기
            let chunks = Self::split_line_by_display_width(&line, max_chars);

            // 이 라인이 선택 범위에 포함되는지 확인
            let in_selection = line_idx >= start.line_idx && line_idx <= end.line_idx;

            let (sel_start_char, sel_end_char) = if in_selection {
                let sel_start = if line_idx == start.line_idx {
                    start.char_idx
                } else {
                    0
                };
                let sel_end = if line_idx == end.line_idx {
                    end.char_idx
                } else {
                    line_len
                };
                (sel_start, sel_end)
            } else {
                (0, 0) // 선택 없음
            };

            // 각 chunk 처리
            for (chunk_idx, (chunk_start_char, chunk_end_char)) in chunks.iter().enumerate() {
                let this_wrapped_line = current_wrapped_line + chunk_idx;

                // 이 래핑 라인이 화면에 보이는지 확인
                if this_wrapped_line >= target_start_line
                    && this_wrapped_line < target_start_line + visible_lines + 1
                {
                    // 선택 범위 내이고 이 chunk에 선택된 부분이 있는지 확인
                    let has_selection = in_selection
                        && sel_end_char > sel_start_char
                        && sel_end_char > *chunk_start_char
                        && sel_start_char < *chunk_end_char;

                    if has_selection {
                        // render_wrapped_line과 동일하게 rendered_lines 사용
                        let y = (Self::usize_to_f32(rendered_lines) - partial_offset) * LINE_HEIGHT;
                        let y_aligned = y.round();

                        // 이 chunk 내에서 선택된 문자 범위
                        let chunk_sel_start = sel_start_char.max(*chunk_start_char);
                        let chunk_sel_end = sel_end_char.min(*chunk_end_char);

                        // 라인의 부분 문자열 추출
                        let line_chars: Vec<char> = line.chars().collect();
                        let sel_text_before: String = line_chars
                            [*chunk_start_char..chunk_sel_start]
                            .iter()
                            .collect();
                        let sel_text: String =
                            line_chars[chunk_sel_start..chunk_sel_end].iter().collect();

                        // Display width 계산
                        // 선택 사각형 그리기
                        let x_start = HORIZONTAL_PADDING
                            + (Self::line_display_width_f32(&sel_text_before) * char_width);
                        let width = Self::line_display_width_f32(&sel_text) * char_width;

                        let selection_rect = canvas::Path::rectangle(
                            Point::new(x_start, y_aligned + HORIZONTAL_PADDING),
                            iced::Size::new(width, LINE_HEIGHT),
                        );
                        frame.fill(&selection_rect, selection_color);
                    }

                    rendered_lines += 1; // 모든 보이는 줄에 대해 카운터 증가
                }
            }

            current_wrapped_line += wrapped_count;
        }
    }

    /// 화면 좌표를 텍스트 위치로 변환
    /// 빈 공간도 가장 가까운 텍스트 위치로 매핑
    fn position_to_text_coord(
        &self,
        point: Point,
        state: &ScrollState,
        available_width: f32,
    ) -> Option<TextPosition> {
        // 라인이 없으면 처리 불가
        if self.lines.is_empty() {
            return None;
        }

        // 패딩 제외한 상대 좌표
        let rel_x = (point.x - HORIZONTAL_PADDING).max(0.0);
        let rel_y = (point.y - HORIZONTAL_PADDING).max(0.0);

        // 실제 문자 너비
        let char_width = get_char_width();
        let max_chars = Self::calculate_max_chars(available_width, char_width);

        if max_chars == 0 {
            return None;
        }

        // 클릭한 라인 인덱스 (스크롤 오프셋 포함)
        let clicked_line_idx = Self::f32_floor_to_usize(rel_y / LINE_HEIGHT);
        let absolute_line_idx = clicked_line_idx + Self::f32_floor_to_usize(state.offset);

        // 라인 인덱스를 실제 lines 배열 인덱스로 변환
        let mut current_wrapped_line = 0;
        let mut last_line_idx = 0;

        let cache_ok = self.wrap_cache_valid(state, max_chars);
        for (line_idx, segments_vec) in self.lines.iter().enumerate() {
            // 세그먼트들을 문자열로 결합
            let line: String = segments_vec.iter().map(|s| s.text.as_str()).collect();
            let wrapped_count = if cache_ok {
                state.line_counts[line_idx] as usize
            } else {
                Self::calculate_wrapped_count(segments_vec, max_chars)
            };
            last_line_idx = line_idx;

            if absolute_line_idx >= current_wrapped_line
                && absolute_line_idx < current_wrapped_line + wrapped_count
            {
                // 이 라인 내에 있음
                let line_offset = absolute_line_idx - current_wrapped_line;
                let line_len = line.chars().count();

                // Display width 기반으로 줄바꿈된 chunks 얻기
                let chunks = Self::split_line_by_display_width(&line, max_chars);

                // 클릭된 줄이 몇 번째 chunk인지 확인
                if line_offset < chunks.len() {
                    let (chunk_start_char, chunk_end_char) = chunks[line_offset];

                    // 클릭된 display offset 계산
                    let clicked_display_offset =
                        Self::f32_floor_to_usize((rel_x / char_width).round());

                    // chunk의 부분 문자열로 display offset을 char index로 변환
                    let line_chars: Vec<char> = line.chars().collect();
                    let chunk_text: String = line_chars[chunk_start_char..chunk_end_char]
                        .iter()
                        .collect();

                    // chunk 내에서의 상대 char index
                    let relative_char_idx =
                        Self::display_to_char_index(&chunk_text, clicked_display_offset);

                    // 절대 char index
                    let char_idx = (chunk_start_char + relative_char_idx).min(line_len);

                    return Some(TextPosition { line_idx, char_idx });
                }

                // chunk를 찾지 못한 경우 (이론적으로 발생하지 않음)
                return Some(TextPosition {
                    line_idx,
                    char_idx: line_len,
                });
            }

            current_wrapped_line += wrapped_count;
        }

        // 클릭한 위치가 모든 라인 아래쪽이면 마지막 라인의 끝으로
        if let Some(last_segments) = self.lines.last() {
            let last_line: String = last_segments.iter().map(|s| s.text.as_str()).collect();
            let line_len = last_line.chars().count();
            return Some(TextPosition {
                line_idx: last_line_idx,
                char_idx: line_len,
            });
        }

        None
    }

    /// 텍스트 라인 렌더링
    /// 검색 매치 라인 배경 패스. 텍스트·선택보다 먼저 그려 Z-order가
    /// 매치배경 → 선택 → 텍스트가 되게 한다. `rendered_lines` 증가 규칙은
    /// `render_lines`와 정확히 동일해야 y 정렬이 맞는다.
    fn render_line_highlights(
        &self,
        frame: &mut canvas::Frame,
        state: &ScrollState,
        max_chars: usize,
        visible_lines: usize,
        match_bg: Color,
        current_match_bg: Color,
    ) {
        if self.highlights.iter().all(|h| *h == LineHighlight::None) {
            return;
        }

        let partial_offset = state.offset.fract();
        let target_start_line = Self::f32_floor_to_usize(state.offset);
        let mut current_wrapped_line = 0;
        let mut rendered_lines = 0;

        let cache_ok = self.wrap_cache_valid(state, max_chars);
        for (line_idx, segments) in self.lines.iter().enumerate() {
            let wrapped_count = if cache_ok {
                state.line_counts[line_idx] as usize
            } else {
                Self::calculate_wrapped_count(segments, max_chars)
            };
            if current_wrapped_line + wrapped_count <= target_start_line {
                current_wrapped_line += wrapped_count;
                continue;
            }
            if current_wrapped_line > target_start_line + visible_lines {
                break;
            }

            let bg = match self.highlights.get(line_idx).copied().unwrap_or_default() {
                LineHighlight::Current => Some(current_match_bg),
                LineHighlight::Match => Some(match_bg),
                LineHighlight::None => None,
            };

            if segments.iter().all(|s| s.text.is_empty()) {
                // 빈 줄: 하이라이트 대상 아님. render_empty_line과 동일하게 카운팅.
                if current_wrapped_line >= target_start_line {
                    rendered_lines += 1;
                }
                current_wrapped_line += 1;
            } else {
                for chunk_idx in 0..wrapped_count {
                    let this_wrapped_line = current_wrapped_line + chunk_idx;
                    if this_wrapped_line >= target_start_line
                        && this_wrapped_line < target_start_line + visible_lines + 1
                    {
                        if let Some(bg) = bg {
                            let y =
                                (Self::usize_to_f32(rendered_lines) - partial_offset) * LINE_HEIGHT;
                            let y_aligned = y.round();
                            let rect = canvas::Path::rectangle(
                                Point::new(0.0, y_aligned + HORIZONTAL_PADDING),
                                iced::Size::new(frame.width(), LINE_HEIGHT),
                            );
                            frame.fill(&rect, bg);
                        }
                        rendered_lines += 1;
                    }
                }
                current_wrapped_line += wrapped_count;
            }

            if rendered_lines >= visible_lines + 2 {
                break;
            }
        }
    }

    fn render_lines(
        &self,
        frame: &mut canvas::Frame,
        state: &ScrollState,
        max_chars: usize,
        visible_lines: usize,
        url_color: Color,
    ) {
        let partial_offset = state.offset.fract();
        let target_start_line = Self::f32_floor_to_usize(state.offset);

        let mut current_wrapped_line = 0;
        let mut rendered_lines = 0;

        let cache_ok = self.wrap_cache_valid(state, max_chars);
        for (i, segments) in self.lines.iter().enumerate() {
            let wrapped_count = if cache_ok {
                state.line_counts[i] as usize
            } else {
                Self::calculate_wrapped_count(segments, max_chars)
            };

            // 가시성 체크: 완전히 위에 있으면 스킵
            if current_wrapped_line + wrapped_count <= target_start_line {
                current_wrapped_line += wrapped_count;
                continue;
            }

            // 가시성 체크: 완전히 아래에 있으면 종료
            if current_wrapped_line > target_start_line + visible_lines {
                break;
            }

            // 라인 렌더링 (빈 줄 여부는 String 결합 없이 세그먼트 직접 검사)
            if segments.iter().all(|s| s.text.is_empty()) {
                Self::render_empty_line(
                    frame,
                    current_wrapped_line,
                    target_start_line,
                    &mut rendered_lines,
                    partial_offset,
                );
                current_wrapped_line += 1;
            } else {
                self.render_wrapped_line(
                    frame,
                    segments,
                    max_chars,
                    current_wrapped_line,
                    target_start_line,
                    visible_lines,
                    &mut rendered_lines,
                    partial_offset,
                    url_color,
                );
                current_wrapped_line += wrapped_count;
            }

            // 충분히 렌더링했으면 종료
            if rendered_lines >= visible_lines + 2 {
                break;
            }
        }
    }

    /// 빈 라인 렌더링
    fn render_empty_line(
        frame: &mut canvas::Frame,
        current_wrapped_line: usize,
        target_start_line: usize,
        rendered_lines: &mut usize,
        partial_offset: f32,
    ) {
        if current_wrapped_line >= target_start_line {
            let y = (Self::usize_to_f32(*rendered_lines) - partial_offset) * LINE_HEIGHT;
            // 픽셀 경계에 정렬하여 Windows에서 텍스트 찌그러짐 방지
            let y_aligned = y.round();
            frame.fill_text(canvas::Text {
                content: String::new(),
                position: Point::new(HORIZONTAL_PADDING, y_aligned + HORIZONTAL_PADDING),
                color: TEXT_COLOR,
                size: FONT_SIZE.into(),
                font: FONT,
                line_height: iced::widget::text::LineHeight::Absolute(LINE_HEIGHT.into()),
                ..canvas::Text::default()
            });
            *rendered_lines += 1;
        }
    }

    /// 줄바꿈된 라인 렌더링 (display width 기반, 색상 세그먼트 지원)
    ///
    /// 전각 문자(한자, 한글)는 2칸, 반각 문자(ASCII)는 1칸으로 계산하여 줄바꿈
    #[allow(clippy::too_many_arguments)]
    fn render_wrapped_line(
        &self,
        frame: &mut canvas::Frame,
        segments: &[crate::ansi::TextSegment],
        max_display_width: usize,
        current_wrapped_line: usize,
        target_start_line: usize,
        visible_lines: usize,
        rendered_lines: &mut usize,
        partial_offset: f32,
        url_color: Color,
    ) {
        let line: String = segments.iter().map(|s| s.text.as_str()).collect();
        let line_urls = Self::urls_in_line(&line);
        let chars: Vec<char> = line.chars().collect();
        let chunk_ranges = Self::chunk_ranges(&chars, max_display_width);

        let char_width = get_char_width();
        for (chunk_idx, (chunk_start, chunk_end)) in chunk_ranges.iter().enumerate() {
            let this_wrapped_line = current_wrapped_line + chunk_idx;

            if this_wrapped_line >= target_start_line
                && this_wrapped_line < target_start_line + visible_lines + 1
            {
                let y = (Self::usize_to_f32(*rendered_lines) - partial_offset) * LINE_HEIGHT;
                let y_aligned = y.round();
                Self::render_wrapped_chunk(
                    frame,
                    segments,
                    &chars,
                    &line_urls,
                    *chunk_start,
                    *chunk_end,
                    y_aligned,
                    char_width,
                    url_color,
                );
                *rendered_lines += 1;
            }
        }
    }

    /// 스크롤바 렌더링
    fn render_scrollbar(
        &self,
        frame: &mut canvas::Frame,
        state: &ScrollState,
        bounds: Rectangle,
        available_width: f32,
        visible_lines: usize,
        scrollbar_color: Color,
    ) {
        let total_wrapped_lines =
            self.calculate_total_wrapped_lines(state, available_width, get_char_width());

        if total_wrapped_lines <= visible_lines {
            return;
        }

        let total_lines_f = Self::usize_to_f32(total_wrapped_lines);
        let visible_lines_f = Self::usize_to_f32(visible_lines);

        // 스크롤바 높이 계산
        let scrollbar_height = (visible_lines_f / total_lines_f) * bounds.height;

        // 스크롤바 위치 계산
        let max_scroll = total_lines_f - visible_lines_f;
        let scrollbar_y = if max_scroll > 0.0 {
            (state.offset / max_scroll) * (bounds.height - scrollbar_height)
        } else {
            0.0
        };

        // 스크롤바 그리기
        let scrollbar = canvas::Path::rectangle(
            Point::new(bounds.width - SCROLLBAR_MARGIN, scrollbar_y),
            iced::Size::new(SCROLLBAR_WIDTH, scrollbar_height),
        );
        frame.fill(&scrollbar, scrollbar_color);
    }

    fn chunk_ranges(chars: &[char], max_display_width: usize) -> Vec<(usize, usize)> {
        let mut chunk_ranges = Vec::new();
        let mut chunk_start = 0;
        let mut current_idx = 0;
        let mut current_width = 0;

        for (idx, c) in chars.iter().enumerate() {
            let char_width = c.width().unwrap_or(1);

            if current_width + char_width > max_display_width && current_idx > chunk_start {
                chunk_ranges.push((chunk_start, current_idx));
                chunk_start = current_idx;
                current_width = char_width;
                current_idx = idx + 1;
            } else {
                current_width += char_width;
                current_idx = idx + 1;
            }
        }

        if chunk_start < chars.len() {
            chunk_ranges.push((chunk_start, chars.len()));
        }

        chunk_ranges
    }

    fn display_segments_for_chunk(
        line_urls: &[UrlSpan],
        chunk_start: usize,
        chunk_end: usize,
    ) -> Vec<DisplaySegment> {
        let mut display_segments = Vec::new();
        let mut seg_start = chunk_start;

        for url in line_urls {
            if url.start_char < chunk_end && url.end_char > chunk_start {
                let url_start_in_chunk = url.start_char.max(chunk_start);
                let url_end_in_chunk = url.end_char.min(chunk_end);

                if seg_start < url_start_in_chunk {
                    display_segments.push((seg_start, url_start_in_chunk, false));
                }
                display_segments.push((url_start_in_chunk, url_end_in_chunk, true));
                seg_start = url_end_in_chunk;
            }
        }

        if seg_start < chunk_end {
            display_segments.push((seg_start, chunk_end, false));
        }

        if display_segments.is_empty() {
            display_segments.push((chunk_start, chunk_end, false));
        }

        display_segments
    }

    #[allow(clippy::too_many_arguments)]
    fn render_wrapped_chunk(
        frame: &mut canvas::Frame,
        segments: &[crate::ansi::TextSegment],
        chars: &[char],
        line_urls: &[UrlSpan],
        chunk_start: usize,
        chunk_end: usize,
        y_aligned: f32,
        char_width: f32,
        url_color: Color,
    ) {
        let mut x_offset = 0.0;
        let mut char_offset = chunk_start;

        for (seg_start, seg_end, is_url) in
            Self::display_segments_for_chunk(line_urls, chunk_start, chunk_end)
        {
            let mut current_pos = char_offset;
            let seg_len = seg_end - seg_start;

            for color_segment in segments {
                let segment_len = color_segment.text.chars().count();

                if current_pos < seg_start + seg_len && current_pos + segment_len > seg_start {
                    let overlap_start = seg_start.max(current_pos);
                    let overlap_end = seg_end.min(current_pos + segment_len);

                    if overlap_end > overlap_start {
                        let overlap_text: String =
                            chars[overlap_start..overlap_end].iter().collect();
                        x_offset += Self::draw_text_run(
                            frame,
                            color_segment,
                            &overlap_text,
                            is_url,
                            url_color,
                            y_aligned,
                            x_offset,
                            char_width,
                        );
                    }
                }

                current_pos += segment_len;
                if current_pos >= seg_end {
                    break;
                }
            }

            char_offset = seg_end;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text_run(
        frame: &mut canvas::Frame,
        color_segment: &crate::ansi::TextSegment,
        text: &str,
        is_url: bool,
        url_color: Color,
        y_aligned: f32,
        x_offset: f32,
        char_width: f32,
    ) -> f32 {
        let text_color = if is_url {
            url_color
        } else {
            color_segment.foreground.unwrap_or(TEXT_COLOR)
        };
        let text_position = Point::new(
            HORIZONTAL_PADDING + x_offset,
            y_aligned + HORIZONTAL_PADDING,
        );

        Self::fill_terminal_text(frame, text, text_position, text_color, color_segment.bold);

        if color_segment.underline {
            Self::draw_underline(frame, text, text_color, y_aligned, x_offset, char_width);
        }

        Self::line_display_width_f32(text) * char_width
    }

    fn fill_terminal_text(
        frame: &mut canvas::Frame,
        text: &str,
        position: Point,
        color: Color,
        is_bold: bool,
    ) {
        let base_text = canvas::Text {
            content: text.to_string(),
            position,
            color,
            size: FONT_SIZE.into(),
            font: FONT,
            line_height: iced::widget::text::LineHeight::Absolute(LINE_HEIGHT.into()),
            ..canvas::Text::default()
        };

        frame.fill_text(base_text.clone());

        if is_bold {
            frame.fill_text(canvas::Text {
                position: Point::new(position.x + 0.5, position.y),
                ..base_text
            });
        }
    }

    fn draw_underline(
        frame: &mut canvas::Frame,
        text: &str,
        text_color: Color,
        y_aligned: f32,
        x_offset: f32,
        char_width: f32,
    ) {
        let underline_width = Self::line_display_width_f32(text) * char_width;
        let underline_y = y_aligned + HORIZONTAL_PADDING + LINE_HEIGHT - 2.0;

        let underline_path = canvas::Path::line(
            Point::new(HORIZONTAL_PADDING + x_offset, underline_y),
            Point::new(HORIZONTAL_PADDING + x_offset + underline_width, underline_y),
        );
        frame.stroke(
            &underline_path,
            canvas::Stroke {
                style: canvas::Style::Solid(text_color),
                width: 1.0,
                ..canvas::Stroke::default()
            },
        );
    }
}

// ============================================================================
// Public API
// ============================================================================

/// 터미널 뷰의 wrap 캐시 무효화 키. `prepare_lines`가 만드는 줄 집합이 바뀌는 입력만
/// 반영한다: 콘텐츠(content_version)와 필터 모드, 그리고 필터 중일 때의 검색어. 검색
/// 하이라이트(매치/현재 매치)는 줄바꿈 행 수에 영향을 주지 않으므로 키에서 제외한다.
/// (필터 OFF면 줄 집합이 검색어와 무관 — 검색어는 하이라이트만 바꾸므로 키에 넣지 않는다.)
fn render_key_for(session: &RunSession) -> RenderKey {
    let (filter, query) = match session.search.as_ref() {
        Some(search) if search.filter => (true, search.query.clone()),
        _ => (false, String::new()),
    };
    RenderKey {
        content_version: session.content_version,
        filter,
        query,
    }
}

/// 특정 세션의 터미널 뷰 렌더링 (세션 참조 기반)
///
/// Pane 시스템에서 사용하기 위한 헬퍼 함수
/// 세션 인덱스 대신 세션 참조를 직접 받아 터미널을 렌더링
///
/// # Arguments
/// * `session` - 렌더링할 세션의 참조
pub fn view_terminal_for_session(session: &RunSession, is_dragging: bool) -> Element<'_, Message> {
    let (lines, highlights) = prepare_lines(session);

    let canvas = Canvas::new(TerminalCanvas {
        lines,
        highlights,
        session_id: session.id,
        initial_scroll_progress: session.scroll_progress,
        auto_scroll: session.auto_scroll,
        scroll_target: session.scroll_target,
        render_key: render_key_for(session),
    })
    .width(Length::Fill)
    .height(Length::Fill);

    container(canvas)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(HORIZONTAL_PADDING)
        .style(move |theme: &Theme| {
            let bg_color = theme.extended_palette().background.weaker.color;
            let background = if is_dragging {
                iced::Color {
                    r: bg_color.r,
                    g: bg_color.g,
                    b: bg_color.b,
                    a: 0.5,
                }
            } else {
                bg_color
            };
            container::Style {
                background: Some(background.into()),
                border: Border {
                    radius: border::Radius::default().bottom(4.0),
                    ..Border::default()
                },
                ..container::Style::default()
            }
        })
        .into()
}

/// 세션에서 표시할 라인 + 라인별 검색 하이라이트 종류 준비.
///
/// 매칭은 대소문자 무시 부분일치(라인 단위). 필터 모드면 매치 라인만, 아니면 보관된 모든
/// 줄을 표시한다(버퍼가 MAX_OUTPUT_LINES로 상한되고 가상화로 가시 영역만 그리므로 별도
/// 렌더 상한/"older lines hidden" 헤더는 없다). 줄은 세션 버퍼를 빌려(zero-copy) 전달하며,
/// 각 출력 라인의 위치 인덱스로 매치/현재 매치를 판정해 `LineHighlight`를 부여한다.
fn prepare_lines(session: &RunSession) -> (Vec<LineRef<'_>>, Vec<LineHighlight>) {
    use crate::ansi::TextSegment;

    let search = session.search.as_ref();
    let query = search.map_or("", |s| s.query.as_str());
    let active = search.is_some() && !query.is_empty();
    let filter = search.is_some_and(|s| s.filter);

    // 매치 위치 인덱스는 app의 update에서 갱신된 캐시(search.matches)를 읽는다
    // (매 프레임 재스캔 방지). 빈 검색어면 캐시도 비어 있다.
    let matches: &[usize] = search.map_or(&[], |s| s.matches.as_slice());
    let match_set: HashSet<usize> = matches.iter().copied().collect();
    let current_output_idx: Option<usize> = if matches.is_empty() {
        None
    } else {
        let cur = search.map_or(0, |s| s.current).min(matches.len() - 1);
        Some(matches[cur])
    };
    let highlight_for = |output_idx: usize| -> LineHighlight {
        if Some(output_idx) == current_output_idx {
            LineHighlight::Current
        } else if match_set.contains(&output_idx) {
            LineHighlight::Match
        } else {
            LineHighlight::None
        }
    };

    let mut lines: Vec<LineRef<'_>> = Vec::new();
    let mut highlights = Vec::new();

    if active && filter {
        // 필터 모드: 매치 라인만 (출력 인덱스 유지로 현재 매치 강조). 매치가 없으면 안내
        // 줄(버퍼에 없는 합성 줄)만 Owned로 추가한다.
        if matches.is_empty() {
            lines.push(LineRef::Owned(vec![TextSegment::new(format!(
                "No lines match \"{query}\""
            ))]));
            highlights.push(LineHighlight::None);
        }
        for &output_idx in matches {
            if let Some((_, segments)) = session.output_lines.get(output_idx) {
                lines.push(LineRef::Ref(segments));
                highlights.push(highlight_for(output_idx));
            }
        }
        return (lines, highlights);
    }

    // 일반 모드: 보관된 모든 줄을 빌려서 그대로 표시(zero-copy). 출력 인덱스로 하이라이트 판정.
    for (output_idx, (_, segments)) in session.output_lines.iter().enumerate() {
        lines.push(LineRef::Ref(segments));
        highlights.push(highlight_for(output_idx));
    }

    (lines, highlights)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::TextSegment;
    use iced::Size;
    use iced::widget::canvas::Program as _;

    /// 테스트용 캔버스: `n`개의 짧은 라인으로 구성 (래핑 없음 → total_wrapped == n).
    /// 합성(Owned) 줄로 만들어 세션 borrow 없이 'static 수명을 갖는다.
    fn canvas_with_lines(n: usize, auto_scroll: bool) -> (TerminalCanvas<'static>, Uuid) {
        let id = Uuid::new_v4();
        let lines: Vec<LineRef<'static>> = (0..n)
            .map(|i| LineRef::Owned(vec![TextSegment::new(format!("line {i}"))]))
            .collect();
        let highlights = vec![LineHighlight::None; n];
        (
            TerminalCanvas {
                lines,
                highlights,
                session_id: id,
                initial_scroll_progress: 1.0,
                auto_scroll,
                scroll_target: None,
                render_key: RenderKey::default(),
            },
            id,
        )
    }

    /// 초기화가 끝난(세션 전환 감지 통과) 스크롤 상태.
    fn ready_state(id: Uuid) -> ScrollState {
        ScrollState {
            last_session_id: Some(id),
            is_initialized: true,
            ..ScrollState::default()
        }
    }

    fn test_bounds() -> Rectangle {
        Rectangle::new(Point::new(0.0, 0.0), Size::new(800.0, 400.0))
    }

    fn cursor_inside() -> mouse::Cursor {
        mouse::Cursor::Available(Point::new(400.0, 200.0))
    }

    /// 회귀 방지(C1 스크롤 락): auto_scroll=true이고 출력이 흐르는 중(offset < max_scroll)에도
    /// 휠 이벤트가 apply_auto_scroll에 선점되지 않고 처리되어, offset이 맨 아래로 스냅되지
    /// 않아야 한다. 과거엔 update 선두의 apply_auto_scroll가 매 입력을 선점해 스크롤이 막혔다.
    #[test]
    fn wheel_is_not_preempted_by_auto_scroll_during_streaming() {
        let (canvas, id) = canvas_with_lines(100, true);
        let bounds = test_bounds();
        let mut state = ready_state(id);
        let max_scroll = canvas.scroll_metrics(&state, bounds).max_scroll;
        assert!(max_scroll > 0.0, "테스트 전제: 스크롤 가능한 버퍼여야 함");

        state.offset = max_scroll * 0.5; // 중간 위치(맨 아래 아님) → 과거엔 선점 대상

        // 휠 위로 스크롤 (Lines{y:1.0} → delta = -SCROLL_SPEED → offset 감소)
        let wheel = Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
        });
        let _ = canvas.update(&mut state, &wheel, bounds, cursor_inside());

        assert!(
            (state.offset - max_scroll).abs() > 0.01,
            "휠이 선점되면 offset이 max_scroll로 스냅된다 — 선점되지 않아야 함 (offset={}, max={max_scroll})",
            state.offset
        );
        assert!(
            state.offset < max_scroll * 0.5,
            "휠 위로 스크롤 시 offset이 감소해야 함 (offset={})",
            state.offset
        );
    }

    /// auto_scroll=true일 때 매 프레임 RedrawRequested는 offset을 맨 아래로 재고정해야 한다
    /// (꼬리 추적이 입력 라우팅과 분리된 뒤에도 정상 동작하는지 확인).
    #[test]
    fn redraw_repins_to_bottom_when_auto_scroll() {
        let (canvas, id) = canvas_with_lines(100, true);
        let bounds = test_bounds();
        let mut state = ready_state(id);
        let max_scroll = canvas.scroll_metrics(&state, bounds).max_scroll;

        state.offset = 0.0; // 맨 위

        let redraw = Event::Window(window::Event::RedrawRequested(std::time::Instant::now()));
        let action = canvas.update(&mut state, &redraw, bounds, cursor_inside());

        assert!(action.is_some(), "재고정 시 redraw를 요청해야 함");
        assert!(
            (state.offset - max_scroll).abs() <= 0.01,
            "RedrawRequested + auto_scroll → 맨 아래 재고정 (offset={}, max={max_scroll})",
            state.offset
        );
    }

    /// auto_scroll=false(사용자가 위로 스크롤해 자동 추적을 끈 상태)면 RedrawRequested가
    /// offset을 건드리지 않아야 한다 ("시작부터 위로 스크롤된 채 폭주" 케이스 포함).
    #[test]
    fn redraw_does_not_repin_when_auto_scroll_disabled() {
        let (canvas, id) = canvas_with_lines(100, false);
        let bounds = test_bounds();

        let mut state = ready_state(id);
        state.offset = 10.0;

        let redraw = Event::Window(window::Event::RedrawRequested(std::time::Instant::now()));
        let _ = canvas.update(&mut state, &redraw, bounds, cursor_inside());

        assert_eq!(state.offset, 10.0, "auto_scroll=false면 재고정하지 않아야 함");
    }

    /// 회귀 방지(이슈 2): auto_scroll 해제는 비율이 아닌 절대 거리로 판정해야 한다. 대량
    /// 버퍼에서 비율 임계치(과거 0.99)는 바닥 근처 수백 줄을 모두 "바닥"으로 오판해, 위로
    /// 스크롤해도 자동 추적이 풀리지 않았다.
    #[test]
    fn at_bottom_uses_absolute_distance_not_ratio() {
        let max_scroll = 50_000.0; // 5만 래핑 행

        assert!(
            TerminalCanvas::is_at_bottom(max_scroll, max_scroll),
            "정확히 바닥은 at_bottom"
        );
        assert!(
            TerminalCanvas::is_at_bottom(max_scroll - 0.4, max_scroll),
            "0.4행 위는 바닥으로 간주(부동소수점 여유)"
        );
        assert!(
            !TerminalCanvas::is_at_bottom(max_scroll - 1.0, max_scroll),
            "1행만 위로 올려도 바닥 아님 → auto_scroll 해제"
        );
        assert!(
            !TerminalCanvas::is_at_bottom(max_scroll - 500.0, max_scroll),
            "500행 위(비율로는 progress=0.99 → 과거 '바닥' 오판 구간)는 바닥 아님"
        );
        assert!(
            TerminalCanvas::is_at_bottom(0.0, 0.0),
            "스크롤 불가 버퍼는 항상 바닥"
        );
    }

    /// 회귀 방지(이슈 1): 검색 점프(scroll_target)는 논리줄 인덱스를 wrapped offset으로
    /// 변환해 실제 offset에 반영해야 한다. 과거엔 scroll_progress만 바뀌고 offset에 반영되는
    /// 경로가 없어 화면이 움직이지 않았다.
    #[test]
    fn scroll_target_jumps_to_logical_line_offset() {
        // 짧은 줄(래핑 없음) 200개 → 줄당 1행이므로 논리줄 인덱스 == wrapped offset.
        let (mut canvas, id) = canvas_with_lines(200, false);
        let bounds = test_bounds();
        let mut state = ready_state(id);

        let max_scroll = canvas.scroll_metrics(&state, bounds).max_scroll;
        assert!(max_scroll > 10.0, "테스트 전제: 10행 위로 점프 가능한 버퍼");

        canvas.scroll_target = Some(10);
        let redraw = Event::Window(window::Event::RedrawRequested(std::time::Instant::now()));
        let action = canvas.update(&mut state, &redraw, bounds, cursor_inside());

        assert!(
            action.is_some(),
            "점프 시 SessionScrollChanged를 발행해 app측 목표를 클리어해야 함"
        );
        assert!(
            (state.offset - 10.0).abs() < 1e-6,
            "scroll_target=10 → offset 10.0 (offset={})",
            state.offset
        );
    }

    /// 회귀 방지(이슈 1): 줄이 여러 행으로 래핑될 때, scroll_target은 단순 논리줄 인덱스가
    /// 아니라 앞쪽 줄들의 누적 래핑 행 수(line_counts 합)로 변환돼야 한다. 1:1 케이스만으론
    /// 변환 로직(논리줄→wrapped offset)이 검증되지 않는다.
    #[test]
    fn scroll_target_jumps_to_wrapped_offset_with_multiline_wrapping() {
        let id = Uuid::new_v4();
        // test_bounds(800px 폭)에서 여러 행으로 래핑되는 긴 줄 30개.
        let long = "x".repeat(600);
        let lines: Vec<LineRef<'static>> = (0..30)
            .map(|_| LineRef::Owned(vec![TextSegment::new(long.clone())]))
            .collect();
        let highlights = vec![LineHighlight::None; 30];
        let canvas = TerminalCanvas {
            lines,
            highlights,
            session_id: id,
            initial_scroll_progress: 0.0,
            auto_scroll: false,
            scroll_target: Some(3),
            render_key: RenderKey::default(),
        };
        let bounds = test_bounds();
        let mut state = ready_state(id);

        let redraw = Event::Window(window::Event::RedrawRequested(std::time::Instant::now()));
        let _ = canvas.update(&mut state, &redraw, bounds, cursor_inside());

        // 모든 줄이 동일 길이 → 줄당 래핑 행 수 동일. 1보다 커야(실제 래핑) 의미가 있다.
        let per_line = state.line_counts[0] as f32;
        assert!(
            per_line > 1.0,
            "테스트 전제: 줄이 여러 행으로 래핑돼야 함 (per_line={per_line})"
        );
        let max_scroll = canvas.scroll_metrics(&state, bounds).max_scroll;
        let expected = state.line_counts[..3]
            .iter()
            .map(|&c| c as f32)
            .sum::<f32>()
            .clamp(0.0, max_scroll);
        assert!(
            (state.offset - expected).abs() < 1e-3,
            "scroll_target=3 → 앞 3줄 누적 래핑 행 offset (offset={}, expected={expected})",
            state.offset
        );
    }

    // ── S1/S3: 가상화 wrap 수학 (순수 함수) ─────────────────────────────────

    #[test]
    fn wrapped_total_sums_counts() {
        assert_eq!(TerminalCanvas::wrapped_total(&[]), 0);
        assert_eq!(TerminalCanvas::wrapped_total(&[1, 1, 1]), 3);
        assert_eq!(TerminalCanvas::wrapped_total(&[1, 3, 2, 1]), 7);
    }
}
