//! Conversation history viewer widget for scrolling through past messages

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Widget, WidgetRef, Wrap, StatefulWidget},
};
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use codex_core::protocol::Op;

pub struct ConversationHistoryViewer {
    app_event_tx: AppEventSender,
    history_entries: Vec<String>,
    scroll_offset: usize,
    visible_height: u16,
    is_complete: bool,
    history_log_id: Option<String>,
    history_entry_count: usize,
    loading_entries: Vec<usize>, // Indices of entries we're waiting to load
}

impl ConversationHistoryViewer {
    pub fn new(
        app_event_tx: AppEventSender,
        history_log_id: Option<String>,
        history_entry_count: usize,
    ) -> Self {
        Self {
            app_event_tx,
            history_entries: Vec::new(),
            scroll_offset: 0,
            visible_height: 0,
            is_complete: false,
            history_log_id,
            history_entry_count,
            loading_entries: Vec::new(),
        }
    }

    pub fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.kind == KeyEventKind::Press {
            match key_event.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.is_complete = true;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.scroll_up();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.scroll_down();
                }
                KeyCode::PageUp => {
                    for _ in 0..10 {
                        self.scroll_up();
                    }
                }
                KeyCode::PageDown => {
                    for _ in 0..10 {
                        self.scroll_down();
                    }
                }
                KeyCode::Home => {
                    self.scroll_offset = 0;
                }
                KeyCode::End => {
                    if !self.history_entries.is_empty() {
                        self.scroll_offset = self.history_entries.len().saturating_sub(self.visible_height as usize);
                    }
                }
                _ => {}
            }
        }
    }

    fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
            self.ensure_entry_loaded(self.scroll_offset);
        }
    }

    fn scroll_down(&mut self) {
        let max_scroll = self.history_entries.len().saturating_sub(self.visible_height as usize);
        if self.scroll_offset < max_scroll {
            self.scroll_offset += 1;
            let bottom_visible = self.scroll_offset + self.visible_height as usize;
            if bottom_visible < self.history_entries.len() {
                self.ensure_entry_loaded(bottom_visible);
            }
        }
    }

    fn ensure_entry_loaded(&mut self, index: usize) {
        // For now, we'll just ensure we have enough placeholder entries
        while self.history_entries.len() <= index {
            if self.history_entries.len() < self.history_entry_count {
                // Add placeholder for entries we haven't loaded yet
                self.history_entries.push(format!("Loading message {}...", self.history_entries.len() + 1));
                
                // Request the actual entry from the backend
                if let Some(ref log_id_str) = self.history_log_id {
                    if let Ok(log_id) = log_id_str.parse::<u64>() {
                        if !self.loading_entries.contains(&self.history_entries.len().saturating_sub(1)) {
                            self.loading_entries.push(self.history_entries.len().saturating_sub(1));
                            let op = Op::GetHistoryEntryRequest {
                                log_id,
                                offset: self.history_entries.len().saturating_sub(1),
                            };
                            self.app_event_tx.send(AppEvent::CodexOp(op));
                        }
                    }
                }
            } else {
                break;
            }
        }
    }

    pub fn on_history_entry_response(&mut self, log_id: String, offset: usize, entry: Option<String>) {
        if Some(&log_id) == self.history_log_id.as_ref() {
            if let Some(entry_text) = entry {
                if offset < self.history_entries.len() {
                    self.history_entries[offset] = entry_text;
                }
            }
            self.loading_entries.retain(|&x| x != offset);
        }
    }

    pub fn is_complete(&self) -> bool {
        self.is_complete
    }


    fn render_content_with_height(&self, height: u16) -> (Vec<Line<'static>>, ScrollbarState) {
        let mut lines = Vec::new();
        
        if self.history_entries.is_empty() {
            lines.push(Line::from(Span::styled(
                "No conversation history available",
                Style::default().fg(Color::Gray),
            )));
        } else {
            let visible_start = self.scroll_offset;
            let visible_end = (self.scroll_offset + height as usize).min(self.history_entries.len());
            
            for (i, entry) in self.history_entries[visible_start..visible_end].iter().enumerate() {
                let entry_index = visible_start + i;
                let prefix = if entry.starts_with("Loading") {
                    Span::styled(format!("{}: ", entry_index + 1), Style::default().fg(Color::Yellow))
                } else {
                    Span::styled(format!("{}: ", entry_index + 1), Style::default().fg(Color::Blue))
                };
                
                // Split long entries into multiple lines
                let entry_text = if entry.len() > 100 {
                    format!("{}...", &entry[..97])
                } else {
                    entry.clone()
                };
                
                lines.push(Line::from(vec![
                    prefix,
                    Span::raw(entry_text),
                ]));
                
                // Add some spacing between entries
                if i < visible_end - visible_start - 1 {
                    lines.push(Line::from(""));
                }
            }
        }

        let scrollbar_state = ScrollbarState::new(self.history_entries.len())
            .position(self.scroll_offset);

        (lines, scrollbar_state)
    }
}

impl WidgetRef for &ConversationHistoryViewer {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title(" Conversation History (Esc to close, ↑↓ to scroll) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        block.render(area, buf);

        // Calculate content and scrollbar state
        let (lines, scrollbar_state) = self.render_content_with_height(inner.height);
        
        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((0, 0));
        
        paragraph.render(inner, buf);

        // Render scrollbar if we have content that extends beyond the visible area
        if self.history_entries.len() > inner.height as usize {
            let scrollbar = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"));
            
            let mut scrollbar_state = scrollbar_state;
            StatefulWidget::render(scrollbar, area, buf, &mut scrollbar_state);
        }
    }
}

// We need Clone for the mutable access workaround above
impl Clone for ConversationHistoryViewer {
    fn clone(&self) -> Self {
        Self {
            app_event_tx: self.app_event_tx.clone(),
            history_entries: self.history_entries.clone(),
            scroll_offset: self.scroll_offset,
            visible_height: self.visible_height,
            is_complete: self.is_complete,
            history_log_id: self.history_log_id.clone(),
            history_entry_count: self.history_entry_count,
            loading_entries: self.loading_entries.clone(),
        }
    }
}
