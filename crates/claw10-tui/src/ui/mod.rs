use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub mod chat;
pub mod components;
pub mod home;
pub mod screens;

use crate::app::{CommandMode, Screen, Tab, TuiApp};
use crate::ui::chat::draw_chat;
use crate::ui::components::{draw_apikey_input, draw_command_palette, draw_model_selection};
use crate::ui::home::draw_home;
use crate::ui::screens::{
    draw_approvals, draw_artifacts, draw_costs, draw_incidents, draw_memory,
    draw_missions, draw_policies, draw_skills, draw_tasks,
};

pub fn draw(frame: &mut Frame, area: Rect, app: &TuiApp) {
    if app.active_screen == Screen::Home || app.active_screen == Screen::WorkspaceSelect {
        draw_home(frame, area, app);

        // Render modals di atas layar apa pun jika aktif
        if matches!(app.command_mode, CommandMode::CommandPalette { .. }) {
            draw_command_palette(frame, area, app);
        }
        if matches!(app.command_mode, CommandMode::ApiKeyInput { .. }) {
            draw_apikey_input(frame, area, app);
        }
        if matches!(app.command_mode, CommandMode::ModelSelection) {
            draw_model_selection(frame, area, app);
        }
        if matches!(app.command_mode, CommandMode::ManualModelInput { .. }) {
            crate::ui::components::draw_manual_model_input(frame, area, app);
        }
        return;
    }

    // Bagi area menjadi Top Bar, Pembatas, dan Content Area
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top Bar
            Constraint::Length(1), // Border pembatas
            Constraint::Min(0),    // Content Area
        ])
        .split(area);

    draw_top_bar(frame, chunks[0], app);

    // Garis horizontal pembatas top bar yang sleek & minimalis
    let border = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(Color::Rgb(50, 50, 50)));
    frame.render_widget(border, chunks[1]);

    let content_area = chunks[2];

    match app.active_screen {
        Screen::Home | Screen::WorkspaceSelect => unreachable!(),
        Screen::Chat => draw_chat(frame, content_area, app),
        Screen::Missions => draw_missions(frame, content_area, app),
        Screen::Tasks => draw_tasks(frame, content_area, app),
        Screen::Memory => draw_memory(frame, content_area, app),
        Screen::Approvals => draw_approvals(frame, content_area, app),
        Screen::Costs => draw_costs(frame, content_area, app),
        Screen::Policies => draw_policies(frame, content_area, app),
        Screen::Skills => draw_skills(frame, content_area, app),
        Screen::Artifacts => draw_artifacts(frame, content_area, app),
        Screen::Incidents => draw_incidents(frame, content_area, app),
    }

    // Render modals di atas layar apa pun jika aktif
    if matches!(app.command_mode, CommandMode::CommandPalette { .. }) {
        draw_command_palette(frame, area, app);
    }
    if matches!(app.command_mode, CommandMode::ApiKeyInput { .. }) {
        draw_apikey_input(frame, area, app);
    }
    if matches!(app.command_mode, CommandMode::ModelSelection) {
        draw_model_selection(frame, area, app);
    }
    if matches!(app.command_mode, CommandMode::ManualModelInput { .. }) {
        crate::ui::components::draw_manual_model_input(frame, area, app);
    }
}

fn draw_top_bar(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let active_tab = app.selected_tab;
    
    let is_chat_active = matches!(
        active_tab,
        Tab::Session | Tab::Agents | Tab::Workers | Tab::SpawnRequests
    );

    let create_tab_span = |name: &str, is_active: bool| -> Span {
        if is_active {
            Span::styled(
                format!("  [{}]  ", name),
                Style::default()
                    .fg(Color::Rgb(254, 192, 126))
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(
                format!("   {}   ", name),
                Style::default().fg(Color::DarkGray),
            )
        }
    };

    let line = Line::from(vec![
        Span::raw(" "),
        create_tab_span("Chat", is_chat_active),
        create_tab_span("Msn", active_tab == Tab::Missions),
        create_tab_span("Tasks", active_tab == Tab::Tasks),
        create_tab_span("Mem", active_tab == Tab::Memory),
        create_tab_span("Aprv", active_tab == Tab::Approvals),
        create_tab_span("Cost", active_tab == Tab::Costs),
        create_tab_span("Pol", active_tab == Tab::Policies),
        create_tab_span("Skl", active_tab == Tab::Skills),
        create_tab_span("Art", active_tab == Tab::Artifacts),
        create_tab_span("Inc", active_tab == Tab::Incidents),
    ]);

    let para = Paragraph::new(line)
        .style(Style::default().bg(Color::Rgb(15, 15, 15)));
    frame.render_widget(para, area);
}

/// Melakukan word-wrap manual per baris teks ke lebar maksimal `max_width`.
/// Menjaga line breaks yang ditulis secara eksplisit dari input.
pub fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }

        let words: Vec<&str> = paragraph.split_whitespace().collect();
        if words.is_empty() {
            lines.push(String::new());
            continue;
        }

        let mut current_line = String::new();
        for word in words {
            let space_needed = if current_line.is_empty() { 0 } else { 1 };
            if current_line.chars().count() + space_needed + word.chars().count() > max_width {
                // Baris penuh, flush ke lines
                lines.push(current_line);
                current_line = word.to_string();
            } else {
                if !current_line.is_empty() {
                    current_line.push(' ');
                }
                current_line.push_str(word);
            }
        }
        if !current_line.is_empty() {
            lines.push(current_line);
        }
    }
    lines
}
