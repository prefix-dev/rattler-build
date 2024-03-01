use super::event::Event;
use super::state::{BuildProgress, TuiState};
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
use std::time::Instant;
use tokio::sync::mpsc;

/// Spinner frames.
///
/// See <https://github.com/sindresorhus/cli-spinners/blob/main/spinners.json> for alternatives.
const SPINNER_FRAMES: &[&str] = &["◢", "◣", "◤", "◥"];

/// Spinner interval.
const SPINNER_INTERVAL: u128 = 50;

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
        KeyCode::PageDown => {
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
        KeyCode::PageUp => {
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
            .style(Style::default().fg(Color::Yellow).bg(Color::Black)),
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

        for (i, package) in state.packages.iter().enumerate() {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    match package.build_progress {
                        BuildProgress::None => Span::from("◉ ").red(),
                        BuildProgress::Building => Span::from("◉ ").yellow(),
                        BuildProgress::Done => Span::from("◉ ").green(),
                    },
                    package.name.to_string().into(),
                    if package.build_progress == BuildProgress::Building {
                        if state.spinner_last_tick.elapsed().as_millis() > SPINNER_INTERVAL {
                            state.spinner_last_tick = Instant::now();
                            state.spinner_frame = if state.spinner_frame < SPINNER_FRAMES.len() - 1
                            {
                                state.spinner_frame + 1
                            } else {
                                0
                            };
                        }
                        format!(" {}", SPINNER_FRAMES[state.spinner_frame]).cyan()
                    } else {
                        String::new().into()
                    },
                ]))
                .block(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .border_style(if state.selected_package == i {
                            if state.is_building_package() {
                                Style::new().green()
                            } else {
                                Style::new()
                            }
                        } else {
                            Style::new().black()
                        }),
                ),
                rects[i],
            );
        }
    }

    let selected_package = state.packages[state.selected_package].clone();
    frame.render_widget(
        Paragraph::new(
            selected_package
                .build_log
                .join("\n")
                .into_text()
                .unwrap()
                .on_black(),
        )
        .block(
            Block::bordered()
                .title_top(format!("Build Logs for {}", selected_package.name))
                .border_type(BorderType::Rounded),
        )
        .scroll((state.vertical_scroll as u16, 0)),
        rects[1],
    );

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));

    let mut scrollbar_state =
        ScrollbarState::new(selected_package.build_log.len()).position(state.vertical_scroll);

    frame.render_stateful_widget(
        scrollbar,
        rects[1].inner(&Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut scrollbar_state,
    );
}
