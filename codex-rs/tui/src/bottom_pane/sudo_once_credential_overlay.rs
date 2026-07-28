use crate::app::app_server_requests::ResolvedAppServerRequest;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::chat_composer::ChatComposer;
use crate::bottom_pane::chat_composer::ChatComposerConfig;
use crate::bottom_pane::chat_composer::InputResult;
use crate::render::renderable::Renderable;
use codex_app_server_protocol::SudoOnceCredential;
use codex_protocol::ThreadId;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

pub(crate) struct SudoOnceCredentialRequest {
    pub thread_id: ThreadId,
    pub item_id: String,
    pub attempt: u32,
}

pub(crate) struct SudoOnceCredentialOverlay {
    request: SudoOnceCredentialRequest,
    app_event_tx: AppEventSender,
    composer: ChatComposer,
    done: bool,
}

impl SudoOnceCredentialOverlay {
    pub(crate) fn new(
        request: SudoOnceCredentialRequest,
        app_event_tx: AppEventSender,
        has_input_focus: bool,
        enhanced_keys_supported: bool,
        disable_paste_burst: bool,
    ) -> Self {
        let mut composer = ChatComposer::new_with_config(
            has_input_focus,
            app_event_tx.clone(),
            enhanced_keys_supported,
            "Sudo password".to_string(),
            disable_paste_burst,
            ChatComposerConfig::plain_text(),
        );
        composer.set_footer_hint_override(Some(Vec::new()));
        Self {
            request,
            app_event_tx,
            composer,
            done: false,
        }
    }
    fn clear(&mut self) {
        self.composer
            .set_text_content(String::new(), Vec::new(), Vec::new());
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
        if matches!(key_event.code, KeyCode::Esc) {
            self.submit(None);
            return;
        }
        let (result, _) = self.composer.handle_key_event(key_event);
        if let InputResult::Submitted { text, .. } | InputResult::Queued { text, .. } = result {
            self.submit(Some(SudoOnceCredential::new(text)));
        }
    }

    fn on_ctrl_c(&mut self) -> crate::bottom_pane::CancellationEvent {
        self.submit(None);
        crate::bottom_pane::CancellationEvent::Handled
    }

    fn handle_paste(&mut self, pasted: String) -> bool {
        self.composer.handle_paste(pasted)
    }

    fn flush_paste_burst_if_due(&mut self) -> bool {
        self.composer.flush_paste_burst_if_due()
    }

    fn is_in_paste_burst(&self) -> bool {
        self.composer.is_in_paste_burst()
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
    fn desired_height(&self, width: u16) -> u16 {
        self.composer.desired_height(width).saturating_add(4)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        let header_height = 3.min(area.height);
        let header = Paragraph::new(vec![
            Line::from("Sudo authentication required".bold()),
            Line::from(format!("Password attempt {}. Press Esc to cancel.", self.request.attempt + 1).dim()),
            Line::from(""),
        ]);
        header.render(
            Rect::new(area.x, area.y, area.width, header_height),
            buf,
        );
        let input_area = Rect::new(
            area.x,
            area.y.saturating_add(header_height),
            area.width,
            area.height.saturating_sub(header_height),
        );
        self.composer.render_with_mask(input_area, buf, Some('*'));
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let header_height = 3.min(area.height);
        self.composer.cursor_pos(Rect::new(
            area.x,
            area.y.saturating_add(header_height),
            area.width,
            area.height.saturating_sub(header_height),
        ))
    }
}

#[cfg(test)]
#[path = "sudo_once_credential_overlay_tests.rs"]
mod tests;
