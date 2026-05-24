use bevy::app::AppExit;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input::ButtonInput;
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowResolution};
use bevy::winit::WinitWindows;
use chrono::Local;
use std::collections::VecDeque;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x00000010;

const LOG_VIEW_LINES: usize = 8;
const LOG_SOURCE_LINES: usize = 500;
const LOG_TAIL_BYTES: usize = 64 * 1024;
const LOG_REFRESH_SECONDS: f32 = 1.0;
const LABEL_FONT: f32 = 15.0;
const VALUE_FONT: f32 = 15.0;
const TITLE_FONT: f32 = 24.0;
const SUBTITLE_FONT: f32 = 12.0;
const BUTTON_WIDTH: f32 = 132.0;
const SMALL_BUTTON_WIDTH: f32 = 36.0;
const VALUE_MIN_WIDTH: f32 = 110.0;
const LABEL_WIDTH: f32 = 140.0;
const ROW_HEIGHT: f32 = 30.0;

fn main() {
    let mut config = LauncherConfig::default();
    config.refresh_log_view();

    App::new()
        .insert_resource(config)
        .insert_resource(LogRefreshTimer(Timer::from_seconds(
            LOG_REFRESH_SECONDS,
            TimerMode::Repeating,
        )))
        .insert_resource(UiTheme::default())
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "RustySerial launcher".into(),
                resolution: WindowResolution::new(1080.0, 740.0),
                resizable: true,
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, (setup_ui, set_window_icon))
        .add_systems(
            Update,
            (
                adaptive_ui_scale,
                auto_refresh_log_view,
                button_interactions,
                log_filter_input,
                sync_value_labels,
                sync_stats_strip,
                sync_log_view,
                sync_filter_button,
                reap_active_child,
            ),
        )
        .run();
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ParityChoice {
    None,
    Odd,
    Even,
}

impl ParityChoice {
    fn as_flag(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Odd => "odd",
            Self::Even => "even",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Odd => "Odd",
            Self::Even => "Even",
        }
    }

    fn step(self, dir: i8) -> Self {
        const ITEMS: [ParityChoice; 3] =
            [ParityChoice::None, ParityChoice::Odd, ParityChoice::Even];
        step_enum(&ITEMS, self, dir)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StopBitsChoice {
    One,
    Two,
}

impl StopBitsChoice {
    fn as_flag(self) -> &'static str {
        match self {
            Self::One => "one",
            Self::Two => "two",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::One => "1",
            Self::Two => "2",
        }
    }

    fn step(self, dir: i8) -> Self {
        const ITEMS: [StopBitsChoice; 2] = [StopBitsChoice::One, StopBitsChoice::Two];
        step_enum(&ITEMS, self, dir)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlowControlChoice {
    None,
    RtsCts,
    XonXoff,
}

impl FlowControlChoice {
    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::RtsCts => "RTS/CTS",
            Self::XonXoff => "XON/XOFF",
        }
    }

    fn step(self, dir: i8) -> Self {
        const ITEMS: [FlowControlChoice; 3] = [
            FlowControlChoice::None,
            FlowControlChoice::RtsCts,
            FlowControlChoice::XonXoff,
        ];
        step_enum(&ITEMS, self, dir)
    }
}

#[derive(Resource)]
struct LauncherConfig {
    ports: Vec<String>,
    selected_port: usize,
    baud_rates: Vec<u32>,
    selected_baud: usize,
    data_bits: u8,
    parity: ParityChoice,
    stop_bits: StopBitsChoice,
    flow: FlowControlChoice,
    crlf: bool,
    local_echo: bool,
    log_enabled: bool,
    auto_reconnect: bool,
    reconnect_delays_ms: Vec<u64>,
    selected_reconnect_delay: usize,
    connect_launch_count: u64,
    log_view_source_lines: VecDeque<String>,
    log_view_lines: VecDeque<String>,
    log_filter: String,
    log_filter_active: bool,
    log_auto_refresh: bool,
    log_last_refresh_count: usize,
    log_total_match_count: usize,
    log_path: String,
    status: String,
    active_child: Option<Child>,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        let ports = discover_ports();
        Self {
            ports,
            selected_port: 0,
            baud_rates: vec![9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600],
            selected_baud: 4,
            data_bits: 8,
            parity: ParityChoice::None,
            stop_bits: StopBitsChoice::One,
            flow: FlowControlChoice::None,
            crlf: false,
            local_echo: false,
            log_enabled: false,
            auto_reconnect: true,
            reconnect_delays_ms: vec![250, 500, 1000, 2000, 5000],
            selected_reconnect_delay: 2,
            connect_launch_count: 0,
            log_view_source_lines: VecDeque::new(),
            log_view_lines: VecDeque::new(),
            log_filter: String::new(),
            log_filter_active: false,
            log_auto_refresh: true,
            log_last_refresh_count: 0,
            log_total_match_count: 0,
            log_path: "rustyserial.log".to_string(),
            status: "Select settings and press Connect".to_string(),
            active_child: None,
        }
    }
}

impl LauncherConfig {
    fn selected_port_name(&self) -> Option<&str> {
        self.ports.get(self.selected_port).map(String::as_str)
    }

    fn selected_baud_value(&self) -> u32 {
        self.baud_rates[self.selected_baud]
    }

    fn selected_reconnect_delay_value(&self) -> u64 {
        self.reconnect_delays_ms
            .get(self.selected_reconnect_delay)
            .copied()
            .unwrap_or(1000)
    }

    /// Resolve `log_path` relative to the executable directory so the GUI and
    /// the launched TUI always agree on where the log lives.
    fn resolved_log_path(&self) -> PathBuf {
        let p = Path::new(&self.log_path);
        if p.is_absolute() {
            return p.to_path_buf();
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                return dir.join(p);
            }
        }
        p.to_path_buf()
    }

    fn refresh_ports(&mut self) {
        self.ports = discover_ports();
        if self.ports.is_empty() {
            self.selected_port = 0;
            self.status =
                "No serial ports found. Connect a device and click Refresh Ports.".to_string();
        } else {
            self.selected_port = self.selected_port.min(self.ports.len().saturating_sub(1));
            self.status = format!("Discovered {} serial port(s)", self.ports.len());
        }
    }

    /// Reload the log view buffer. Returns a human-readable status string; the
    /// auto-refresh path discards it so it does not stomp the user-facing status
    /// every second.
    fn refresh_log_view(&mut self) -> String {
        let resolved = self.resolved_log_path();
        match load_log_view(&resolved) {
            Ok(lines) => {
                self.log_view_source_lines = lines;
                self.apply_log_filter();
                let filter = self.log_filter.trim();
                if filter.is_empty() {
                    format!(
                        "Loaded {} recent line(s) from {}",
                        self.log_view_source_lines.len(),
                        resolved.display()
                    )
                } else {
                    format!(
                        "{} match(es) in last {} line(s) of {}",
                        self.log_total_match_count,
                        self.log_view_source_lines.len(),
                        resolved.display()
                    )
                }
            }
            Err(status) => {
                self.log_view_source_lines.clear();
                self.log_view_lines.clear();
                self.log_last_refresh_count = 0;
                self.log_total_match_count = 0;
                status
            }
        }
    }

    fn apply_log_filter(&mut self) {
        let filter = self.log_filter.trim().to_lowercase();
        self.log_view_lines.clear();

        let mut total_matches = 0usize;
        for line in &self.log_view_source_lines {
            let matches = filter.is_empty() || line.to_lowercase().contains(&filter);
            if matches {
                total_matches += 1;
                if self.log_view_lines.len() >= LOG_VIEW_LINES {
                    self.log_view_lines.pop_front();
                }
                self.log_view_lines.push_back(line.clone());
            }
        }

        self.log_total_match_count = total_matches;
        self.log_last_refresh_count = self.log_view_lines.len();
    }

    fn export_log_view(&mut self) {
        let resolved = self.resolved_log_path();
        match export_log(&resolved) {
            Ok(path) => {
                self.status = format!("Exported log to {}", path.display());
                let _ = self.refresh_log_view();
            }
            Err(status) => {
                self.status = status;
            }
        }
    }

    fn set_filter_char(&mut self, ch: char) {
        if !self.log_filter_active {
            return;
        }
        self.log_filter.push(ch);
        self.apply_log_filter();
    }

    fn backspace_filter(&mut self) {
        if !self.log_filter_active {
            return;
        }
        self.log_filter.pop();
        self.apply_log_filter();
    }

    fn clear_filter(&mut self) {
        self.log_filter.clear();
        self.apply_log_filter();
    }
}

#[derive(Resource)]
struct UiTheme {
    bg: Color,
    bg_glow: Color,
    panel: Color,
    panel_border: Color,
    row: Color,
    row_border: Color,
    button: Color,
    button_hover: Color,
    button_pressed: Color,
    title: Color,
    subtitle: Color,
    text: Color,
    accent: Color,
    status_ok: Color,
}

impl Default for UiTheme {
    fn default() -> Self {
        Self {
            bg: Color::srgb(0.03, 0.07, 0.11),
            bg_glow: Color::srgb(0.08, 0.17, 0.22),
            panel: Color::srgb(0.06, 0.12, 0.17),
            panel_border: Color::srgb(0.18, 0.38, 0.50),
            row: Color::srgb(0.09, 0.18, 0.25),
            row_border: Color::srgb(0.18, 0.30, 0.39),
            button: Color::srgb(0.15, 0.28, 0.36),
            button_hover: Color::srgb(0.22, 0.40, 0.49),
            button_pressed: Color::srgb(0.33, 0.57, 0.63),
            title: Color::srgb(0.90, 0.97, 0.99),
            subtitle: Color::srgb(0.56, 0.78, 0.86),
            text: Color::srgb(0.83, 0.90, 0.95),
            accent: Color::srgb(0.29, 0.82, 0.92),
            status_ok: Color::srgb(0.64, 0.92, 0.76),
        }
    }
}

#[derive(Component, Clone, Copy)]
enum Action {
    PortPrev,
    PortNext,
    BaudPrev,
    BaudNext,
    DataBitsPrev,
    DataBitsNext,
    ParityPrev,
    ParityNext,
    StopBitsPrev,
    StopBitsNext,
    FlowPrev,
    FlowNext,
    ReconnectDelayPrev,
    ReconnectDelayNext,
    ToggleCrlf,
    ToggleLocalEcho,
    ToggleLog,
    ToggleReconnect,
    ToggleFilterFocus,
    ClearFilter,
    ToggleLogAutoRefresh,
    RefreshPorts,
    RefreshLog,
    ExportLog,
    Connect,
    Quit,
}

#[derive(Component, Clone, Copy)]
enum ValueField {
    Port,
    Baud,
    DataBits,
    Parity,
    StopBits,
    Flow,
    Crlf,
    LocalEcho,
    Log,
    Reconnect,
    ReconnectDelay,
    Status,
}

#[derive(Component)]
struct ValueLabel(ValueField);

#[derive(Component)]
struct StatsLine;

#[derive(Component)]
struct LogFilterText;

#[derive(Component)]
struct EventLogLine(usize);

#[derive(Component)]
struct ActionButton {
    action: Action,
}

#[derive(Component)]
struct FilterEditButton;

#[derive(Resource)]
struct LogRefreshTimer(Timer);

/// Set the OS window icon (title bar + taskbar) from the bundled PNG. Runs
/// once at Startup. `NonSend` because winit window handles are not Send.
fn set_window_icon(
    windows: NonSend<WinitWindows>,
    primary: Query<Entity, With<PrimaryWindow>>,
) {
    let Ok(entity) = primary.get_single() else {
        return;
    };
    let Some(window) = windows.get_window(entity) else {
        return;
    };

    // Bundled at compile time so the .exe is self-contained.
    let bytes = include_bytes!("../../assets/rsicon-64.png");
    let decoded = match image::load_from_memory(bytes) {
        Ok(img) => img.into_rgba8(),
        Err(err) => {
            warn!("could not decode bundled window icon: {err}");
            return;
        }
    };
    let (w, h) = decoded.dimensions();
    let icon = match winit::window::Icon::from_rgba(decoded.into_raw(), w, h) {
        Ok(icon) => icon,
        Err(err) => {
            warn!("could not build window icon: {err}");
            return;
        }
    };
    window.set_window_icon(Some(icon));
}

fn setup_ui(mut commands: Commands, theme: Res<UiTheme>) {
    commands.spawn(Camera2dBundle::default());

    commands
        .spawn(NodeBundle {
            style: Style {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexStart,
                padding: UiRect::all(Val::Px(4.0)),
                ..default()
            },
            background_color: theme.bg.into(),
            ..default()
        })
        .with_children(|root| {
            root.spawn(NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    top: Val::Px(0.0),
                    left: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Px(96.0),
                    ..default()
                },
                background_color: theme.bg_glow.into(),
                ..default()
            });

            root.spawn(NodeBundle {
                style: Style {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    padding: UiRect::all(Val::Px(8.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                background_color: theme.panel.into(),
                border_color: theme.panel_border.into(),
                ..default()
            })
            .with_children(|panel| {
                panel.spawn(NodeBundle {
                    style: Style {
                        width: Val::Percent(100.0),
                        height: Val::Px(3.0),
                        margin: UiRect::bottom(Val::Px(4.0)),
                        ..default()
                    },
                    background_color: theme.accent.into(),
                    ..default()
                });

                panel.spawn(
                    TextBundle::from_section(
                        "RustySerial setup",
                        TextStyle {
                            font_size: TITLE_FONT,
                            color: theme.title,
                            ..default()
                        },
                    )
                    .with_style(Style {
                        margin: UiRect::bottom(Val::Px(2.0)),
                        ..default()
                    }),
                );

                panel.spawn(
                    TextBundle::from_section(
                        "Configure serial line settings, then launch the live terminal.",
                        TextStyle {
                            font_size: SUBTITLE_FONT,
                            color: theme.subtitle,
                            ..default()
                        },
                    )
                    .with_style(Style {
                        margin: UiRect::bottom(Val::Px(6.0)),
                        ..default()
                    }),
                );

                panel
                    .spawn(NodeBundle {
                        style: Style {
                            width: Val::Percent(100.0),
                            padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                            margin: UiRect::bottom(Val::Px(6.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        background_color: theme.row.into(),
                        border_color: theme.row_border.into(),
                        ..default()
                    })
                    .with_children(|strip| {
                        strip.spawn((
                            TextBundle::from_section(
                                "",
                                TextStyle {
                                    font_size: 12.0,
                                    color: theme.status_ok,
                                    ..default()
                                },
                            ),
                            StatsLine,
                        ));
                    });

                spawn_selector_row(
                    panel,
                    &theme,
                    "Port",
                    Action::PortPrev,
                    Action::PortNext,
                    ValueField::Port,
                );
                spawn_selector_row(
                    panel,
                    &theme,
                    "Baud",
                    Action::BaudPrev,
                    Action::BaudNext,
                    ValueField::Baud,
                );
                spawn_selector_row(
                    panel,
                    &theme,
                    "Data Bits",
                    Action::DataBitsPrev,
                    Action::DataBitsNext,
                    ValueField::DataBits,
                );
                spawn_selector_row(
                    panel,
                    &theme,
                    "Parity",
                    Action::ParityPrev,
                    Action::ParityNext,
                    ValueField::Parity,
                );
                spawn_selector_row(
                    panel,
                    &theme,
                    "Stop Bits",
                    Action::StopBitsPrev,
                    Action::StopBitsNext,
                    ValueField::StopBits,
                );
                spawn_selector_row(
                    panel,
                    &theme,
                    "Flow Control",
                    Action::FlowPrev,
                    Action::FlowNext,
                    ValueField::Flow,
                );
                spawn_toggle_row(panel, &theme, "CRLF", Action::ToggleCrlf, ValueField::Crlf);
                spawn_toggle_row(
                    panel,
                    &theme,
                    "Local Echo",
                    Action::ToggleLocalEcho,
                    ValueField::LocalEcho,
                );
                spawn_toggle_row(panel, &theme, "Logging", Action::ToggleLog, ValueField::Log);
                spawn_toggle_row(
                    panel,
                    &theme,
                    "Auto Reconnect",
                    Action::ToggleReconnect,
                    ValueField::Reconnect,
                );
                spawn_selector_row(
                    panel,
                    &theme,
                    "Reconnect Delay",
                    Action::ReconnectDelayPrev,
                    Action::ReconnectDelayNext,
                    ValueField::ReconnectDelay,
                );

                panel
                    .spawn(NodeBundle {
                        style: Style {
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            margin: UiRect::top(Val::Px(8.0)),
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: Val::Px(10.0),
                            row_gap: Val::Px(6.0),
                            ..default()
                        },
                        ..default()
                    })
                    .with_children(|row| {
                        spawn_action_button(
                            row,
                            &theme,
                            "Refresh Ports",
                            Action::RefreshPorts,
                            BUTTON_WIDTH,
                        );
                        spawn_action_button(
                            row,
                            &theme,
                            "Connect",
                            Action::Connect,
                            BUTTON_WIDTH,
                        );
                        spawn_action_button(row, &theme, "Quit", Action::Quit, BUTTON_WIDTH);
                    });

                panel
                    .spawn(NodeBundle {
                        style: Style {
                            width: Val::Percent(100.0),
                            margin: UiRect::top(Val::Px(8.0)),
                            padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        background_color: theme.row.into(),
                        border_color: theme.row_border.into(),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            TextBundle::from_section(
                                "",
                                TextStyle {
                                    font_size: LABEL_FONT,
                                    color: theme.status_ok,
                                    ..default()
                                },
                            ),
                            ValueLabel(ValueField::Status),
                        ));
                    });

                panel
                    .spawn(NodeBundle {
                        style: Style {
                            width: Val::Percent(100.0),
                            margin: UiRect::top(Val::Px(6.0)),
                            padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            flex_direction: FlexDirection::Column,
                            flex_grow: 1.0,
                            row_gap: Val::Px(4.0),
                            ..default()
                        },
                        background_color: theme.row.into(),
                        border_color: theme.row_border.into(),
                        ..default()
                    })
                    .with_children(|log_panel| {
                        log_panel
                            .spawn(NodeBundle {
                                style: Style {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(ROW_HEIGHT + 4.0),
                                    align_items: AlignItems::Center,
                                    column_gap: Val::Px(8.0),
                                    ..default()
                                },
                                ..default()
                            })
                            .with_children(|row| {
                                row.spawn(
                                    TextBundle::from_section(
                                        "Session Log",
                                        TextStyle {
                                            font_size: LABEL_FONT,
                                            color: theme.accent,
                                            ..default()
                                        },
                                    )
                                    .with_style(Style {
                                        flex_grow: 1.0,
                                        ..default()
                                    }),
                                );
                                spawn_action_button(
                                    row,
                                    &theme,
                                    "Refresh Log",
                                    Action::RefreshLog,
                                    BUTTON_WIDTH,
                                );
                                spawn_action_button(
                                    row,
                                    &theme,
                                    "Export Log",
                                    Action::ExportLog,
                                    BUTTON_WIDTH,
                                );
                                spawn_action_button(
                                    row,
                                    &theme,
                                    "Auto Refresh",
                                    Action::ToggleLogAutoRefresh,
                                    BUTTON_WIDTH,
                                );
                            });

                        log_panel
                            .spawn(NodeBundle {
                                style: Style {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(ROW_HEIGHT + 4.0),
                                    padding: UiRect::axes(Val::Px(10.0), Val::Px(2.0)),
                                    border: UiRect::all(Val::Px(1.0)),
                                    align_items: AlignItems::Center,
                                    column_gap: Val::Px(8.0),
                                    ..default()
                                },
                                background_color: theme.row.into(),
                                border_color: theme.row_border.into(),
                                ..default()
                            })
                            .with_children(|row| {
                                row.spawn(
                                    TextBundle::from_section(
                                        "Filter",
                                        TextStyle {
                                            font_size: LABEL_FONT,
                                            color: theme.accent,
                                            ..default()
                                        },
                                    )
                                    .with_style(Style {
                                        width: Val::Px(LABEL_WIDTH - 60.0),
                                        ..default()
                                    }),
                                );

                                row.spawn((
                                    TextBundle::from_section(
                                        "",
                                        TextStyle {
                                            font_size: VALUE_FONT,
                                            color: theme.text,
                                            ..default()
                                        },
                                    )
                                    .with_style(Style {
                                        flex_grow: 1.0,
                                        min_width: Val::Px(240.0),
                                        ..default()
                                    }),
                                    LogFilterText,
                                ));

                                spawn_filter_edit_button(row, &theme);
                                spawn_action_button(
                                    row,
                                    &theme,
                                    "Clear",
                                    Action::ClearFilter,
                                    BUTTON_WIDTH,
                                );
                            });

                        for index in 0..LOG_VIEW_LINES {
                            log_panel.spawn((
                                TextBundle::from_section(
                                    "",
                                    TextStyle {
                                        font_size: VALUE_FONT,
                                        color: theme.text,
                                        ..default()
                                    },
                                ),
                                EventLogLine(index),
                            ));
                        }
                    });
            });
        });
}

fn spawn_selector_row(
    parent: &mut ChildBuilder,
    theme: &UiTheme,
    label: &str,
    prev_action: Action,
    next_action: Action,
    value_field: ValueField,
) {
    parent
        .spawn(NodeBundle {
            style: row_style(),
            background_color: theme.row.into(),
            border_color: theme.row_border.into(),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                TextBundle::from_section(
                    label,
                    TextStyle {
                        font_size: LABEL_FONT,
                        color: theme.accent,
                        ..default()
                    },
                )
                .with_style(label_style()),
                Name::new(format!("label_{label}")),
            ));
            row.spawn(NodeBundle {
                style: Style {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(6.0),
                    ..default()
                },
                ..default()
            })
            .with_children(|cluster| {
                spawn_small_button(cluster, theme, "<", prev_action);
                cluster.spawn((
                    TextBundle::from_section(
                        "",
                        TextStyle {
                            font_size: VALUE_FONT,
                            color: theme.text,
                            ..default()
                        },
                    )
                    .with_text_justify(JustifyText::Center)
                    .with_style(Style {
                        width: Val::Px(VALUE_MIN_WIDTH),
                        ..default()
                    }),
                    ValueLabel(value_field),
                ));
                spawn_small_button(cluster, theme, ">", next_action);
            });
        });
}

fn spawn_toggle_row(
    parent: &mut ChildBuilder,
    theme: &UiTheme,
    label: &str,
    action: Action,
    value_field: ValueField,
) {
    parent
        .spawn(NodeBundle {
            style: row_style(),
            background_color: theme.row.into(),
            border_color: theme.row_border.into(),
            ..default()
        })
        .with_children(|row| {
            row.spawn(
                TextBundle::from_section(
                    label,
                    TextStyle {
                        font_size: LABEL_FONT,
                        color: theme.accent,
                        ..default()
                    },
                )
                .with_style(label_style()),
            );
            spawn_action_button(row, theme, "Toggle", action, BUTTON_WIDTH);
            row.spawn((
                TextBundle::from_section(
                    "",
                    TextStyle {
                        font_size: VALUE_FONT,
                        color: theme.text,
                        ..default()
                    },
                )
                .with_style(Style {
                    margin: UiRect::left(Val::Px(8.0)),
                    ..default()
                }),
                ValueLabel(value_field),
            ));
        });
}

fn row_style() -> Style {
    Style {
        width: Val::Percent(100.0),
        height: Val::Px(ROW_HEIGHT),
        justify_content: JustifyContent::FlexStart,
        align_items: AlignItems::Center,
        column_gap: Val::Px(8.0),
        padding: UiRect::axes(Val::Px(10.0), Val::Px(2.0)),
        border: UiRect::all(Val::Px(1.0)),
        ..default()
    }
}

fn label_style() -> Style {
    Style {
        width: Val::Px(LABEL_WIDTH),
        ..default()
    }
}

fn spawn_small_button(parent: &mut ChildBuilder, theme: &UiTheme, text: &str, action: Action) {
    spawn_action_button(parent, theme, text, action, SMALL_BUTTON_WIDTH);
}

/// Spawn the Edit Filter button with an extra marker component so the filter
/// sync system can highlight it while filter-edit mode is active.
fn spawn_filter_edit_button(parent: &mut ChildBuilder, theme: &UiTheme) {
    parent
        .spawn((
            ButtonBundle {
                style: Style {
                    width: Val::Px(BUTTON_WIDTH),
                    height: Val::Px(28.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                background_color: theme.button.into(),
                ..default()
            },
            ActionButton {
                action: Action::ToggleFilterFocus,
            },
            FilterEditButton,
        ))
        .with_children(|button| {
            button.spawn(TextBundle::from_section(
                "Edit Filter",
                TextStyle {
                    font_size: 13.0,
                    color: theme.text,
                    ..default()
                },
            ));
        });
}

fn spawn_action_button(
    parent: &mut ChildBuilder,
    theme: &UiTheme,
    text: &str,
    action: Action,
    width: f32,
) {
    parent
        .spawn((
            ButtonBundle {
                style: Style {
                    width: Val::Px(width),
                    height: Val::Px(28.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                background_color: theme.button.into(),
                ..default()
            },
            ActionButton { action },
        ))
        .with_children(|button| {
            button.spawn(TextBundle::from_section(
                text,
                TextStyle {
                    font_size: 13.0,
                    color: theme.text,
                    ..default()
                },
            ));
        });
}

fn button_interactions(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor, &ActionButton),
        (Changed<Interaction>, With<Button>),
    >,
    theme: Res<UiTheme>,
    mut config: ResMut<LauncherConfig>,
    mut exits: EventWriter<AppExit>,
) {
    for (interaction, mut color, action) in &mut buttons {
        match *interaction {
            Interaction::Pressed => {
                *color = theme.button_pressed.into();
                apply_action(action.action, &mut config, &mut exits);
            }
            Interaction::Hovered => {
                *color = theme.button_hover.into();
            }
            Interaction::None => {
                *color = theme.button.into();
            }
        }
    }
}

fn apply_action(action: Action, config: &mut LauncherConfig, exits: &mut EventWriter<AppExit>) {
    match action {
        Action::PortPrev => step_index(&mut config.selected_port, config.ports.len(), -1),
        Action::PortNext => step_index(&mut config.selected_port, config.ports.len(), 1),
        Action::BaudPrev => step_index(&mut config.selected_baud, config.baud_rates.len(), -1),
        Action::BaudNext => step_index(&mut config.selected_baud, config.baud_rates.len(), 1),
        Action::DataBitsPrev => {
            config.data_bits = if config.data_bits <= 5 {
                8
            } else {
                config.data_bits - 1
            };
        }
        Action::DataBitsNext => {
            config.data_bits = if config.data_bits >= 8 {
                5
            } else {
                config.data_bits + 1
            };
        }
        Action::ParityPrev => config.parity = config.parity.step(-1),
        Action::ParityNext => config.parity = config.parity.step(1),
        Action::StopBitsPrev => config.stop_bits = config.stop_bits.step(-1),
        Action::StopBitsNext => config.stop_bits = config.stop_bits.step(1),
        Action::FlowPrev => config.flow = config.flow.step(-1),
        Action::FlowNext => config.flow = config.flow.step(1),
        Action::ReconnectDelayPrev => step_index(
            &mut config.selected_reconnect_delay,
            config.reconnect_delays_ms.len(),
            -1,
        ),
        Action::ReconnectDelayNext => step_index(
            &mut config.selected_reconnect_delay,
            config.reconnect_delays_ms.len(),
            1,
        ),
        Action::ToggleCrlf => config.crlf = !config.crlf,
        Action::ToggleLocalEcho => config.local_echo = !config.local_echo,
        Action::ToggleLog => config.log_enabled = !config.log_enabled,
        Action::ToggleReconnect => {
            config.auto_reconnect = !config.auto_reconnect;
            config.status = if config.auto_reconnect {
                "auto reconnect enabled".to_string()
            } else {
                "auto reconnect disabled".to_string()
            };
        }
        Action::ToggleFilterFocus => {
            config.log_filter_active = !config.log_filter_active;
            config.status = if config.log_filter_active {
                "filter editing enabled (type to search, Enter/Esc to stop)".to_string()
            } else {
                "filter editing disabled".to_string()
            };
        }
        Action::ClearFilter => {
            config.clear_filter();
            config.log_filter_active = true;
            config.status = "filter cleared".to_string();
        }
        Action::ToggleLogAutoRefresh => {
            config.log_auto_refresh = !config.log_auto_refresh;
            config.status = if config.log_auto_refresh {
                "log auto-refresh enabled".to_string()
            } else {
                "log auto-refresh paused".to_string()
            };
        }
        Action::RefreshPorts => config.refresh_ports(),
        Action::RefreshLog => {
            let s = config.refresh_log_view();
            config.status = s;
        }
        Action::ExportLog => config.export_log_view(),
        Action::Connect => {
            // De-dup: if a child is still running, don't spawn another.
            let already_running = match config.active_child.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(_)) => {
                        // Previous TUI already exited; clear the slot.
                        config.active_child = None;
                        false
                    }
                    Ok(None) => true,
                    Err(_) => {
                        // Can't query — assume gone.
                        config.active_child = None;
                        false
                    }
                },
                None => false,
            };

            if already_running {
                config.status =
                    "A TUI session is already running. Close that window first.".to_string();
                return;
            }

            let Some(port) = config.selected_port_name().map(str::to_string) else {
                config.status =
                    "No port selected. Connect a device and click Refresh Ports.".to_string();
                return;
            };

            match launch_rustyserial(&port, config) {
                Ok(child) => {
                    config.active_child = Some(child);
                    config.connect_launch_count =
                        config.connect_launch_count.saturating_add(1);
                    config.status = if config.auto_reconnect {
                        format!(
                            "Launched on {} @ {} bps (auto reconnect {} ms)",
                            port,
                            config.selected_baud_value(),
                            config.selected_reconnect_delay_value()
                        )
                    } else {
                        format!(
                            "Launched on {} @ {} bps (reconnect off)",
                            port,
                            config.selected_baud_value()
                        )
                    };
                }
                Err(err) => {
                    config.status = format!("Launch failed: {err}");
                }
            }
        }
        Action::Quit => {
            exits.send(AppExit::Success);
        }
    }
}

fn launch_rustyserial(port: &str, config: &LauncherConfig) -> Result<Child, String> {
    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_dir = current_exe
        .parent()
        .ok_or_else(|| "could not derive executable directory".to_string())?;
    let bin_name = if cfg!(windows) {
        "rustyserial.exe"
    } else {
        "rustyserial"
    };
    let rustyserial_path = exe_dir.join(bin_name);

    if !rustyserial_path.exists() {
        return Err(format!(
            "{} not found at {}. Build both binaries first.",
            bin_name,
            rustyserial_path.display()
        ));
    }

    let mut cmd = Command::new(rustyserial_path);
    cmd.current_dir(exe_dir);
    cmd.arg(port)
        .arg("--baud")
        .arg(config.selected_baud_value().to_string())
        .arg("--data-bits")
        .arg(config.data_bits.to_string())
        .arg("--parity")
        .arg(config.parity.as_flag())
        .arg("--stop-bits")
        .arg(config.stop_bits.as_flag());

    match config.flow {
        FlowControlChoice::None => {}
        FlowControlChoice::RtsCts => {
            cmd.arg("--rtscts");
        }
        FlowControlChoice::XonXoff => {
            cmd.arg("--xonxoff");
        }
    }

    if config.crlf {
        cmd.arg("--crlf");
    }
    if config.local_echo {
        cmd.arg("--local-echo");
    }
    if config.log_enabled {
        // Pass an absolute path so the TUI writes the log to the same place
        // the GUI reads it from, regardless of either process's CWD.
        let resolved = config.resolved_log_path();
        cmd.arg("--log").arg(resolved);
    }
    if !config.auto_reconnect {
        cmd.arg("--no-reconnect");
    }
    cmd.arg("--reconnect-delay-ms")
        .arg(config.selected_reconnect_delay_value().to_string());

    #[cfg(windows)]
    {
        // Open the TUI in its own terminal window so it is always visible.
        cmd.creation_flags(CREATE_NEW_CONSOLE);
    }

    cmd.spawn().map_err(|e| e.to_string())
}

fn sync_value_labels(config: Res<LauncherConfig>, mut labels: Query<(&ValueLabel, &mut Text)>) {
    if !config.is_changed() {
        return;
    }

    for (tag, mut text) in &mut labels {
        text.sections[0].value = match tag.0 {
            ValueField::Port => config
                .selected_port_name()
                .map(str::to_string)
                .unwrap_or_else(|| "No ports detected".to_string()),
            ValueField::Baud => config.selected_baud_value().to_string(),
            ValueField::DataBits => config.data_bits.to_string(),
            ValueField::Parity => config.parity.label().to_string(),
            ValueField::StopBits => config.stop_bits.label().to_string(),
            ValueField::Flow => config.flow.label().to_string(),
            ValueField::Crlf => on_off(config.crlf),
            ValueField::LocalEcho => on_off(config.local_echo),
            ValueField::Log => {
                if config.log_enabled {
                    format!("On ({})", config.log_path)
                } else {
                    "Off".to_string()
                }
            }
            ValueField::Reconnect => on_off(config.auto_reconnect),
            ValueField::ReconnectDelay => format!("{} ms", config.selected_reconnect_delay_value()),
            ValueField::Status => config.status.clone(),
        };
    }
}

fn sync_log_view(config: Res<LauncherConfig>, mut lines: Query<(&EventLogLine, &mut Text)>) {
    if !config.is_changed() {
        return;
    }

    for (line, mut text) in &mut lines {
        let value = config
            .log_view_lines
            .iter()
            .rev()
            .nth(line.0)
            .map_or("".to_string(), |entry| entry.clone());
        text.sections[0].value = value;
    }
}

fn sync_stats_strip(
    config: Res<LauncherConfig>,
    mut stats: Query<&mut Text, (With<StatsLine>, Without<LogFilterText>)>,
    mut filter_text: Query<&mut Text, (With<LogFilterText>, Without<StatsLine>)>,
) {
    if !config.is_changed() {
        return;
    }

    if let Ok(mut text) = stats.get_single_mut() {
        let filter_label = if config.log_filter_active {
            "edit"
        } else {
            "idle"
        };
        let refresh_label = if config.log_auto_refresh {
            "auto"
        } else {
            "manual"
        };
        let reconnect_label = if config.auto_reconnect {
            "reconnect on"
        } else {
            "reconnect off"
        };

        text.sections = vec![
            TextSection::new(
                format!("Port {}", config.selected_port_name().unwrap_or("none")),
                TextStyle {
                    font_size: 12.0,
                    color: Color::WHITE,
                    ..default()
                },
            ),
            TextSection::new(
                "   ",
                TextStyle {
                    font_size: 12.0,
                    color: Color::WHITE,
                    ..default()
                },
            ),
            TextSection::new(
                format!("Baud {}", config.selected_baud_value()),
                TextStyle {
                    font_size: 12.0,
                    color: Color::WHITE,
                    ..default()
                },
            ),
            TextSection::new(
                "   ",
                TextStyle {
                    font_size: 12.0,
                    color: Color::WHITE,
                    ..default()
                },
            ),
            TextSection::new(
                format!(
                    "{} | refresh {} | log {} | launches {}",
                    reconnect_label,
                    refresh_label,
                    config.log_last_refresh_count,
                    config.connect_launch_count
                ),
                TextStyle {
                    font_size: 12.0,
                    color: Color::WHITE,
                    ..default()
                },
            ),
            TextSection::new(
                "   ",
                TextStyle {
                    font_size: 12.0,
                    color: Color::WHITE,
                    ..default()
                },
            ),
            TextSection::new(
                format!(
                    "Filter {}: {}",
                    filter_label,
                    if config.log_filter.is_empty() {
                        "(empty)"
                    } else {
                        &config.log_filter
                    }
                ),
                TextStyle {
                    font_size: 12.0,
                    color: Color::WHITE,
                    ..default()
                },
            ),
        ];
    }

    if let Ok(mut text) = filter_text.get_single_mut() {
        let prompt = if config.log_filter_active {
            "type to filter"
        } else {
            "click Edit Filter"
        };

        text.sections[0].value = if config.log_filter.is_empty() {
            format!("{}: (empty)", prompt)
        } else {
            format!("{}: {}", prompt, config.log_filter)
        };
    }
}

fn auto_refresh_log_view(
    time: Res<Time>,
    mut timer: ResMut<LogRefreshTimer>,
    mut config: ResMut<LauncherConfig>,
) {
    if !config.log_auto_refresh {
        return;
    }

    if timer.0.tick(time.delta()).just_finished() {
        // Discard the returned status: auto-refresh runs every second and we
        // don't want it stomping the user's last action feedback.
        let _ = config.refresh_log_view();
    }
}

/// Poll the spawned TUI child periodically and drop the handle once it has
/// exited, so the de-dup check on Connect can detect that the slot is free.
fn reap_active_child(mut config: ResMut<LauncherConfig>) {
    if let Some(child) = config.active_child.as_mut() {
        if matches!(child.try_wait(), Ok(Some(_)) | Err(_)) {
            config.active_child = None;
        }
    }
}

fn log_filter_input(
    mut config: ResMut<LauncherConfig>,
    mut keyboard_events: EventReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if !config.log_filter_active {
        return;
    }

    for event in keyboard_events.read() {
        if !event.state.is_pressed() {
            continue;
        }

        if let Key::Character(text) = &event.logical_key {
            for ch in text.chars() {
                if !ch.is_control() {
                    config.set_filter_char(ch);
                }
            }
        }
    }

    if keys.just_pressed(KeyCode::Backspace) {
        config.backspace_filter();
    }
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Escape) {
        config.log_filter_active = false;
        config.status = "filter editing disabled".to_string();
    }
}

/// Highlight the Edit Filter button while filter-edit mode is active so the
/// user has a visual cue beyond the prompt text in the filter row.
fn sync_filter_button(
    config: Res<LauncherConfig>,
    theme: Res<UiTheme>,
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (With<FilterEditButton>, With<Button>),
    >,
) {
    if !config.is_changed() {
        return;
    }
    for (interaction, mut color) in &mut buttons {
        // Don't fight the hover/press colors driven by button_interactions.
        if !matches!(*interaction, Interaction::None) {
            continue;
        }
        *color = if config.log_filter_active {
            theme.button_pressed.into()
        } else {
            theme.button.into()
        };
    }
}

fn adaptive_ui_scale(windows: Query<&Window>, mut ui_scale: ResMut<UiScale>) {
    let Ok(window) = windows.get_single() else {
        return;
    };

    // Keep the form near native size for typical windows and only shrink
    // when dimensions become constrained. Baselines match the default window
    // resolution (1080x740) so the launcher renders at 1.0 out of the box.
    let scale_from_height = window.height() / 740.0;
    let scale_from_width = window.width() / 1080.0;
    let target = scale_from_height.min(scale_from_width).clamp(0.72, 1.10);
    if (ui_scale.0 - target).abs() > 0.01 {
        ui_scale.0 = target;
    }
}

fn discover_ports() -> Vec<String> {
    let mut ports = tokio_serial::available_ports()
        .map(|items| items.into_iter().map(|p| p.port_name).collect::<Vec<_>>())
        .unwrap_or_default();
    ports.sort();
    ports
}

fn load_log_view(path: &Path) -> Result<VecDeque<String>, String> {
    if !path.exists() {
        return Ok(VecDeque::new());
    }

    let mut file = fs::File::open(path)
        .map_err(|e| format!("failed to open log {}: {}", path.display(), e))?;
    let size = file
        .metadata()
        .map_err(|e| format!("failed to read log metadata {}: {}", path.display(), e))?
        .len();
    let start = size.saturating_sub(LOG_TAIL_BYTES as u64);
    file.seek(SeekFrom::Start(start))
        .map_err(|e| format!("failed to seek log {}: {}", path.display(), e))?;

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| format!("failed to read log {}: {}", path.display(), e))?;

    let mut text = String::from_utf8_lossy(&bytes).to_string();
    if start > 0 {
        if let Some(index) = text.find('\n') {
            text = text[index + 1..].to_string();
        } else {
            text.clear();
        }
    }

    // Keep a wider source buffer than the visible 8 lines so the filter can
    // report honest match counts across the recent history, not just the
    // current visible window.
    let mut lines = VecDeque::new();
    for line in text
        .lines()
        .rev()
        .take(LOG_SOURCE_LINES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        lines.push_back(line.to_string());
    }

    Ok(lines)
}

fn export_log(path: &Path) -> Result<PathBuf, String> {
    let source = path;
    if !source.exists() {
        return Err(format!("no source log file exists at {}", source.display()));
    }

    let export_name = format!(
        "rustyserial-export-{}.log",
        Local::now().format("%Y%m%d-%H%M%S")
    );
    let export_path = source
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(export_name);

    fs::copy(source, &export_path)
        .map_err(|e| format!("failed to export log to {}: {}", export_path.display(), e))?;

    Ok(export_path)
}

fn on_off(value: bool) -> String {
    if value {
        "On".to_string()
    } else {
        "Off".to_string()
    }
}

fn step_index(current: &mut usize, len: usize, direction: i8) {
    if len == 0 {
        *current = 0;
        return;
    }

    if direction < 0 {
        *current = if *current == 0 { len - 1 } else { *current - 1 };
    } else {
        *current = (*current + 1) % len;
    }
}

fn step_enum<T: Copy + PartialEq>(items: &[T], current: T, dir: i8) -> T {
    let index = items
        .iter()
        .position(|value| *value == current)
        .unwrap_or(0);
    let len = items.len();
    let next = if dir < 0 {
        if index == 0 {
            len - 1
        } else {
            index - 1
        }
    } else {
        (index + 1) % len
    };
    items[next]
}
