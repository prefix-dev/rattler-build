use ansi_to_tui::IntoText;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use miette::IntoDiagnostic;
use ratatui::{
    Frame,
    crossterm::event::Event as CrosstermEvent,
    layout::{Alignment, Position},
    prelude::*,
    style::{Color, Style, Stylize},
    widgets::{Block, BorderType, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use tokio::sync::mpsc;
use tui_input::backend::crossterm::EventHandler;

use super::{event::Event, state::TuiState};

/// Key bindings.
const KEY_BINDINGS: &[(&str, &str)] = &[
    ("⏎ ", "Build"),
    ("a", "Build All"),
    ("e", "Edit Recipe"),
    ("c", "Console"),
    ("j", "Next"),
    ("k", "Prev"),
    ("↕ ↔ ", "Scroll"),
    ("q", "Quit"),
];

/// Handles the key events and updates the state.
pub(crate) fn handle_key_events(
    key_event: KeyEvent,
    sender: mpsc::UnboundedSender<Event>,
    state: &mut TuiState,
) -> miette::Result<()> {
    if state.input_mode {
        match key_event.code {
            KeyCode::Enter => sender.send(Event::HandleInput).into_diagnostic()?,
            KeyCode::Esc => {
                state.input_mode = false;
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                if key_event.modifiers == KeyModifiers::CONTROL {
                    state.input_mode = false;
                } else {
                    state.input.handle_event(&CrosstermEvent::Key(key_event));
                }
            }
            _ => {
                state.input.handle_event(&CrosstermEvent::Key(key_event));
            }
        }
        return Ok(());
    }
    match key_event.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            if state.input_mode {
                state.input_mode = false;
            } else {
                state.quit();
            }
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
            if key_event.modifiers == KeyModifiers::CONTROL {
                state.quit();
            } else {
                state.input_mode = true;
            }
        }
        KeyCode::Char('j') => {
            state.vertical_scroll = 0;
            state.selected_package = if state.selected_package >= state.packages.len() - 1 {
                0
            } else {
                state.selected_package + 1
            }
        }
        KeyCode::Up => {
            state.vertical_scroll += 5;
        }
        KeyCode::Char('k') => {
            state.vertical_scroll = 0;
            state.selected_package = if state.selected_package == 0 {
                state.packages.len() - 1
            } else {
                state.selected_package - 1
            }
        }
        KeyCode::Down => {
            if state.vertical_scroll > 1 {
                state.vertical_scroll = state.vertical_scroll.saturating_sub(5);
            }
        }
        KeyCode::Right => {
            state.horizontal_scroll += 5;
        }
        KeyCode::Left => {
            state.horizontal_scroll = state.horizontal_scroll.saturating_sub(5);
        }
        KeyCode::Char('a') => sender.send(Event::StartBuildQueue).into_diagnostic()?,
        KeyCode::Enter => sender
            .send(Event::StartBuild(state.selected_package))
            .into_diagnostic()?,
        KeyCode::Char(':') => {
            state.input.reset();
            state.input_mode = true;
        }
        KeyCode::Char('e') => sender.send(Event::EditRecipe).into_diagnostic()?,
        _ => {}
    }
    Ok(())
}

/// Handles the mouse events and updates the state.
pub(crate) fn handle_mouse_events(
    mouse_event: MouseEvent,
    sender: mpsc::UnboundedSender<Event>,
    state: &mut TuiState,
) -> miette::Result<()> {
    match mouse_event.kind {
        MouseEventKind::ScrollDown => {
            if state.vertical_scroll > 1 {
                state.vertical_scroll = state.vertical_scroll.saturating_sub(5);
            }
        }
        MouseEventKind::ScrollUp => {
            state.vertical_scroll += 5;
        }
        MouseEventKind::ScrollRight => {
            state.horizontal_scroll += 5;
        }
        MouseEventKind::ScrollLeft => {
            state.horizontal_scroll = state.horizontal_scroll.saturating_sub(5);
        }
        MouseEventKind::Moved => {
            let p = Position::new(mouse_event.column, mouse_event.row);
            state.packages.iter_mut().for_each(|package| {
                package.is_hovered = package.area.contains(p);
            })
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(selected_pos) = state.packages.iter().position(|p| p.is_hovered) {
                sender
                    .send(Event::StartBuild(selected_pos))
                    .into_diagnostic()?
            }
        }
        _ => {}
    }
    Ok(())
}

/// Renders the user interface widgets.
pub(crate) fn render_widgets(state: &mut TuiState, frame: &mut Frame) {
    frame.render_widget(
        Block::new()
            .title_top(Line::from("rattler-build-tui").style(Style::default().bold()))
            .title_alignment(Alignment::Center),
        frame.area(),
    );
    let rects = Layout::vertical([Constraint::Percentage(100), Constraint::Min(3)])
        .margin(1)
        .split(frame.area());
    frame.render_widget(
        Paragraph::new(
            Line::default()
                .spans(
                    KEY_BINDINGS
                        .iter()
                        .flat_map(|(key, desc)| {
                            vec![
                                "<".fg(Color::Rgb(100, 100, 100)),
                                key.yellow(),
                                ": ".fg(Color::Rgb(100, 100, 100)),
                                Span::from(*desc),
                                "> ".fg(Color::Rgb(100, 100, 100)),
                            ]
                        })
                        .collect::<Vec<Span>>(),
                )
                .alignment(Alignment::Center),
        )
        .block(
            Block::bordered()
                .title_bottom(Line::from(format!("|{}|", env!("CARGO_PKG_VERSION"))))
                .title_alignment(Alignment::Right)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(100, 100, 100))),
        ),
        rects[1],
    );
    frame.render_widget(
        Block::new()
            .title_top(Line::from("rattler-build-tui").style(Style::default().bold()))
            .title_alignment(Alignment::Center),
        rects[0],
    );
    let rects = Layout::horizontal([Constraint::Percentage(20), Constraint::Percentage(80)])
        .split(rects[0]);
    {
        frame.render_widget(
            Block::bordered()
                .title_top("|Packages|".yellow())
                .title_alignment(Alignment::Center)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(100, 100, 100))),
            rects[0],
        );

        if !state.packages.is_empty() {
            let item_count = ((rects[0].height - 2) / 3) as usize;
            let start_offset = (state.selected_package + 1).saturating_sub(item_count);
            let rects = Layout::vertical([Constraint::Min(2)].repeat(item_count))
                .margin(1)
                .split(rects[0]);
            for (i, package) in state
                .packages
                .iter_mut()
                .skip(start_offset)
                .take(item_count)
                .enumerate()
            {
                package.area = rects[i];
                frame.render_widget(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .border_style({
                            let mut style = Style::new().fg(package.build_progress.as_color());
                            if package.is_hovered && !package.build_progress.is_building() {
                                style = style.yellow()
                            } else if state.selected_package == i + start_offset {
                                if package.build_progress.is_building() {
                                    style = style.green()
                                } else {
                                    style = style.white();
                                }
                            }
                            style
                        }),
                    rects[i],
                );
                let item = Layout::horizontal([Constraint::Min(3), Constraint::Percentage(100)])
                    .margin(1)
                    .split(rects[i]);
                frame.render_stateful_widget(
                    throbber_widgets_tui::Throbber::default()
                        .style(Style::default().fg(Color::Cyan))
                        .throbber_style(
                            Style::default()
                                .fg(package.build_progress.as_color())
                                .add_modifier(Modifier::BOLD),
                        )
                        .throbber_set(throbber_widgets_tui::BLACK_CIRCLE)
                        .use_type(throbber_widgets_tui::WhichUse::Spin),
                    item[0],
                    &mut package.spinner_state,
                );
                let mut line = Line::from(vec![
                    package.name.clone().into(),
                    "-".fg(Color::Rgb(100, 100, 100)),
                    package.version.clone().into(),
                    format!(
                        "{}{}",
                        "-".fg(Color::Rgb(100, 100, 100)),
                        &package.build_string
                    )
                    .into(),
                ]);
                if item[1].width < line.width() as u16 {
                    line = Line::from(vec![
                        package.name.clone().into(),
                        "-".fg(Color::Rgb(100, 100, 100)),
                        package.version.clone().into(),
                    ]);
                }
                frame.render_widget(Paragraph::new(line), item[1]);
            }
        }
    }

    let mut log_lines = state.log.clone();
    if let Some(selected_package) = state.packages.get(state.selected_package) {
        log_lines.extend(selected_package.build_log.clone());
    }
    let log_lines = log_lines
        .iter()
        .map(|l| l.trim_end())
        .collect::<Vec<&str>>();
    let logs = log_lines.join("\n").into_text().unwrap().on_black();
    let vertical_scroll = (logs.height() as u16)
        .saturating_sub(rects[1].height.saturating_sub(3))
        .saturating_sub(state.vertical_scroll);
    if vertical_scroll == 0 {
        state.vertical_scroll =
            (logs.height() as u16).saturating_sub(rects[1].height.saturating_sub(3));
    }

    let logs_rect = if state.input_mode {
        let rects =
            Layout::vertical([Constraint::Percentage(100), Constraint::Min(3)]).split(rects[1]);
        frame.render_widget(
            Paragraph::new(Line::from(vec!["> ".yellow(), state.input.value().into()])).block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Rgb(100, 100, 100))),
            ),
            rects[1],
        );
        frame.set_cursor_position(Position::new(
            rects[1].x + state.input.visual_cursor() as u16 + 3,
            rects[1].y + 1,
        ));
        rects[0]
    } else {
        rects[1]
    };

    frame.render_widget(
        Paragraph::new(logs.clone())
            .block(
                Block::bordered()
                    .title_top(
                        match state.packages.get(state.selected_package) {
                            Some(package) => {
                                format!("|Build Logs for {}|", package.name)
                            }
                            None => String::from("|Build Logs|"),
                        }
                        .yellow(),
                    )
                    .title_alignment(Alignment::Left)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Rgb(100, 100, 100))),
            )
            .scroll((vertical_scroll, state.horizontal_scroll)),
        logs_rect,
    );

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));

    let mut scrollbar_state =
        ScrollbarState::new(logs.height().saturating_sub(logs_rect.height.into()))
            .position(vertical_scroll.into());

    frame.render_stateful_widget(
        scrollbar,
        logs_rect.inner(Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut scrollbar_state,
    );

    let scrollbar = Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
        .thumb_symbol("🬋")
        .begin_symbol(Some("←"))
        .end_symbol(Some("→"));

    let max_width = logs
        .lines
        .iter()
        .map(|l| l.width())
        .max()
        .unwrap_or_default();
    let content_length = max_width.saturating_sub(logs_rect.width.saturating_sub(2).into());
    if content_length == 0 {
        state.horizontal_scroll = 0;
    }
    let mut scrollbar_state =
        ScrollbarState::new(content_length).position(state.horizontal_scroll.into());

    frame.render_stateful_widget(
        scrollbar,
        logs_rect.inner(Margin {
            vertical: 0,
            horizontal: 1,
        }),
        &mut scrollbar_state,
    );
}
