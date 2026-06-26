use crate::messages::Message;
use crate::models::{
    ConfigTypeData, ConfigurationType, ExecuteMode, ExecuteModeType, KotlinLaunchMode,
    KotlinLaunchModeType, NodeCommand, PackageManager, RunConfiguration,
};
use crate::utils::{
    ICON_CLOSE, ICON_COPY, ICON_DELETE, ICON_EDIT, ICON_FOLDER_OPEN, ICON_REFRESH, to_relative_path,
};
use iced::widget::scrollable;
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding, Theme,
    alignment::{Horizontal, Vertical},
    widget::{
        Column, Space, button, center, checkbox, column, combo_box, container, opaque, row, svg,
        text, text_input,
    },
};
use std::fmt::Display;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub struct EditorLoadingState {
    pub file_dialog: FileDialogLoadingState,
    pub node: NodeLoadingState,
}

#[derive(Debug, Clone, Copy)]
pub struct FileDialogLoadingState {
    pub folder: bool,
    pub script_file: bool,
    pub interpreter: bool,
    pub kotlin_jar: bool,
    pub kotlin_jdk: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct NodeLoadingState {
    pub scripts: bool,
    pub project_directory: bool,
}

struct NodeFieldOptions {
    selected_package_json: Option<String>,
    is_loading_scripts: bool,
    is_loading_project_directory: bool,
}

pub struct EditorSelectState {
    pub config_type: combo_box::State<ConfigurationType>,
    pub execute_mode: combo_box::State<ExecuteModeType>,
    pub package_manager: combo_box::State<PackageManager>,
    pub node_command: combo_box::State<NodeCommand>,
    pub node_runtime: combo_box::State<String>,
    pub package_json: combo_box::State<String>,
    pub node_script: combo_box::State<String>,
    pub kotlin_launch_mode: combo_box::State<KotlinLaunchModeType>,
    pub jdk: combo_box::State<String>,
}

impl EditorSelectState {
    pub fn new() -> Self {
        Self {
            config_type: combo_box::State::new(ConfigurationType::ALL.to_vec()),
            execute_mode: combo_box::State::new(ExecuteModeType::ALL.to_vec()),
            package_manager: combo_box::State::new(PackageManager::ALL.to_vec()),
            node_command: combo_box::State::new(NodeCommand::all_commands()),
            node_runtime: combo_box::State::new(vec![String::from("Default (system)")]),
            package_json: combo_box::State::new(Vec::new()),
            node_script: combo_box::State::new(Vec::new()),
            kotlin_launch_mode: combo_box::State::new(KotlinLaunchModeType::ALL.to_vec()),
            jdk: combo_box::State::new(vec![String::from("Default (system)")]),
        }
    }

    pub fn set_node_runtimes(&mut self, runtimes: &[(String, String)]) {
        self.node_runtime =
            combo_box::State::new(runtimes.iter().map(|(label, _)| label.clone()).collect());
    }

    pub fn set_jdks(&mut self, jdks: &[(String, String)]) {
        self.jdk = combo_box::State::new(jdks.iter().map(|(label, _)| label.clone()).collect());
    }

    pub fn set_package_jsons(&mut self, package_jsons: Vec<String>) {
        self.package_json = combo_box::State::new(package_jsons);
    }

    pub fn set_node_scripts(&mut self, scripts: Vec<String>) {
        self.node_script = combo_box::State::new(scripts);
    }
}

impl Default for EditorSelectState {
    fn default() -> Self {
        Self::new()
    }
}

/// 구성 편집 패널 렌더링
///
/// 선택된 구성의 상세 정보를 편집할 수 있는 폼
/// 이름, 타입, 명령어, 인자, 작업 디렉토리, 환경 변수 등을 설정
///
/// # Arguments
/// * `configurations` - 전체 구성 목록
/// * `selected_config_index` - 현재 선택된 구성의 인덱스
/// * `env_bulk_text` - 환경변수 메인 input에 표시할 텍스트 (`KEY=val;...`)
/// * `loading` - 파일 다이얼로그 / 노드 로딩 상태
/// * `select_state` - 콤보박스 상태
/// * `available_node_runtimes` - 시스템에서 감지된 Node.js 런타임 목록
/// * `available_jdks` - 시스템에서 감지된 JDK 목록
///
/// # Note
/// iced 프레임워크의 view 함수는 많은 파라미터를 받는 것이 일반적인 패턴입니다.
#[allow(clippy::too_many_arguments)]
pub fn view_configuration_editor<'a>(
    configurations: &'a [RunConfiguration],
    selected_config_index: Option<usize>,
    env_bulk_text: &'a str,
    loading: EditorLoadingState,
    select_state: &'a EditorSelectState,
    available_node_runtimes: &'a [(String, String)],
    available_jdks: &'a [(String, String)],
) -> Element<'a, Message> {
    let Some(index) = selected_config_index else {
        return view_empty_configuration_editor();
    };
    let Some(config) = configurations.get(index) else {
        return view_empty_configuration_editor();
    };

    let type_specific_fields = match &config.type_data {
        ConfigTypeData::Application { command, arguments } => {
            view_application_fields(command, arguments)
        }
        ConfigTypeData::ShellScript { execute_mode } => view_shell_script_fields(
            execute_mode,
            loading.file_dialog.script_file,
            loading.file_dialog.interpreter,
            select_state,
        ),
        ConfigTypeData::Node {
            project_directory,
            package_manager,
            node_runtime_path,
            command,
            script_name,
            arguments,
            node_options,
        } => view_node_fields(
            project_directory,
            &config.working_directory,
            node_runtime_path.as_ref(),
            available_node_runtimes,
            select_state,
            package_manager,
            command,
            script_name.as_ref(),
            arguments,
            node_options,
            NodeFieldOptions {
                selected_package_json: selected_package_json(
                    project_directory,
                    &config.working_directory,
                ),
                is_loading_scripts: loading.node.scripts,
                is_loading_project_directory: loading.node.project_directory,
            },
        ),
        ConfigTypeData::Kotlin {
            jdk_path,
            launch_mode,
            vm_options,
            program_arguments,
        } => view_kotlin_fields(
            launch_mode,
            vm_options,
            program_arguments,
            jdk_path.as_ref(),
            available_jdks,
            select_state,
            loading.file_dialog.kotlin_jar,
            loading.file_dialog.kotlin_jdk,
        ),
        ConfigTypeData::Compound { members, workspace } => {
            view_compound_fields(members, workspace.as_deref(), configurations, index)
        }
    };

    let mut basics = column![
        view_name_row(&config.name),
        Space::new().height(12),
        view_type_row(config.config_type(), select_state),
        Space::new().height(12),
        type_specific_fields,
    ]
    .width(Length::Fill);

    // 작업 디렉터리는 Node(프로젝트 경로로 대체)와 Compound(멤버가 각자 보유)에서 숨긴다.
    if !matches!(
        config.config_type(),
        ConfigurationType::Node | ConfigurationType::Compound
    ) {
        basics = basics
            .push(Space::new().height(12))
            .push(view_working_directory_row(
                &config.working_directory,
                loading.file_dialog.folder,
            ));
    }

    let mut sections = column![view_section_block("Basics", basics.into())].width(Length::Fill);

    // 환경변수는 멤버 구성이 각자 보유하므로 Compound에는 표시하지 않는다.
    if !matches!(config.config_type(), ConfigurationType::Compound) {
        sections = sections
            .push(Space::new().height(16))
            .push(view_section_block(
                "Environment",
                view_environment_bulk_input(env_bulk_text),
            ));
    }

    sections
        .padding(Padding::new(0.0).right(10.0).bottom(14.0))
        .into()
}

fn view_empty_configuration_editor() -> Element<'static, Message> {
    container(
        column![
            text("Configuration Details")
                .size(17)
                .color(Color::from_rgba8(255, 255, 255, 0.82)),
            Space::new().height(8),
            text("Select a configuration from the list to edit its command,\nworking directory, and environment.")
                .size(14)
                .align_x(Horizontal::Center)
                .align_y(Vertical::Center)
                .color(Color::from_rgba8(255, 255, 255, 0.56)),
        ]
        .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

fn view_section_block<'a>(title: &'a str, content: Element<'a, Message>) -> Element<'a, Message> {
    column![
        row![
            text(title).size(12),
            Space::new().width(Length::Fixed(10.0)),
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(divider_style),
        ]
        .align_y(Alignment::Center),
        Space::new().height(12),
        container(content).width(Length::Fill).padding([2, 0]),
    ]
    .width(Length::Fill)
    .into()
}

fn view_editor_row<'a>(
    label: &'a str,
    input: Element<'a, Message>,
) -> iced::widget::Row<'a, Message> {
    row![
        container(text(label).size(12).style(editor_label_style)).width(Length::Fixed(136.0)),
        container(input).width(Length::Fill)
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .width(Length::Fill)
}

fn editor_label_style(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(Color {
            a: 0.58,
            ..theme.extended_palette().background.base.text
        }),
    }
}

fn muted_text_style(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(Color {
            a: 0.48,
            ..theme.extended_palette().background.base.text
        }),
    }
}

fn divider_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.10,
            ..theme.extended_palette().background.base.text
        })),
        ..container::Style::default()
    }
}

fn readonly_value(value: &str) -> iced::widget::Container<'_, Message> {
    container(
        scrollable(text(value).size(13).style(muted_text_style))
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new()
                    .width(4.0)
                    .spacing(2.0)
                    .scroller_width(4),
            ))
            .width(Length::Fill),
    )
    .padding([7, 10])
    .width(Length::Fill)
    .style(flat_readonly_style)
}

fn editor_input<'a>(placeholder: &'a str, value: &'a str) -> iced::widget::TextInput<'a, Message> {
    text_input(placeholder, value)
        .padding([7, 10])
        .size(14)
        .width(Length::Fill)
        .style(flat_text_input_style)
}

fn editor_combo_box<'a, T, MessageFn>(
    state: &'a combo_box::State<T>,
    placeholder: &'a str,
    selected: Option<&T>,
    on_selected: MessageFn,
) -> iced::widget::ComboBox<'a, T, Message, Theme>
where
    T: Display + Clone + 'a,
    MessageFn: Fn(T) -> Message + 'static,
{
    combo_box(state, placeholder, selected, on_selected)
        .padding([7, 10])
        .size(14)
        .width(Length::Fill)
        .input_style(flat_text_input_style)
}

fn flat_text_input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let _palette = theme.extended_palette();
    let base = Color::from_rgba8(17, 19, 24, 1.0);
    let line = Color::from_rgba8(255, 255, 255, 0.08);
    let hover_line = Color::from_rgba8(255, 255, 255, 0.14);
    let focus_line = Color::from_rgba8(96, 138, 255, 0.72);

    let border_color = match status {
        text_input::Status::Active => line,
        text_input::Status::Hovered => hover_line,
        text_input::Status::Focused { .. } => focus_line,
        text_input::Status::Disabled => Color::from_rgba8(255, 255, 255, 0.04),
    };

    text_input::Style {
        background: Background::Color(Color { a: 1.0, ..base }),
        border: Border {
            radius: 4.0.into(),
            width: 1.0,
            color: border_color,
        },
        icon: Color::from_rgba8(255, 255, 255, 0.32),
        placeholder: Color::from_rgba8(255, 255, 255, 0.30),
        value: Color::from_rgba8(255, 255, 255, 0.88),
        selection: Color::from_rgba8(95, 140, 255, 0.28),
    }
}

fn flat_readonly_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        background: Some(Background::Color(Color {
            a: 0.42,
            ..palette.background.weak.color
        })),
        border: Border {
            radius: 4.0.into(),
            width: 1.0,
            color: Color {
                a: 0.10,
                ..palette.background.base.text
            },
        },
        ..container::Style::default()
    }
}

fn flat_icon_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let subtle = Color::from_rgba8(255, 255, 255, 0.05);
    let hover = Color::from_rgba8(255, 255, 255, 0.08);
    let pressed = Color::from_rgba8(57, 84, 148, 0.78);

    let background = match status {
        button::Status::Active => None,
        button::Status::Hovered => Some(Background::Color(hover)),
        button::Status::Pressed => Some(Background::Color(pressed)),
        button::Status::Disabled => Some(Background::Color(subtle)),
    };

    button::Style {
        background,
        border: Border {
            radius: 4.0.into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        text_color: Color::from_rgba8(255, 255, 255, 0.64),
        ..button::Style::default()
    }
}

fn view_name_row(name: &str) -> iced::widget::Row<'_, Message> {
    view_editor_row(
        "Name:",
        editor_input("Configuration name", name)
            .on_input(Message::NameChanged)
            .into(),
    )
}

fn view_type_row(
    config_type: ConfigurationType,
    select_state: &EditorSelectState,
) -> iced::widget::Row<'_, Message> {
    view_editor_row(
        "Type:",
        editor_combo_box(
            &select_state.config_type,
            "Select type",
            Some(&config_type),
            Message::TypeChanged,
        )
        .into(),
    )
}

fn view_working_directory_row(
    working_directory: &str,
    is_loading_folder: bool,
) -> iced::widget::Row<'_, Message> {
    let mut browse_btn = icon_button(ICON_FOLDER_OPEN, None)
        .padding(0)
        .width(34)
        .height(34);

    if !is_loading_folder {
        browse_btn = browse_btn.on_press(Message::BrowseWorkingDirectory);
    }

    row![
        container(
            text("Working Directory:")
                .size(12)
                .style(editor_label_style)
        )
        .width(Length::Fixed(136.0)),
        editor_input("Working directory path", working_directory)
            .on_input(Message::WorkingDirectoryChanged),
        browse_btn,
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fill)
}

fn selected_package_json(project_directory: &str, working_directory: &str) -> Option<String> {
    use std::path::Path;

    if working_directory.is_empty() || working_directory == "." {
        return None;
    }

    let working_dir_path = Path::new(working_directory);
    let package_json_path = working_dir_path.join("package.json");

    if project_directory.is_empty() || project_directory == "." {
        None
    } else {
        Some(to_relative_path(
            &package_json_path,
            Path::new(project_directory),
        ))
    }
}

/// 환경변수 메인 input (한 줄 + [편집] 버튼).
///
/// IntelliJ Run/Debug Configuration의 Environment 필드와 동일한 UX.
/// 직접 타이핑 가능하며 Enter로 적용. [편집] 버튼 클릭 시 모달이 열림.
fn view_environment_bulk_input(env_bulk_text: &str) -> Element<'_, Message> {
    row![
        editor_input("KEY1=value1;KEY2=value2;...", env_bulk_text)
            .on_input(Message::EnvBulkInputChanged)
            .on_submit(Message::EnvBulkInputSubmitted)
            .width(Length::Fill),
        icon_button(ICON_EDIT, Some(Message::OpenEnvModal)),
    ]
    .spacing(5)
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

/// Compound 구성 편집: 함께 실행할 멤버를 토글로 선택.
///
/// 멤버는 동시 실행되므로 순서가 무의미하다 → 적격 구성(자기 자신·다른 Compound 제외)을
/// 나열하고 클릭으로 추가/제거하는 토글 리스트로 충분하다.
fn view_compound_fields<'a>(
    members: &'a [Uuid],
    workspace: Option<&'a str>,
    configurations: &'a [RunConfiguration],
    self_index: usize,
) -> Element<'a, Message> {
    let mut rows = Column::new().spacing(5);
    let mut eligible = 0usize;
    let mut member_count = 0usize;

    for (i, candidate) in configurations.iter().enumerate() {
        if i == self_index || matches!(candidate.type_data, ConfigTypeData::Compound { .. }) {
            continue;
        }
        eligible += 1;
        let is_member = members.contains(&candidate.id);
        if is_member {
            member_count += 1;
        }

        let toggle = if is_member {
            Message::CompoundMemberRemoved(candidate.id)
        } else {
            Message::CompoundMemberAdded(candidate.id)
        };

        let row_content = row![
            container(text(if is_member { "✓" } else { "+" }).size(13)).width(Length::Fixed(18.0)),
            text(candidate.name.as_str()).size(13),
            Space::new().width(Length::Fill),
            text(candidate.config_type().to_string())
                .size(11)
                .style(muted_text_style),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .width(Length::Fill);

        rows = rows.push(
            button(row_content)
                .padding([6, 10])
                .width(Length::Fill)
                .on_press(toggle)
                .style(move |theme: &Theme, status| {
                    compound_member_row_style(theme, status, is_member)
                }),
        );
    }

    if eligible == 0 {
        return view_editor_row(
            "Tasks:",
            text("Create other configurations first, then add them here.")
                .size(13)
                .style(muted_text_style)
                .into(),
        )
        .into();
    }

    let workspace_input = text_input(
        "(optional) run in a workspace tab named…",
        workspace.unwrap_or(""),
    )
    .on_input(Message::CompoundWorkspaceChanged)
    .size(13)
    .padding([6, 10])
    .width(Length::Fill);

    column![
        view_editor_row("Workspace:", workspace_input.into()),
        Space::new().height(12),
        text(format!(
            "{member_count} selected — click to add/remove. Members run together when this compound runs."
        ))
        .size(12)
        .style(muted_text_style),
        Space::new().height(10),
        rows,
    ]
    .width(Length::Fill)
    .into()
}

fn compound_member_row_style(
    theme: &Theme,
    status: button::Status,
    is_member: bool,
) -> button::Style {
    let palette = theme.extended_palette();
    let member_bg = Color {
        a: 0.18,
        ..palette.primary.base.color
    };
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => Some(Background::Color(Color {
            a: if is_member { 0.28 } else { 0.08 },
            ..palette.primary.base.color
        })),
        _ if is_member => Some(Background::Color(member_bg)),
        _ => None,
    };

    button::Style {
        background,
        text_color: Color {
            a: if is_member { 0.95 } else { 0.78 },
            ..palette.background.base.text
        },
        border: Border {
            radius: 6.0.into(),
            width: 1.0,
            color: Color {
                a: if is_member { 0.0 } else { 0.08 },
                ..palette.background.base.text
            },
        },
        ..button::Style::default()
    }
}

/// 환경변수 편집 모달 props.
///
/// always-inline 패턴 — 모든 row가 항상 input. view에서 마지막에 빈 row를
/// 자동으로 한 줄 더 그려 auto-grow를 유도한다.
pub struct EnvModalView<'a> {
    pub entries: &'a [(String, String)],
}

/// 환경변수 편집 모달 (반투명 배경 + 중앙 다이얼로그).
///
/// iced 공식 modal 패턴(`opaque(center(opaque(dialog)).style(backdrop))`):
/// - 바깥 `opaque`: backdrop 영역에서 발생한 모든 마우스 이벤트를 흡수해
///   `Stack` 뒤쪽 레이어(메인 UI)로 통과시키지 않음.
/// - 안쪽 `opaque`: dialog 영역 클릭이 backdrop으로 새지 않도록 격리.
///
/// 외부 클릭은 무시되며 명시적인 OK / Cancel / X 버튼으로만 닫힌다.
pub fn view_env_modal<'a>(props: EnvModalView<'a>) -> Element<'a, Message> {
    let header = row![
        text("Environment Variables").size(15),
        Space::new().width(Length::Fill),
        icon_button(ICON_CLOSE, Some(Message::CancelEnvModal)),
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let duplicates = duplicate_key_indices(props.entries);
    let mut table = Column::new().spacing(5);
    for (idx, (key, value)) in props.entries.iter().enumerate() {
        let is_duplicate = duplicates.contains(&idx);
        table = table.push(view_env_modal_row(
            idx,
            key,
            value,
            is_duplicate,
            /* is_placeholder = */ false,
        ));
    }
    // 항상 마지막에 빈 row 한 줄 더 — 사용자가 타이핑하면 handler가 entries에 push (auto-grow)
    table = table.push(view_env_modal_row(
        props.entries.len(),
        "",
        "",
        false,
        /* is_placeholder = */ true,
    ));

    let footer = row![
        Space::new().width(Length::Fill),
        button(text("Cancel").size(13))
            .on_press(Message::CancelEnvModal)
            .padding([6, 16])
            .style(modal_secondary_button_style),
        button(text("OK").size(13))
            .on_press(Message::ConfirmEnvModal)
            .padding([6, 18])
            .style(modal_primary_button_style),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let dialog_content = column![
        header,
        Space::new().height(10),
        scrollable(table.padding(Padding::new(0.0).right(8.0)))
            .height(Length::Fill)
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::default()
                    .width(4)
                    .scroller_width(4.0)
                    .spacing(2.0),
            )),
        Space::new().height(14),
        footer,
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    let dialog = container(dialog_content)
        .padding(18)
        .max_width(640)
        .max_height(520)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(modal_dialog_style);

    opaque(
        center(opaque(dialog))
            .padding(40)
            .style(modal_backdrop_style),
    )
}

/// 앱 설정 모달 뷰 모델 (app의 staging 상태를 참조로 전달).
pub struct SettingsModalView<'a> {
    pub show_environment: bool,
    pub max_output_lines_text: &'a str,
    pub default_auto_scroll: bool,
    pub auto_check_updates: bool,
}

/// 앱 설정 모달 (반투명 배경 + 중앙 다이얼로그). env 모달과 동일한 오버레이/스타일을
/// 재사용한다. 외부 클릭은 무시되며 OK / Cancel / X 로만 닫힌다.
pub fn view_settings_modal(props: SettingsModalView<'_>) -> Element<'_, Message> {
    let header = row![
        text("Settings").size(15),
        Space::new().width(Length::Fill),
        icon_button(ICON_CLOSE, Some(Message::CancelSettingsModal)),
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let max_lines_row = row![
        text("Max output lines per session").size(13),
        Space::new().width(Length::Fill),
        // placeholder는 기본값(DEFAULT_MAX_OUTPUT_LINES=50_000) 힌트 — 상수 변경 시 함께 갱신.
        // 입력을 비웠을 때만 노출되며, 모달을 열면 현재값으로 채워진다.
        text_input("50000", props.max_output_lines_text)
            .on_input(Message::SettingsMaxLinesChanged)
            .padding([6, 10])
            .size(13)
            .width(Length::Fixed(140.0)),
    ]
    .align_y(Alignment::Center)
    .spacing(10);

    let body = column![
        checkbox(props.show_environment)
            .label("Show environment line on run")
            .on_toggle(Message::SettingsToggleEnvironment)
            .size(18)
            .text_size(13),
        max_lines_row,
        checkbox(props.default_auto_scroll)
            .label("Auto-scroll new sessions")
            .on_toggle(Message::SettingsToggleAutoScroll)
            .size(18)
            .text_size(13),
        checkbox(props.auto_check_updates)
            .label("Check for updates on startup")
            .on_toggle(Message::SettingsToggleAutoCheckUpdates)
            .size(18)
            .text_size(13),
    ]
    .spacing(14);

    let footer = row![
        Space::new().width(Length::Fill),
        button(text("Cancel").size(13))
            .on_press(Message::CancelSettingsModal)
            .padding([6, 16])
            .style(modal_secondary_button_style),
        button(text("OK").size(13))
            .on_press(Message::ConfirmSettingsModal)
            .padding([6, 18])
            .style(modal_primary_button_style),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let dialog_content = column![
        header,
        Space::new().height(18),
        body,
        Space::new().height(22),
        footer,
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    let dialog = container(dialog_content)
        .padding(18)
        .max_width(460)
        .max_height(330)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(modal_dialog_style);

    opaque(
        center(opaque(dialog))
            .padding(40)
            .style(modal_backdrop_style),
    )
}

/// staging entries 중 동일 키가 둘 이상 있는 row index 집합.
/// trim 후 빈 키는 중복 검사에서 제외 (placeholder/입력 중인 row 보호).
fn duplicate_key_indices(entries: &[(String, String)]) -> std::collections::HashSet<usize> {
    use std::collections::HashMap;
    let mut by_key: HashMap<&str, Vec<usize>> = HashMap::new();
    for (idx, (key, _)) in entries.iter().enumerate() {
        let trimmed = key.trim();
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

/// 단일 row: [Key input] [Value input] [Duplicate] [Delete].
///
/// `is_placeholder == true`이면 마지막 자동 추가 빈 row — Duplicate/Delete 비활성.
/// `is_duplicate == true`이면 Key/Value input에 경고 테두리 (같은 키 검출 시).
/// Duplicate 버튼: 동일 (key, value)를 entries 끝에 push해 사본 생성.
fn view_env_modal_row<'a>(
    index: usize,
    key: &'a str,
    value: &'a str,
    is_duplicate: bool,
    is_placeholder: bool,
) -> iced::widget::Row<'a, Message> {
    let style_fn: fn(&Theme, text_input::Status) -> text_input::Style = if is_duplicate {
        text_input_warn_style
    } else {
        text_input::default
    };

    let key_input = text_input("Key", key)
        .id(env_modal_key_id(index))
        .on_input(move |s| Message::EnvModalRowKeyChanged(index, s))
        .padding(8)
        .size(13)
        .width(Length::FillPortion(4))
        .style(style_fn);

    let value_input = text_input("Value", value)
        .id(env_modal_value_id(index))
        .on_input(move |s| Message::EnvModalRowValueChanged(index, s))
        .padding(8)
        .size(13)
        .width(Length::FillPortion(6))
        .style(style_fn);

    let duplicate_msg = (!is_placeholder).then_some(Message::EnvModalDuplicateEntry(index));
    let delete_msg = (!is_placeholder).then_some(Message::EnvModalRemoveEntry(index));

    row![
        key_input,
        value_input,
        icon_button(ICON_COPY, duplicate_msg),
        icon_button(ICON_DELETE, delete_msg),
    ]
    .spacing(5)
    .align_y(Alignment::Center)
}

/// 모달 row의 key input에 부여하는 widget id (Tab focus 추적용).
pub fn env_modal_key_id(index: usize) -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::from(format!("env-modal-key-{index}"))
}

/// 모달 row의 value input에 부여하는 widget id.
pub fn env_modal_value_id(index: usize) -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::from(format!("env-modal-value-{index}"))
}

fn text_input_warn_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let base = text_input::default(theme, status);
    text_input::Style {
        border: Border {
            color: Color::from_rgb(0.95, 0.65, 0.20),
            width: 1.0,
            ..base.border
        },
        ..base
    }
}

fn modal_backdrop_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
        ..Default::default()
    }
}

fn modal_dialog_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(palette.background.weak.color)),
        border: Border {
            radius: 8.0.into(),
            width: 1.0,
            color: Color {
                a: 0.18,
                ..palette.background.base.text
            },
        },
        ..Default::default()
    }
}

fn modal_primary_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let base = palette.primary.base;
    let bg = match status {
        button::Status::Hovered => palette.primary.strong.color,
        button::Status::Pressed => palette.primary.weak.color,
        _ => base.color,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: base.text,
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..button::Style::default()
    }
}

fn modal_secondary_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let bg = match status {
        button::Status::Hovered => Color {
            a: 0.10,
            ..palette.background.base.text
        },
        _ => Color::TRANSPARENT,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: palette.background.base.text,
        border: Border {
            radius: 4.0.into(),
            width: 1.0,
            color: Color {
                a: 0.20,
                ..palette.background.base.text
            },
        },
        ..button::Style::default()
    }
}

fn icon_button(
    icon_bytes: &'static [u8],
    on_press: Option<Message>,
) -> iced::widget::Button<'static, Message> {
    let icon = container(
        svg(svg::Handle::from_memory(icon_bytes))
            .width(20)
            .height(20)
            .style(editor_icon_style),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill);
    let button = button(icon)
        .padding(0)
        .width(34)
        .height(34)
        .style(flat_icon_button_style);

    if let Some(message) = on_press {
        button.on_press(message)
    } else {
        button
    }
}

fn editor_icon_style(theme: &Theme, _status: svg::Status) -> svg::Style {
    let palette = theme.extended_palette();
    svg::Style {
        color: Some(Color {
            a: 0.62,
            ..palette.background.base.text
        }),
    }
}

/// Application 타입 필드 렌더링
///
/// Command와 Arguments 입력 필드
fn view_application_fields<'a>(command: &'a str, arguments: &'a str) -> Element<'a, Message> {
    let command_label = text("Command:").size(12).style(editor_label_style);
    let command_input =
        editor_input("Command to execute", command).on_input(Message::CommandChanged);

    let args_label = text("Arguments:").size(12).style(editor_label_style);
    let args_input =
        editor_input("Program arguments", arguments).on_input(Message::ArgumentsChanged);

    let command_row = row![
        container(command_label).width(Length::Fixed(136.0)),
        command_input
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let args_row = row![
        container(args_label).width(Length::Fixed(136.0)),
        args_input
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    column![command_row, Space::new().height(10), args_row]
        .width(Length::Fill)
        .into()
}

/// Shell Script 타입 필드 렌더링
///
/// Execute Mode 선택 및 모드별 필드
fn view_shell_script_fields<'a>(
    execute_mode: &'a ExecuteMode,
    is_loading_script_file: bool,
    is_loading_interpreter: bool,
    select_state: &'a EditorSelectState,
) -> Element<'a, Message> {
    // 현재 모드 타입 결정
    let current_mode_type = match execute_mode {
        ExecuteMode::ScriptFile { .. } => &ExecuteModeType::ScriptFile,
        ExecuteMode::ScriptText { .. } => &ExecuteModeType::ScriptText,
    };

    let mode_label = text("Execute Mode:").size(12).style(editor_label_style);
    let mode_picker = editor_combo_box(
        &select_state.execute_mode,
        "Select execute mode",
        Some(current_mode_type),
        Message::ExecuteModeChanged,
    );

    let mode_row = row![
        container(mode_label).width(Length::Fixed(136.0)),
        mode_picker,
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    // 모드별 필드
    let mode_fields = match execute_mode {
        ExecuteMode::ScriptFile { .. } => {
            view_script_file_fields(execute_mode, is_loading_script_file, is_loading_interpreter)
        }
        ExecuteMode::ScriptText { .. } => view_script_text_fields(execute_mode),
    };

    column![mode_row, Space::new().height(10), mode_fields]
        .width(Length::Fill)
        .into()
}

/// Script File 모드 필드 렌더링
fn view_script_file_fields(
    execute_mode: &ExecuteMode,
    is_loading_script_file: bool,
    is_loading_interpreter: bool,
) -> Element<'_, Message> {
    let ExecuteMode::ScriptFile {
        script_path,
        script_options,
        interpreter_path,
        interpreter_options,
    } = execute_mode
    else {
        unreachable!();
    };

    // 1. Script Path (file select 버튼 포함)
    let path_label = text("Script Path:").size(12).style(editor_label_style);
    let path_input =
        editor_input("Script file path", script_path).on_input(Message::ScriptPathChanged);

    let browse_svg = svg::Handle::from_memory(ICON_FOLDER_OPEN);
    let browse_icon = container(
        svg(browse_svg)
            .width(20)
            .height(20)
            .style(editor_icon_style),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill);

    let mut browse_btn = button(browse_icon)
        .padding(0)
        .width(34)
        .height(34)
        .style(flat_icon_button_style);

    if !is_loading_script_file {
        browse_btn = browse_btn.on_press(Message::BrowseScriptPath);
    }

    let path_row = row![
        container(path_label).width(Length::Fixed(136.0)),
        path_input,
        browse_btn,
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    // 2. Script Options
    let options_label = text("Script Options:").size(12).style(editor_label_style);
    let options_input = editor_input("Options (e.g., -v --release)", script_options)
        .on_input(Message::ScriptOptionsChanged);

    let options_row = row![
        container(options_label).width(Length::Fixed(136.0)),
        options_input
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    // 3. Interpreter Path (optional)
    let interp_label = text("Interpreter:").size(12).style(editor_label_style);
    let interp_value = interpreter_path.as_ref().map_or("", String::as_str);
    let interp_input = editor_input("Interpreter path (optional)", interp_value)
        .on_input(Message::InterpreterPathChanged);

    let interp_browse_svg = svg::Handle::from_memory(ICON_FOLDER_OPEN);
    let interp_browse_icon = container(
        svg(interp_browse_svg)
            .width(20)
            .height(20)
            .style(editor_icon_style),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill);

    let mut interp_browse_btn = button(interp_browse_icon)
        .padding(0)
        .width(34)
        .height(34)
        .style(flat_icon_button_style);

    if !is_loading_interpreter {
        interp_browse_btn = interp_browse_btn.on_press(Message::BrowseInterpreterPath);
    }

    let interp_row = row![
        container(interp_label).width(Length::Fixed(136.0)),
        interp_input,
        interp_browse_btn,
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    // 4. Interpreter Options (optional)
    let interp_opts_label = text("Interpreter Options:")
        .size(12)
        .style(editor_label_style);
    let interp_opts_value = interpreter_options.as_ref().map_or("", String::as_str);
    let interp_opts_input = editor_input("Interpreter options (optional)", interp_opts_value)
        .on_input(Message::InterpreterOptionsChanged);

    let interp_opts_row = row![
        container(interp_opts_label).width(Length::Fixed(136.0)),
        interp_opts_input
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    column![
        path_row,
        Space::new().height(10),
        options_row,
        Space::new().height(10),
        interp_row,
        Space::new().height(10),
        interp_opts_row,
    ]
    .width(Length::Fill)
    .into()
}

/// Script Text 모드 필드 렌더링
fn view_script_text_fields(execute_mode: &ExecuteMode) -> Element<'_, Message> {
    let ExecuteMode::ScriptText { script_text } = execute_mode else {
        unreachable!();
    };

    let text_label = text("Script Text:").size(12).style(editor_label_style);
    let text_area =
        editor_input("Enter script here...", script_text).on_input(Message::ScriptTextChanged);

    column![
        text_label,
        Space::new().height(5),
        container(text_area).width(Length::Fill),
    ]
    .width(Length::Fill)
    .into()
}

/// Node package manager 타입 필드 렌더링
///
/// 프로젝트 디렉토리, package.json 선택, Node Runtime, package manager 명령어, 스크립트 선택, 인자, Node 옵션 설정
#[allow(clippy::too_many_arguments)]
fn view_node_fields<'a>(
    project_directory: &'a str,
    working_directory: &'a str,
    node_runtime_path: Option<&'a String>,
    available_node_runtimes: &'a [(String, String)],
    select_state: &'a EditorSelectState,
    package_manager: &'a PackageManager,
    command: &'a NodeCommand,
    script_name: Option<&'a String>,
    arguments: &'a str,
    node_options: &'a str,
    options: NodeFieldOptions,
) -> Element<'a, Message> {
    let NodeFieldOptions {
        selected_package_json,
        is_loading_scripts,
        is_loading_project_directory,
    } = options;
    let selected_package_json = selected_package_json.as_ref().and_then(|path| {
        select_state
            .package_json
            .options()
            .iter()
            .find(|option| *option == path)
    });

    let mut col = column![
        view_node_project_directory_row(project_directory, is_loading_project_directory),
        Space::new().height(5),
        view_node_package_json_row(select_state, selected_package_json),
        Space::new().height(5),
        view_node_working_directory_row(working_directory),
        Space::new().height(10),
    ]
    .width(Length::Fill);

    col = col
        .push(view_node_package_manager_row(package_manager, select_state))
        .push(Space::new().height(10))
        .push(view_node_command_row(command, select_state));

    if command.requires_script() {
        col = col.push(Space::new().height(10)).push(view_node_script_row(
            select_state,
            script_name,
            is_loading_scripts,
        ));

        if select_state.node_script.options().is_empty() {
            col = col
                .push(Space::new().height(5))
                .push(view_node_script_hint_row());
        }
    }

    col = col
        .push(Space::new().height(10))
        .push(Space::new().height(10))
        .push(view_node_arguments_row(arguments))
        .push(Space::new().height(10))
        .push(view_node_runtime_row(
            node_runtime_path,
            available_node_runtimes,
            select_state,
        ))
        .push(Space::new().height(10))
        .push(view_node_options_row(node_options));

    col.into()
}

fn view_node_project_directory_row(
    project_directory: &str,
    is_loading_project_directory: bool,
) -> iced::widget::Row<'_, Message> {
    let proj_display: Element<'_, Message> =
        if project_directory.is_empty() || project_directory == "." {
            readonly_value("Not selected").into()
        } else {
            readonly_value(project_directory).into()
        };

    let mut browse_btn = icon_button(ICON_FOLDER_OPEN, None)
        .padding(0)
        .width(34)
        .height(34);

    if !is_loading_project_directory {
        browse_btn = browse_btn.on_press(Message::BrowseProjectDirectory);
    }

    row![
        container(
            text("Project Directory:")
                .size(12)
                .style(editor_label_style)
        )
        .width(Length::Fixed(136.0)),
        proj_display,
        browse_btn,
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fill)
}

fn view_node_package_json_row<'a>(
    select_state: &'a EditorSelectState,
    selected_package_json: Option<&'a String>,
) -> iced::widget::Row<'a, Message> {
    view_editor_row(
        "package.json:",
        editor_combo_box(
            &select_state.package_json,
            "Search package.json",
            selected_package_json,
            Message::PackageJsonDropdownChanged,
        )
        .into(),
    )
}

fn view_node_working_directory_row(working_directory: &str) -> iced::widget::Row<'_, Message> {
    view_editor_row(
        "Working Directory:",
        readonly_value(working_directory).into(),
    )
}

fn view_node_command_row<'a>(
    command: &'a NodeCommand,
    select_state: &'a EditorSelectState,
) -> iced::widget::Row<'a, Message> {
    view_editor_row(
        "Command:",
        editor_combo_box(
            &select_state.node_command,
            "Search command",
            Some(command),
            Message::NodeCommandChanged,
        )
        .into(),
    )
}

fn view_node_package_manager_row<'a>(
    package_manager: &'a PackageManager,
    select_state: &'a EditorSelectState,
) -> iced::widget::Row<'a, Message> {
    view_editor_row(
        "Package Manager:",
        editor_combo_box(
            &select_state.package_manager,
            "Search package manager",
            Some(package_manager),
            Message::PackageManagerChanged,
        )
        .into(),
    )
}

fn view_node_script_row<'a>(
    select_state: &'a EditorSelectState,
    script_name: Option<&'a String>,
    is_loading_scripts: bool,
) -> iced::widget::Row<'a, Message> {
    let script_picker: Element<'_, Message> = if select_state.node_script.options().is_empty() {
        editor_combo_box(
            &select_state.node_script,
            "No scripts found",
            None,
            Message::NodeScriptNameChanged,
        )
        .into()
    } else {
        editor_combo_box(
            &select_state.node_script,
            "Search script",
            script_name,
            Message::NodeScriptNameChanged,
        )
        .into()
    };

    let mut refresh_btn = icon_button(ICON_REFRESH, None)
        .padding(0)
        .width(34)
        .height(34);

    if !is_loading_scripts {
        refresh_btn = refresh_btn.on_press(Message::NodeRefreshScripts);
    }

    row![
        container(text("Script:").size(12).style(editor_label_style)).width(Length::Fixed(136.0)),
        container(script_picker).width(Length::Fill),
        refresh_btn,
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fill)
}

fn view_node_script_hint_row() -> iced::widget::Row<'static, Message> {
    row![
        Space::new().width(Length::Fixed(148.0)),
        text("Select a package.json from the dropdown above")
            .size(12)
            .style(muted_text_style),
    ]
}

fn view_node_arguments_row(arguments: &str) -> iced::widget::Row<'_, Message> {
    view_editor_row(
        "Arguments:",
        editor_input("--flag value", arguments)
            .on_input(Message::NodeArgumentsChanged)
            .into(),
    )
}

fn view_node_runtime_row<'a>(
    node_runtime_path: Option<&'a String>,
    available_node_runtimes: &'a [(String, String)],
    select_state: &'a EditorSelectState,
) -> iced::widget::Row<'a, Message> {
    let current_runtime = node_runtime_path.map_or("node", String::as_str);
    let current_label = available_node_runtimes
        .iter()
        .find(|(_, path)| path == current_runtime)
        .map(|(label, _)| label);

    view_editor_row(
        "Node Runtime:",
        editor_combo_box(
            &select_state.node_runtime,
            "Search Node runtime",
            current_label,
            Message::NodeRuntimeChanged,
        )
        .into(),
    )
}

fn view_node_options_row(node_options: &str) -> iced::widget::Row<'_, Message> {
    view_editor_row(
        "Node Options:",
        editor_input("--max-old-space-size=4096", node_options)
            .on_input(Message::NodeOptionsChanged)
            .into(),
    )
}

/// Kotlin 타입 필드 렌더링
///
/// 실행 모드(Main class / JAR), 모드별 필드, VM options, program arguments, JDK 선택
#[allow(clippy::too_many_arguments)]
fn view_kotlin_fields<'a>(
    launch_mode: &'a KotlinLaunchMode,
    vm_options: &'a str,
    program_arguments: &'a str,
    jdk_path: Option<&'a String>,
    available_jdks: &'a [(String, String)],
    select_state: &'a EditorSelectState,
    is_loading_kotlin_jar: bool,
    is_loading_kotlin_jdk: bool,
) -> Element<'a, Message> {
    let mut col =
        column![view_kotlin_launch_mode_row(launch_mode, select_state)].width(Length::Fill);

    match launch_mode {
        KotlinLaunchMode::MainClass {
            main_class,
            classpath,
        } => {
            col = col
                .push(Space::new().height(10))
                .push(view_kotlin_main_class_row(main_class))
                .push(Space::new().height(10))
                .push(view_kotlin_classpath_row(classpath));
        }
        KotlinLaunchMode::Jar { jar_path } => {
            col = col
                .push(Space::new().height(10))
                .push(view_kotlin_jar_path_row(jar_path, is_loading_kotlin_jar));
        }
    }

    col = col
        .push(Space::new().height(10))
        .push(view_kotlin_vm_options_row(vm_options))
        .push(Space::new().height(10))
        .push(view_kotlin_program_arguments_row(program_arguments))
        .push(Space::new().height(10))
        .push(view_kotlin_jdk_row(
            jdk_path,
            available_jdks,
            select_state,
            is_loading_kotlin_jdk,
        ));

    col.into()
}

fn view_kotlin_launch_mode_row<'a>(
    launch_mode: &'a KotlinLaunchMode,
    select_state: &'a EditorSelectState,
) -> iced::widget::Row<'a, Message> {
    // 단위 variant라 rvalue static promotion으로 &'static 참조를 얻는다 (ExecuteModeType 동일 패턴).
    let current_mode_type = match launch_mode {
        KotlinLaunchMode::MainClass { .. } => &KotlinLaunchModeType::MainClass,
        KotlinLaunchMode::Jar { .. } => &KotlinLaunchModeType::Jar,
    };

    view_editor_row(
        "Launch Mode:",
        editor_combo_box(
            &select_state.kotlin_launch_mode,
            "Select launch mode",
            Some(current_mode_type),
            Message::KotlinLaunchModeChanged,
        )
        .into(),
    )
}

fn view_kotlin_main_class_row(main_class: &str) -> iced::widget::Row<'_, Message> {
    view_editor_row(
        "Main Class:",
        editor_input("com.example.MainKt", main_class)
            .on_input(Message::KotlinMainClassChanged)
            .into(),
    )
}

fn view_kotlin_classpath_row(classpath: &str) -> iced::widget::Row<'_, Message> {
    view_editor_row(
        "Classpath:",
        editor_input("build/libs/*:libs/*", classpath)
            .on_input(Message::KotlinClasspathChanged)
            .into(),
    )
}

fn view_kotlin_jar_path_row(
    jar_path: &str,
    is_loading_kotlin_jar: bool,
) -> iced::widget::Row<'_, Message> {
    let path_input = editor_input("Path to .jar", jar_path).on_input(Message::KotlinJarPathChanged);

    let mut browse_btn = icon_button(ICON_FOLDER_OPEN, None)
        .padding(0)
        .width(34)
        .height(34);

    if !is_loading_kotlin_jar {
        browse_btn = browse_btn.on_press(Message::BrowseKotlinJarPath);
    }

    row![
        container(text("JAR Path:").size(12).style(editor_label_style)).width(Length::Fixed(136.0)),
        path_input,
        browse_btn,
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fill)
}

fn view_kotlin_vm_options_row(vm_options: &str) -> iced::widget::Row<'_, Message> {
    view_editor_row(
        "VM Options:",
        editor_input("-Xmx2g -Dkey=value", vm_options)
            .on_input(Message::KotlinVmOptionsChanged)
            .into(),
    )
}

fn view_kotlin_program_arguments_row(program_arguments: &str) -> iced::widget::Row<'_, Message> {
    view_editor_row(
        "Program Args:",
        editor_input("arg1 arg2", program_arguments)
            .on_input(Message::KotlinProgramArgumentsChanged)
            .into(),
    )
}

fn view_kotlin_jdk_row<'a>(
    jdk_path: Option<&'a String>,
    available_jdks: &'a [(String, String)],
    select_state: &'a EditorSelectState,
    is_loading_kotlin_jdk: bool,
) -> iced::widget::Row<'a, Message> {
    let current_jdk = jdk_path.map_or("java", String::as_str);
    // 감지 목록에서 레이블을 찾고, 없으면(수동 선택 경로) 원시 경로를 그대로 표시.
    let current_label = available_jdks
        .iter()
        .find(|(_, path)| path == current_jdk)
        .map(|(label, _)| label)
        .or(jdk_path);

    let mut browse_btn = icon_button(ICON_FOLDER_OPEN, None)
        .padding(0)
        .width(34)
        .height(34);

    if !is_loading_kotlin_jdk {
        browse_btn = browse_btn.on_press(Message::BrowseKotlinJdk);
    }

    row![
        container(text("JDK:").size(12).style(editor_label_style)).width(Length::Fixed(136.0)),
        container(editor_combo_box(
            &select_state.jdk,
            "Search JDK",
            current_label,
            Message::KotlinJdkChanged,
        ))
        .width(Length::Fill),
        browse_btn,
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fill)
}
