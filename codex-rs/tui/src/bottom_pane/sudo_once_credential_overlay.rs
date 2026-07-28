use std::sync::Arc;

use crate::bottom_pane::BottomPaneView;
use crate::render::renderable::Renderable;
use codex_sudo_once::SudoOnceCommand;
use codex_sudo_once::SudoOnceCredential;
use codex_sudo_once::SudoOnceCredentialResponder;
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

const MAX_CREDENTIAL_BYTES: usize = 4096;

struct SecretInput {
    bytes: Zeroizing<[u8; MAX_CREDENTIAL_BYTES]>,
    len: usize,
    char_count: usize,
}

impl SecretInput {
    fn new() -> Self {
        Self {
            bytes: Zeroizing::new([0; MAX_CREDENTIAL_BYTES]),
            len: 0,
            char_count: 0,
        }
    }

    fn push_char(&mut self, character: char) {
        let mut encoded = [0; 4];
        self.push_str(character.encode_utf8(&mut encoded));
        encoded.zeroize();
    }

    fn push_str(&mut self, value: &str) {
        let Some(end) = self.len.checked_add(value.len()) else {
            return;
        };
        if end > MAX_CREDENTIAL_BYTES {
            return;
        }
        self.bytes[self.len..end].copy_from_slice(value.as_bytes());
        self.len = end;
        self.char_count += value.chars().count();
    }

    fn pop(&mut self) {
        let value = std::str::from_utf8(&self.bytes[..self.len])
            .expect("credential input always contains valid UTF-8");
        let Some((start, _)) = value.char_indices().next_back() else {
            return;
        };
        self.bytes[start..self.len].zeroize();
        self.len = start;
        self.char_count -= 1;
    }

    fn take(&mut self) -> SudoOnceCredential {
        let mut credential = String::with_capacity(self.len);
        let value = std::str::from_utf8(&self.bytes[..self.len])
            .expect("credential input always contains valid UTF-8");
        credential.push_str(value);
        self.clear();
        SudoOnceCredential::new(credential)
    }

    fn clear(&mut self) {
        self.bytes.zeroize();
        self.len = 0;
        self.char_count = 0;
    }

    fn char_count(&self) -> usize {
        self.char_count
    }
}

impl std::fmt::Debug for SecretInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretInput([REDACTED])")
    }
}

pub(crate) struct SudoOnceCredentialOverlay {
    command: Arc<SudoOnceCommand>,
    attempt: u32,
    responder: Option<SudoOnceCredentialResponder>,
    input: SecretInput,
}

impl SudoOnceCredentialOverlay {
    pub(crate) fn new(
        command: Arc<SudoOnceCommand>,
        attempt: u32,
        responder: SudoOnceCredentialResponder,
    ) -> Self {
        Self {
            command,
            attempt,
            responder: Some(responder),
            input: SecretInput::new(),
        }
    }

    fn clear(&mut self) {
        self.input.clear();
    }

    fn take_credential(&mut self) -> SudoOnceCredential {
        self.input.take()
    }

    fn submit(&mut self, credential: Option<SudoOnceCredential>) {
        self.clear();
        let Some(responder) = self.responder.take() else {
            return;
        };
        match credential {
            Some(credential) => {
                responder.submit(credential);
            }
            None => {
                responder.cancel();
            }
        }
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
                if !key_event.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                self.input.push_char(character);
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
        self.responder
            .as_ref()
            .is_none_or(SudoOnceCredentialResponder::is_closed)
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
            Line::from(format!("Password attempt {}. Press Esc to cancel.", self.attempt).dim()),
            Line::from(""),
        ]);
        header.render(Rect::new(area.x, area.y, area.width, header_height), buf);
        let input_area = Rect::new(
            area.x,
            area.y.saturating_add(header_height),
            area.width,
            area.height.saturating_sub(header_height),
        );
        let masked = "*".repeat(self.input.char_count());
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
        let cursor_x = "Password: ".width().saturating_add(self.input.char_count()) as u16;
        Some((
            input_area
                .x
                .saturating_add(cursor_x.min(input_area.width.saturating_sub(1))),
            input_area.y,
        ))
    }
}

impl Drop for SudoOnceCredentialOverlay {
    fn drop(&mut self) {
        self.input.clear();
        if let Some(responder) = self.responder.take() {
            responder.cancel();
        }
    }
}

#[cfg(test)]
#[path = "sudo_once_credential_overlay_tests.rs"]
mod tests;
