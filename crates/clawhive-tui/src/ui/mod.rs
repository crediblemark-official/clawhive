use ratatui::{layout::Rect, Frame};

pub mod chat;
pub mod components;
pub mod home;

use crate::app::{CommandMode, Screen, TuiApp};
use crate::ui::chat::draw_chat;
use crate::ui::components::{draw_apikey_input, draw_command_palette, draw_model_selection};
use crate::ui::home::draw_home;

pub fn draw(frame: &mut Frame, area: Rect, app: &TuiApp) {
    match app.active_screen {
        Screen::Home => draw_home(frame, area, app),
        Screen::Chat => draw_chat(frame, area, app),
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
}
