use crate::app::app_server_requests::ResolvedAppServerRequest;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::render::renderable::Renderable;
use codex_app_server_protocol::SudoOnceCredential;
use codex_protocol::ThreadId;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;
use zeroize::Zeroize;
use zeroize::Zeroizing;

pub(crate) struct SudoOnceCredentialRequest {
    pub thread_id: ThreadId,
    pub item_id: String,
    pub attempt: u32,
}

pub(crate) struct SudoOnceCredentialOverlay {
    request: SudoOnceCredentialRequest,
    app_event_tx: AppEventSender,
    input: Zeroizing<String>,
    done: bool,
}

impl SudoOnceCredentialOverlay {
    pub(crate) fn new(
        request: SudoOnceCredentialRequest,
        app_event_tx: AppEventSender,
        _has_input_focus: bool,
        _enhanced_keys_supported: bool,
        _disable_paste_burst: bool,
    ) -> Self {
        Self {
            request,
            app_event_tx,
            input: Zeroizing::new(String::new()),
            done: false,
        }
    }

    fn clear(&mut self) {
        self.input.zeroize();
    }

    fn take_credential(&mut self) -> SudoOnceCredential {
        SudoOnceCredential::new(std::mem::take(&mut *self.input))
    }

    fn submit(&mut self, credential: Option<SudoOnceCredential>) {
        if self.done {
            return;
        }
        self.clear();
        self.app_event_tx.sudo_once_credential(
            self.request.thread_id,
            self.request.item_id.clone(),
            credential,
        );
        self.done = true;
    }
}

impl BottomPaneView for SudoOnceCredentialOverlay {
    fn prefer_esc_to_handle_key_event(&self) -> bool {
        true
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.kind == KeyEventKind::Release {
            return;
        }
        if matches!(key_event.code, KeyCode::Esc)
            || matches!(
                key_event,
                KeyEvent {
                    code: KeyCode::Char('c' | 'C'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                }
            )
        {
            self.submit(None);
            return;
        }
        match key_event.code {
            KeyCode::Char(character)
                if !key_event
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) =>
            {
                self.input.push(character);
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Enter => {
                let credential = self.take_credential();
                self.submit(Some(credential));
            }
            _ => {}
        }
    }

    fn on_ctrl_c(&mut self) -> crate::bottom_pane::CancellationEvent {
        self.submit(None);
        crate::bottom_pane::CancellationEvent::Handled
    }

    fn handle_paste(&mut self, mut pasted: String) -> bool {
        self.input.push_str(&pasted);
        pasted.zeroize();
        true
    }

    fn is_complete(&self) -> bool {
        self.done
    }

    fn dismiss_app_server_request(&mut self, request: &ResolvedAppServerRequest) -> bool {
        if matches!(request, ResolvedAppServerRequest::SudoOnceCredential { id } if id == &self.request.item_id)
        {
            self.clear();
            self.done = true;
            return true;
        }
        false
    }
    fn terminal_title_requires_action(&self) -> bool {
        true
    }
}

impl Renderable for SudoOnceCredentialOverlay {
    fn desired_height(&self, _width: u16) -> u16 {
        4
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        let header_height = 3.min(area.height);
        let header = Paragraph::new(vec![
            Line::from("Sudo authentication required".bold()),
            Line::from(
                format!(
                    "Password attempt {}. Press Esc to cancel.",
                    self.request.attempt + 1
                )
                .dim(),
            ),
            Line::from(""),
        ]);
        header.render(Rect::new(area.x, area.y, area.width, header_height), buf);
        let input_area = Rect::new(
            area.x,
            area.y.saturating_add(header_height),
            area.width,
            area.height.saturating_sub(header_height),
        );
        let masked = "*".repeat(self.input.chars().count());
        Paragraph::new(Line::from(format!("Password: {masked}"))).render(input_area, buf);
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let header_height = 3.min(area.height);
        let input_area = Rect::new(
            area.x,
            area.y.saturating_add(header_height),
            area.width,
            area.height.saturating_sub(header_height),
        );
        let cursor_x = "Password: ".width().saturating_add(self.input.chars().count()) as u16;
        Some((
            input_area.x.saturating_add(cursor_x.min(input_area.width)),
            input_area.y,
        ))
    }
}

#[cfg(test)]
#[path = "sudo_once_credential_overlay_tests.rs"]
mod tests;
