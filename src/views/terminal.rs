use crate::messages::Message;
use crate::models::RunSession;
use iced::{
    Border, Color, Element, Event, Length, Point, Rectangle, Renderer, Theme, border, keyboard,
    mouse,
    widget::{Canvas, canvas, container},
};
use unicode_width::UnicodeWidthChar;
use uuid::Uuid;

/// D2 Coding 폰트 사용
const FONT: iced::Font = crate::D2CODING;

/// D2 Coding 폰트 데이터
const D2CODING_FONT_DATA: &[u8] = include_bytes!("../../fonts/D2Coding.ttf");

// ============================================================================
// Constants
// ============================================================================

/// 렌더링할 최대 라인 수 (메모리 관리)
const RENDER_LINE_LIMIT: usize = 500;

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
    *CHAR_WIDTH.get_or_init(|| TerminalCanvas::calculate_char_width(FONT_SIZE))
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

/// URL 정보
#[derive(Debug, Clone)]
struct UrlInfo {
    /// 라인 인덱스
    line_idx: usize,
    /// URL 시작 문자 인덱스
    start_char: usize,
    /// URL 끝 문자 인덱스
    end_char: usize,
    /// URL 문자열
    url: String,
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
struct TerminalCanvas {
    lines: Vec<Vec<crate::ansi::TextSegment>>,
    session_id: Uuid,
    initial_scroll_progress: f32,
    auto_scroll: bool,
    urls: Vec<UrlInfo>,
}

impl TerminalCanvas {
    fn usize_to_f32(value: usize) -> f32 {
        let value = u16::try_from(value).unwrap_or(u16::MAX);
        f32::from(value)
    }

    fn f32_floor_to_usize(value: f32) -> usize {
        if !value.is_finite() || value <= 0.0 {
            return 0;
        }

        value.floor().to_string().parse::<usize>().unwrap_or(0)
    }

    fn line_display_width_f32(text: &str) -> f32 {
        Self::usize_to_f32(Self::line_display_width(text))
    }

    // ============================================================================
    // URL 감지 및 처리
    // ============================================================================

    /// 텍스트에서 URL 추출
    ///
    /// RFC 3986 표준에 따라 URL 패턴을 감지
    /// - http://, https://, localhost 패턴 지원
    /// - RFC 3986 금지 문자 제외: `<`, `>`, `"`, `{`, `}`, `|`, `\`, `^`, `` ` ``
    /// - Trailing punctuation 자동 제거
    fn extract_urls(lines: &[Vec<crate::ansi::TextSegment>]) -> Vec<UrlInfo> {
        let mut urls = Vec::new();

        // RFC 3986 기반 URL 패턴
        // 금지 문자: whitespace, < > " { } | \ ^ ` (RFC 3986 Section 2.4)
        let url_pattern =
            regex::Regex::new(r#"https?://[^\s<>"{}|\\^`]+|localhost:\d+[^\s<>"{}|\\^`]*"#)
                .unwrap();

        for (line_idx, segments) in lines.iter().enumerate() {
            // 세그먼트들을 하나의 문자열로 결합
            let line: String = segments.iter().map(|s| s.text.as_str()).collect();

            for mat in url_pattern.find_iter(&line) {
                let raw_url = mat.as_str();

                // Trailing punctuation 제거
                // URL이 문장 끝에 올 때 마침표, 쉼표 등이 포함되는 것을 방지
                let cleaned_url = Self::trim_trailing_punctuation(raw_url);

                if cleaned_url.is_empty() {
                    continue;
                }

                let start_char = line[..mat.start()].chars().count();
                let end_char = start_char + cleaned_url.chars().count();

                urls.push(UrlInfo {
                    line_idx,
                    start_char,
                    end_char,
                    url: cleaned_url.to_string(),
                });
            }
        }

        urls
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
        self.urls
            .iter()
            .find(|url| {
                url.line_idx == pos.line_idx
                    && pos.char_idx >= url.start_char
                    && pos.char_idx < url.end_char
            })
            .map(|url| url.url.clone())
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

    // ============================================================================
    // 폰트 메트릭
    // ============================================================================

    /// D2 Coding 폰트의 정확한 문자 너비 계산
    ///
    /// ttf-parser를 사용하여 폰트 파일에서 직접 glyph 너비를 읽어 픽셀 단위로 변환
    ///
    /// # Arguments
    /// * `font_size` - 폰트 크기 (픽셀)
    ///
    /// # Returns
    /// 실제 문자 너비 (픽셀)
    fn calculate_char_width(font_size: f32) -> f32 {
        use ttf_parser::{Face, GlyphId};

        // D2 Coding 폰트 파싱
        let Ok(face) = Face::parse(D2CODING_FONT_DATA, 0) else {
            // 파싱 실패 시 기본값 사용 (FONT_SIZE * 0.6)
            return font_size * 0.6;
        };

        // 'M' 문자의 glyph ID 찾기 (모노스페이스 폰트의 표준 너비)
        let glyph_id = face.glyph_index('M').unwrap_or(GlyphId(0));

        // glyph advance width 가져오기 (폰트 유닛)
        let advance_width = f32::from(face.glyph_hor_advance(glyph_id).unwrap_or(0));

        // 폰트 유닛을 픽셀로 변환
        // pixels = (advance_width / units_per_em) * font_size
        let units_per_em = f32::from(face.units_per_em());

        (advance_width / units_per_em) * font_size
    }

    /// 화면 너비에 맞게 줄바꿈했을 때의 총 렌더링 줄 수 계산
    ///
    /// # Arguments
    /// * `available_width` - 렌더링 가능한 화면 너비 (픽셀)
    /// * `char_width` - 실제 측정된 문자 너비
    ///
    /// # Returns
    /// 줄바꿈을 적용한 총 줄 수
    fn calculate_total_wrapped_lines(&self, available_width: f32, char_width: f32) -> usize {
        if available_width <= 0.0 || char_width <= 0.0 {
            return self.lines.len();
        }

        let max_chars = Self::calculate_max_chars(available_width, char_width);
        if max_chars == 0 {
            return self.lines.len();
        }

        self.lines
            .iter()
            .map(|line| Self::calculate_wrapped_count(line, max_chars))
            .sum()
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
        if display_width == 0 {
            1
        } else {
            display_width.div_ceil(max_display_width)
        }
    }
}

impl canvas::Program<Message> for TerminalCanvas {
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
                self.calculate_total_wrapped_lines(available_width, char_width);

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
        if let Some(action) = self.initialize_scroll_state(state, bounds) {
            return Some(action);
        }

        let metrics = self.scroll_metrics(bounds);

        if let Some(action) = self.apply_auto_scroll(state, metrics) {
            return Some(action);
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
        let char_width = Self::calculate_char_width(FONT_SIZE);

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

        // Render selection highlight (behind text, but above background)
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

impl TerminalCanvas {
    /// 스크롤바의 위치와 크기 계산
    ///
    /// # Returns
    /// (x, y, width, height)
    fn calculate_scrollbar_bounds(
        &self,
        bounds: Rectangle,
        available_width: f32,
        visible_lines: usize,
        offset: f32,
    ) -> (f32, f32, f32, f32) {
        let total_wrapped_lines =
            self.calculate_total_wrapped_lines(available_width, get_char_width());
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

    fn scroll_metrics(&self, bounds: Rectangle) -> ScrollMetrics {
        let available_width = bounds.width - (HORIZONTAL_PADDING * 2.0);
        let visible_lines_f = Self::calculate_visible_lines(bounds.height);
        let visible_lines = Self::f32_floor_to_usize(visible_lines_f);
        let total_wrapped_lines =
            self.calculate_total_wrapped_lines(available_width, get_char_width());
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

        let metrics = self.scroll_metrics(bounds);
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

    fn publish_scroll_progress(&self, offset: f32, max_scroll: f32) -> canvas::Action<Message> {
        canvas::Action::publish(Message::SessionScrollChanged(
            self.session_id,
            Self::scroll_progress(offset, max_scroll),
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
            self.calculate_total_wrapped_lines(available_width, get_char_width());

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

        for (line_idx, segments_vec) in self.lines.iter().enumerate() {
            // 세그먼트들을 문자열로 결합
            let line: String = segments_vec.iter().map(|s| s.text.as_str()).collect();
            let line_len = line.chars().count();
            let wrapped_count = Self::calculate_wrapped_count(segments_vec, max_chars);

            // 가시성 체크: 완전히 위에 있으면 스킵 (render_lines와 동일)
            if current_wrapped_line + wrapped_count <= target_start_line {
                current_wrapped_line += wrapped_count;
                continue;
            }

            // 가시성 체크: 완전히 아래에 있으면 종료 (render_lines와 동일)
            if current_wrapped_line > target_start_line + visible_lines {
                break;
            }

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

        for (line_idx, segments_vec) in self.lines.iter().enumerate() {
            // 세그먼트들을 문자열로 결합
            let line: String = segments_vec.iter().map(|s| s.text.as_str()).collect();
            let wrapped_count = Self::calculate_wrapped_count(segments_vec, max_chars);
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

        for (line_idx, segments) in self.lines.iter().enumerate() {
            // 세그먼트들을 문자열로 결합하여 문자 수 계산
            let line_text: String = segments.iter().map(|s| s.text.as_str()).collect();
            let char_count = line_text.chars().count();
            let wrapped_count = Self::calculate_wrapped_count(segments, max_chars);

            // 가시성 체크: 완전히 위에 있으면 스킵
            if current_wrapped_line + wrapped_count <= target_start_line {
                current_wrapped_line += wrapped_count;
                continue;
            }

            // 가시성 체크: 완전히 아래에 있으면 종료
            if current_wrapped_line > target_start_line + visible_lines {
                break;
            }

            // 라인 렌더링
            if char_count == 0 {
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
                    line_idx,
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
        line_idx: usize,
        max_display_width: usize,
        current_wrapped_line: usize,
        target_start_line: usize,
        visible_lines: usize,
        rendered_lines: &mut usize,
        partial_offset: f32,
        url_color: Color,
    ) {
        let line_urls = self.line_urls(line_idx);
        let line: String = segments.iter().map(|s| s.text.as_str()).collect();
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
            self.calculate_total_wrapped_lines(available_width, get_char_width());

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

    fn line_urls(&self, line_idx: usize) -> Vec<&UrlInfo> {
        self.urls
            .iter()
            .filter(|url| url.line_idx == line_idx)
            .collect()
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
        line_urls: &[&UrlInfo],
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
        line_urls: &[&UrlInfo],
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

/// 특정 세션의 터미널 뷰 렌더링 (세션 참조 기반)
///
/// Pane 시스템에서 사용하기 위한 헬퍼 함수
/// 세션 인덱스 대신 세션 참조를 직접 받아 터미널을 렌더링
///
/// # Arguments
/// * `session` - 렌더링할 세션의 참조
pub fn view_terminal_for_session(session: &RunSession, is_dragging: bool) -> Element<'_, Message> {
    let lines = prepare_lines(session);
    let urls = TerminalCanvas::extract_urls(&lines);

    let canvas = Canvas::new(TerminalCanvas {
        lines,
        session_id: session.id,
        initial_scroll_progress: session.scroll_progress,
        auto_scroll: session.auto_scroll,
        urls,
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

/// 세션에서 표시할 라인 준비
fn prepare_lines(session: &RunSession) -> Vec<Vec<crate::ansi::TextSegment>> {
    let total_lines = session.output_lines.len();

    let start_idx = total_lines.saturating_sub(RENDER_LINE_LIMIT);

    let mut lines = Vec::new();

    // 숨겨진 라인 정보 표시
    if total_lines > RENDER_LINE_LIMIT {
        let hidden = total_lines - RENDER_LINE_LIMIT;
        let info_line = format!("... {hidden} older lines hidden (total: {total_lines} lines)");
        lines.push(vec![crate::ansi::TextSegment::new(info_line)]);
        lines.push(vec![crate::ansi::TextSegment::new(String::new())]);
    }

    // 최근 라인 추가
    for (_, segments) in session.output_lines.iter().skip(start_idx) {
        lines.push(segments.clone());
    }

    lines
}
