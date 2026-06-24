use iced::Color;

/// ANSI 색상 코드를 파싱하여 색상이 적용된 텍스트 세그먼트로 변환
///
/// ANSI 이스케이프 시퀀스를 파싱하여 각 텍스트 조각에 해당하는 색상을 추출
/// 텍스트 세그먼트 - 색상 정보와 텍스트를 포함
#[derive(Debug, Clone, PartialEq)]
pub struct TextSegment {
    /// 텍스트 내용
    pub text: String,
    /// 전경색 (텍스트 색상)
    pub foreground: Option<Color>,
    /// 배경색
    pub background: Option<Color>,
    /// 볼드 여부
    pub bold: bool,
    /// 이탤릭 여부
    pub italic: bool,
    /// 밑줄 여부
    pub underline: bool,
}

impl TextSegment {
    /// 기본 텍스트 세그먼트 생성
    pub fn new(text: String) -> Self {
        Self {
            text,
            foreground: None,
            background: None,
            bold: false,
            italic: false,
            underline: false,
        }
    }

    /// 스타일이 적용된 텍스트 세그먼트 생성
    pub fn with_style(
        text: String,
        foreground: Option<Color>,
        background: Option<Color>,
        bold: bool,
        italic: bool,
        underline: bool,
    ) -> Self {
        Self {
            text,
            foreground,
            background,
            bold,
            italic,
            underline,
        }
    }
}

/// ANSI 색상 상태 - 파싱 과정에서 현재 스타일을 추적
#[derive(Debug, Clone, Default)]
struct AnsiState {
    foreground: Option<Color>,
    background: Option<Color>,
    bold: bool,
    italic: bool,
    underline: bool,
}

impl AnsiState {
    /// 현재 상태를 리셋 (SGR 0 코드)
    fn reset(&mut self) {
        self.foreground = None;
        self.background = None;
        self.bold = false;
        self.italic = false;
        self.underline = false;
    }
}

/// ANSI 색상 코드를 Color로 변환
///
/// 표준 8색과 256색 팔레트를 지원
fn ansi_color_to_color(code: u8, is_bright: bool) -> Color {
    match code {
        30 | 40 => Color::from_rgb(0.0, 0.0, 0.0), // Black
        31 | 41 => {
            if is_bright {
                Color::from_rgb(1.0, 0.3, 0.3) // Bright Red
            } else {
                Color::from_rgb(0.8, 0.0, 0.0) // Red
            }
        }
        32 | 42 => {
            if is_bright {
                Color::from_rgb(0.3, 1.0, 0.3) // Bright Green
            } else {
                Color::from_rgb(0.0, 0.8, 0.0) // Green
            }
        }
        33 | 43 => {
            if is_bright {
                Color::from_rgb(1.0, 1.0, 0.3) // Bright Yellow
            } else {
                Color::from_rgb(0.8, 0.8, 0.0) // Yellow
            }
        }
        34 | 44 => {
            if is_bright {
                Color::from_rgb(0.3, 0.3, 1.0) // Bright Blue
            } else {
                Color::from_rgb(0.0, 0.0, 0.8) // Blue
            }
        }
        35 | 45 => {
            if is_bright {
                Color::from_rgb(1.0, 0.3, 1.0) // Bright Magenta
            } else {
                Color::from_rgb(0.8, 0.0, 0.8) // Magenta
            }
        }
        36 | 46 => {
            if is_bright {
                Color::from_rgb(0.3, 1.0, 1.0) // Bright Cyan
            } else {
                Color::from_rgb(0.0, 0.8, 0.8) // Cyan
            }
        }
        37 | 47 => {
            if is_bright {
                Color::from_rgb(1.0, 1.0, 1.0) // Bright White
            } else {
                Color::from_rgb(0.75, 0.75, 0.75) // White
            }
        }
        90 | 100 => Color::from_rgb(0.5, 0.5, 0.5), // Bright Black (Gray)
        91 | 101 => Color::from_rgb(1.0, 0.3, 0.3), // Bright Red
        92 | 102 => Color::from_rgb(0.3, 1.0, 0.3), // Bright Green
        93 | 103 => Color::from_rgb(1.0, 1.0, 0.3), // Bright Yellow
        94 | 104 => Color::from_rgb(0.3, 0.3, 1.0), // Bright Blue
        95 | 105 => Color::from_rgb(1.0, 0.3, 1.0), // Bright Magenta
        96 | 106 => Color::from_rgb(0.3, 1.0, 1.0), // Bright Cyan
        97 | 107 => Color::from_rgb(1.0, 1.0, 1.0), // Bright White
        _ => Color::WHITE,                          // Default
    }
}

/// 256색 팔레트 색상을 Color로 변환
fn color_256_to_color(idx: u8) -> Color {
    match idx {
        0..=15 => {
            // 기본 16색
            let is_bright = idx >= 8;
            let base_code = if idx >= 8 { 82 + idx } else { 30 + idx };
            ansi_color_to_color(base_code, is_bright)
        }
        16..=231 => {
            // 6x6x6 RGB 큐브
            let idx = idx - 16;
            let r = idx / 36;
            let g = (idx / 6) % 6;
            let b = idx % 6;
            Color::from_rgb(f32::from(r) / 5.0, f32::from(g) / 5.0, f32::from(b) / 5.0)
        }
        232..=255 => {
            // 그레이스케일
            let level = f32::from(idx - 232) / 23.0;
            Color::from_rgb(level, level, level)
        }
    }
}

/// ANSI 이스케이프 시퀀스를 파싱하여 텍스트 세그먼트 벡터로 변환
///
/// # Arguments
/// * `text` - 파싱할 텍스트 (ANSI 이스케이프 시퀀스 포함)
///
/// # Returns
/// 색상 정보가 포함된 텍스트 세그먼트 벡터
pub fn parse_ansi_text(text: &str) -> Vec<TextSegment> {
    let mut segments = Vec::new();
    let mut current_text = String::new();
    let mut state = AnsiState::default();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // ESC 문자를 만나면 ANSI 시퀀스 처리
            if chars.peek() == Some(&'[') {
                chars.next(); // '[' 소비

                // 현재까지의 텍스트를 세그먼트로 추가
                if !current_text.is_empty() {
                    segments.push(TextSegment::with_style(
                        current_text.clone(),
                        state.foreground,
                        state.background,
                        state.bold,
                        state.italic,
                        state.underline,
                    ));
                    current_text.clear();
                }

                // CSI (Control Sequence Introducer) 시퀀스 파싱
                let mut params = String::new();
                for next_ch in chars.by_ref() {
                    if next_ch.is_ascii_alphabetic() {
                        // 'm' 종료 문자 (SGR - Select Graphic Rendition)
                        if next_ch == 'm' {
                            parse_sgr_params(&params, &mut state);
                        }
                        break;
                    }
                    params.push(next_ch);
                }
            }
        } else {
            current_text.push(ch);
        }
    }

    // 남은 텍스트 추가
    if !current_text.is_empty() {
        segments.push(TextSegment::with_style(
            current_text,
            state.foreground,
            state.background,
            state.bold,
            state.italic,
            state.underline,
        ));
    }

    segments
}

/// SGR (Select Graphic Rendition) 파라미터 파싱
///
/// # Arguments
/// * `params` - 세미콜론으로 구분된 SGR 파라미터 문자열
/// * `state` - 업데이트할 ANSI 상태
fn parse_sgr_params(params: &str, state: &mut AnsiState) {
    let codes: Vec<u8> = params
        .split(';')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    let mut i = 0;
    while i < codes.len() {
        let code = codes[i];
        match code {
            0 => state.reset(),            // Reset
            1 => state.bold = true,        // Bold
            3 => state.italic = true,      // Italic
            4 => state.underline = true,   // Underline
            22 => state.bold = false,      // Normal intensity
            23 => state.italic = false,    // Not italic
            24 => state.underline = false, // Not underlined
            30..=37 => {
                // 전경색 (표준 8색)
                state.foreground = Some(ansi_color_to_color(code, false));
            }
            38 if i + 2 < codes.len() => {
                // 전경색 (확장)
                // 38;5;n (256색) 또는 38;2;r;g;b (RGB)
                if codes[i + 1] == 5 {
                    // 256색 팔레트
                    state.foreground = Some(color_256_to_color(codes[i + 2]));
                    i += 2;
                } else if codes[i + 1] == 2 && i + 4 < codes.len() {
                    // RGB 색상
                    let r = f32::from(codes[i + 2]) / 255.0;
                    let g = f32::from(codes[i + 3]) / 255.0;
                    let b = f32::from(codes[i + 4]) / 255.0;
                    state.foreground = Some(Color::from_rgb(r, g, b));
                    i += 4;
                }
            }
            39 => state.foreground = None, // Default foreground
            40..=47 => {
                // 배경색 (표준 8색)
                state.background = Some(ansi_color_to_color(code, false));
            }
            48 if i + 2 < codes.len() => {
                // 배경색 (확장)
                // 48;5;n (256색) 또는 48;2;r;g;b (RGB)
                if codes[i + 1] == 5 {
                    // 256색 팔레트
                    state.background = Some(color_256_to_color(codes[i + 2]));
                    i += 2;
                } else if codes[i + 1] == 2 && i + 4 < codes.len() {
                    // RGB 색상
                    let r = f32::from(codes[i + 2]) / 255.0;
                    let g = f32::from(codes[i + 3]) / 255.0;
                    let b = f32::from(codes[i + 4]) / 255.0;
                    state.background = Some(Color::from_rgb(r, g, b));
                    i += 4;
                }
            }
            49 => state.background = None, // Default background
            90..=97 => {
                // 전경색 (밝은 색상)
                state.foreground = Some(ansi_color_to_color(code, true));
            }
            100..=107 => {
                // 배경색 (밝은 색상)
                state.background = Some(ansi_color_to_color(code, true));
            }
            _ => {}
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_color_parsing() {
        let text = "\x1b[31mRed Text\x1b[0m Normal Text";
        let segments = parse_ansi_text(text);

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "Red Text");
        assert!(segments[0].foreground.is_some());
        assert_eq!(segments[1].text, " Normal Text");
        assert!(segments[1].foreground.is_none());
    }

    #[test]
    fn test_no_ansi_codes() {
        let text = "Plain text without codes";
        let segments = parse_ansi_text(text);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "Plain text without codes");
        assert!(segments[0].foreground.is_none());
    }

    #[test]
    fn test_multiple_styles() {
        let text = "\x1b[1;31mBold Red\x1b[0m";
        let segments = parse_ansi_text(text);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "Bold Red");
        assert!(segments[0].bold);
        assert!(segments[0].foreground.is_some());
    }

    #[test]
    fn test_256_color() {
        let text = "\x1b[38;5;196mBright Red\x1b[0m";
        let segments = parse_ansi_text(text);

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "Bright Red");
        assert!(segments[0].foreground.is_some());
    }
}
