use super::event::Event;
use super::state::TuiState;
use ansi_to_tui::IntoText;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use miette::IntoDiagnostic;
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
        KeyCode::Down => {
            if !state.is_building_package() {
                state.vertical_scroll = 0;
                state.selected_package = if state.selected_package >= state.packages.len() - 1 {
                    0
                } else {
                    state.selected_package + 1
                }
            }
        }
        KeyCode::PageUp => {
            state.vertical_scroll += 5;
        }
        KeyCode::Up => {
            if !state.is_building_package() {
                state.vertical_scroll = 0;
                state.selected_package = if state.selected_package == 0 {
                    state.packages.len() - 1
                } else {
                    state.selected_package - 1
                }
            }
        }
        KeyCode::PageDown => {
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
        let rects =
            Layout::vertical([Constraint::Min(2)].repeat(((rects[0].height - 2) / 3) as usize))
                .margin(1)
                .split(rects[0]);

        for (i, package) in state.packages.iter_mut().enumerate() {
            frame.render_widget(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(if state.selected_package == i {
                        if package.build_progress.is_building() {
                            Style::new().green()
                        } else {
                            Style::new()
                        }
                    } else {
                        Style::new().black()
                    }),
                rects[0],
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

    let selected_package = state.packages[state.selected_package].clone();
    let logs = selected_package
        .build_log
        .join("")
        .into_text()
        .unwrap()
        .on_black();
    let vertical_scroll = (logs.height() as u16)
        .saturating_sub(rects[1].height)
        .saturating_sub(state.vertical_scroll);
    frame.render_widget(
        Paragraph::new(logs.clone())
            .block(
                Block::bordered()
                    .title_top(format!("Build Logs for {}", selected_package.name))
                    .border_type(BorderType::Rounded),
            )
            .scroll((vertical_scroll, 0)),
        rects[1],
    );

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));

    let mut scrollbar_state =
        ScrollbarState::new(selected_package.build_log.len()).position(vertical_scroll as usize);

    frame.render_stateful_widget(
        scrollbar,
        rects[1].inner(&Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut scrollbar_state,
    );
}
