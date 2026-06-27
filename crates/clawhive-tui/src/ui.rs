use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use crate::app::{InputMode, Tab, TuiApp};

fn header(area: Rect, frame: &mut Frame, _app: &TuiApp) {
    let title = format!(" ClawHive OS TUI v0.1.0 ");
    let info = " :cmd  Tab:switch  ↑↓:nav  q:quit ";

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(title, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
        .title_alignment(ratatui::layout::Alignment::Left);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = Paragraph::new(Line::from(Span::raw(info)))
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(text, inner);
}

fn agent_list(area: Rect, frame: &mut Frame, app: &TuiApp) {
    let title = format!(" Agents ({}) ", app.agents.len());

    let items: Vec<ListItem> = app
        .agents
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let prefix = if i == app.selected_index && matches!(app.selected_tab, Tab::Agents) {
                "> "
            } else {
                "  "
            };
            let state_str = format!("{:?}", a.state);
            let state_color = match a.state {
                clawhive_domain::AgentState::Active => Color::Green,
                clawhive_domain::AgentState::Hibernating => Color::Yellow,
                clawhive_domain::AgentState::Paused => Color::Blue,
                clawhive_domain::AgentState::Terminated => Color::Red,
                _ => Color::DarkGray,
            };
            let content = Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Cyan)),
                Span::raw(format!("{} ", a.name)),
                Span::styled(state_str, Style::default().fg(state_color)),
            ]);
            if i == app.selected_index && matches!(app.selected_tab, Tab::Agents) {
                ListItem::new(content).style(Style::default().bg(Color::DarkGray))
            } else {
                ListItem::new(content)
            }
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_alignment(ratatui::layout::Alignment::Center),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    frame.render_widget(list, area);
}

fn agent_details(area: Rect, frame: &mut Frame, app: &TuiApp) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Agent Details ")
        .title_alignment(ratatui::layout::Alignment::Center);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = if matches!(app.selected_tab, Tab::Agents) && !app.agents.is_empty() {
        let idx = app.selected_index.min(app.agents.len() - 1);
        let agent = &app.agents[idx];
        format!(
            "ID:       {}\n\
             Name:     {}\n\
             Role:     {}\n\
             State:    {:?}\n\
             Lifespan: {:?}\n\
             Parent:   {}\n\
             Turns:    {}\n\
             Cost:     ${:.4}\n\
             Created:  {}",
            agent.id.0,
            agent.name,
            agent.role,
            agent.state,
            agent.lifecycle_mode,
            agent.parent_agent_id.as_ref().map_or("none".into(), |id| id.0.to_string()),
            agent.turn_count,
            agent.total_cost_usd,
            agent.created_at.format("%Y-%m-%d %H:%M:%S"),
        )
    } else {
        "Select an agent to view details\n\n:help for commands".into()
    };

    let paragraph = Paragraph::new(text)
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

fn worker_list(area: Rect, frame: &mut Frame, app: &TuiApp) {
    let title = format!(" Workers ({}) ", app.workers.len());

    let items: Vec<ListItem> = app
        .workers
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let prefix = if i == app.selected_index && matches!(app.selected_tab, Tab::Workers) {
                "> "
            } else {
                "  "
            };
            let state_color = match w.state {
                clawhive_domain::WorkerState::Online => Color::Green,
                clawhive_domain::WorkerState::Offline => Color::DarkGray,
                clawhive_domain::WorkerState::Draining => Color::Yellow,
                clawhive_domain::WorkerState::Quarantined => Color::Red,
            };
            let content = Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Cyan)),
                Span::raw(format!("{} ", w.name)),
                Span::styled(format!("{:?}", w.state), Style::default().fg(state_color)),
            ]);
            if i == app.selected_index && matches!(app.selected_tab, Tab::Workers) {
                ListItem::new(content).style(Style::default().bg(Color::DarkGray))
            } else {
                ListItem::new(content)
            }
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_alignment(ratatui::layout::Alignment::Center),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    frame.render_widget(list, area);
}

fn worker_details(area: Rect, frame: &mut Frame, app: &TuiApp) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Worker Details ")
        .title_alignment(ratatui::layout::Alignment::Center);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = if !app.workers.is_empty() {
        let idx = app.selected_index.min(app.workers.len() - 1);
        let w = &app.workers[idx];
        format!(
            "ID:       {}\n\
             Name:     {}\n\
             Type:     {:?}\n\
             State:    {:?}\n\
             Draining: {}\n\
             Version:  {}",
            w.id.0, w.name, w.worker_type, w.state, w.is_draining, w.version,
        )
    } else {
        "Select a worker to view details".into()
    };

    let paragraph = Paragraph::new(text)
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

fn spawn_requests_list(area: Rect, frame: &mut Frame, app: &TuiApp) {
    let title = format!(" Spawn Requests ({}) ", app.spawn_requests.len());

    let items: Vec<ListItem> = app
        .spawn_requests
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let prefix = if i == app.selected_index && matches!(app.selected_tab, Tab::SpawnRequests)
            {
                "> "
            } else {
                "  "
            };
            let state_str = format!("{:?}", r.state);
            let state_color = match r.state {
                clawhive_domain::SpawnState::Pending => Color::Yellow,
                clawhive_domain::SpawnState::Approved => Color::Green,
                clawhive_domain::SpawnState::Denied => Color::Red,
                _ => Color::DarkGray,
            };
            let content = Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Cyan)),
                Span::raw(format!("{} ", r.id.0)),
                Span::styled(state_str, Style::default().fg(state_color)),
            ]);
            if i == app.selected_index && matches!(app.selected_tab, Tab::SpawnRequests) {
                ListItem::new(content).style(Style::default().bg(Color::DarkGray))
            } else {
                ListItem::new(content)
            }
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_alignment(ratatui::layout::Alignment::Center),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    frame.render_widget(list, area);
}

fn spawn_requests_details(area: Rect, frame: &mut Frame, app: &TuiApp) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Spawn Request Details ")
        .title_alignment(ratatui::layout::Alignment::Center);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = if !app.spawn_requests.is_empty() {
        let idx = app.selected_index.min(app.spawn_requests.len() - 1);
        let r = &app.spawn_requests[idx];
        let children_str: String = r
            .children
            .iter()
            .map(|c| format!("  - {}: {} (${})", c.role, c.objective, c.budget_usd))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "ID:       {}\n\
             Mission:  {}\n\
             State:    {:?}\n\
             Team:     {}\n\
             Reason:   {}\n\
             Children:\n{}\n\
             Created:  {}",
            r.id.0,
            r.mission_id.0,
            r.state,
            r.team.name,
            r.reason,
            children_str,
            r.created_at.format("%Y-%m-%d %H:%M:%S"),
        )
    } else {
        "Select a spawn request to view details\n\n:approve <id> / :deny <id>".into()
    };

    let paragraph = Paragraph::new(text)
        .style(Style::default().fg(Color::White))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

fn input_bar(area: Rect, frame: &mut Frame, app: &TuiApp) {
    let block = Block::default()
        .borders(Borders::TOP)
        .style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    match &app.input_mode {
        InputMode::Command => {
            let prompt = format!(":{}", app.input_buffer);
            let text = Paragraph::new(Line::from(Span::styled(
                prompt,
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )));
            frame.render_widget(text, inner);
            frame.set_cursor_position((inner.x + 1 + app.input_buffer.len() as u16, inner.y));
        }
        InputMode::Normal => {
            let tab_name = match app.selected_tab {
                Tab::Agents => "Agents",
                Tab::Workers => "Workers",
                Tab::SpawnRequests => "Spawn",
            };
            let text = format!(
                " {} | Agents: {} | Workers: {} | Spawn: {} | Tab: {} ",
                app.status_message,
                app.agents.len(),
                app.workers.len(),
                app.spawn_requests.len(),
                tab_name,
            );
            let paragraph = Paragraph::new(Line::from(Span::raw(text)))
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(paragraph, inner);
        }
    }
}

pub fn draw(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    header(chunks[0], frame, app);

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[1]);

    match app.selected_tab {
        Tab::Agents => {
            agent_list(main_chunks[0], frame, app);
            agent_details(main_chunks[1], frame, app);
        }
        Tab::Workers => {
            worker_list(main_chunks[0], frame, app);
            worker_details(main_chunks[1], frame, app);
        }
        Tab::SpawnRequests => {
            spawn_requests_list(main_chunks[0], frame, app);
            spawn_requests_details(main_chunks[1], frame, app);
        }
    }

    input_bar(chunks[2], frame, app);
}
