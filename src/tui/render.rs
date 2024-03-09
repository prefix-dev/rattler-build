use super::event::Event;
use super::state::TuiState;
use ansi_to_tui::IntoText;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use miette::IntoDiagnostic;
use ratatui::layout::Position;
use ratatui::prelude::*;
use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::{
    layout::Alignment,
    style::{Color, Style},
    widgets::{Block, BorderType, Paragraph},
    Frame,
};
use tokio::sync::mpsc;

/// Handles the key events and updates the state.
pub(crate) fn handle_key_events(
    key_event: KeyEvent,
    sender: mpsc::UnboundedSender<Event>,
    state: &mut TuiState,
) -> miette::Result<()> {
    match key_event.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            state.quit();
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
            if key_event.modifiers == KeyModifiers::CONTROL {
                state.quit();
            }
        }
        KeyCode::Char('j') => {
            if !state.is_building_package() {
                state.vertical_scroll = 0;
                state.selected_package = if state.selected_package >= state.packages.len() - 1 {
                    0
                } else {
                    state.selected_package + 1
                }
            }
        }
        KeyCode::Up => {
            state.vertical_scroll += 5;
        }
        KeyCode::Char('k') => {
            if !state.is_building_package() {
                state.vertical_scroll = 0;
                state.selected_package = if state.selected_package == 0 {
                    state.packages.len() - 1
                } else {
                    state.selected_package - 1
                }
            }
        }
        KeyCode::Down => {
            if state.vertical_scroll > 1 {
                state.vertical_scroll -= 5;
            }
        }
        KeyCode::Enter => sender
            .send(Event::StartBuild(state.selected_package))
            .into_diagnostic()?,
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
                state.vertical_scroll -= 5;
            }
        }
        MouseEventKind::ScrollUp => {
            state.vertical_scroll += 5;
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
            .title_top("rattler-build-tui")
            .title_bottom(env!("CARGO_PKG_VERSION"))
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::Yellow)),
        frame.size(),
    );
    let rects = Layout::horizontal([Constraint::Percentage(20), Constraint::Percentage(80)])
        .margin(1)
        .split(frame.size());
    {
        frame.render_widget(
            Block::bordered()
                .title_top("Packages")
                .title_alignment(Alignment::Center)
                .border_type(BorderType::Rounded),
            rects[0],
        );

        if !state.packages.is_empty() {
            let rects =
                Layout::vertical([Constraint::Min(2)].repeat(((rects[0].height - 2) / 3) as usize))
                    .margin(1)
                    .split(rects[0]);
            for (i, package) in state.packages.iter_mut().enumerate() {
                package.area = rects[i];
                frame.render_widget(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .border_style({
                            let mut style = Style::new();
                            if package.is_hovered {
                                style = style.white()
                            } else if state.selected_package == i {
                                if package.build_progress.is_building() {
                                    style = style.green()
                                }
                            } else {
                                style = style.black()
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
                frame.render_widget(Paragraph::new(package.name.to_string()), item[1]);
            }
        }
    }

    let mut log_lines = state.log.clone();
    if let Some(selected_package) = state.packages.get(state.selected_package) {
        log_lines.extend(selected_package.build_log.clone());
    }
    let logs = log_lines.join("").into_text().unwrap().on_black();

    let vertical_scroll = (logs.height() as u16)
        .saturating_sub(rects[1].height)
        .saturating_sub(state.vertical_scroll);
    frame.render_widget(
        Paragraph::new(logs.clone())
            .block(
                Block::bordered()
                    .title_top(match state.packages.get(state.selected_package) {
                        Some(package) => {
                            format!("Build Logs for {}", package.name)
                        }
                        None => String::from("Build Logs"),
                    })
                    .border_type(BorderType::Rounded),
            )
            .scroll((vertical_scroll, 0)),
        rects[1],
    );

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));

    let mut scrollbar_state =
        ScrollbarState::new(log_lines.len()).position(vertical_scroll as usize);

    frame.render_stateful_widget(
        scrollbar,
        rects[1].inner(&Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut scrollbar_state,
    );
}
