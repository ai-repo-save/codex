use std::sync::Arc;

use crate::bottom_pane::BottomPaneView;
use crate::render::renderable::Renderable;
use codex_sudo_once::SudoOnceApprovalResponder;
use codex_sudo_once::SudoOnceCommand;
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
use textwrap::wrap;

const ROOT_WARNING: &str = "This command will run as root and outside the Codex sandbox.";
const UNRENDERABLE_COMMAND: &str = "[command cannot be safely rendered]";

pub(crate) struct SudoOnceApprovalOverlay {
    command: Arc<SudoOnceCommand>,
    responder: Option<SudoOnceApprovalResponder>,
}

impl SudoOnceApprovalOverlay {
    pub(crate) fn new(command: Arc<SudoOnceCommand>, responder: SudoOnceApprovalResponder) -> Self {
        Self {
            command,
            responder: Some(responder),
        }
    }

    fn command_line(&self) -> String {
        shlex::try_join(self.command.argv().iter().map(String::as_str))
            .unwrap_or_else(|_| UNRENDERABLE_COMMAND.to_string())
    }

    fn abort(&mut self) {
        if let Some(responder) = self.responder.take() {
            responder.abort();
        }
    }

    fn approve(&mut self) {
        if let Some(responder) = self.responder.take() {
            responder.approve();
        }
    }

    fn render_wrapped(text: &str, width: u16) -> Vec<Line<'static>> {
        wrap(text, width.max(1) as usize)
            .into_iter()
            .map(|line| Line::from(line.into_owned()))
            .collect()
    }
}

impl BottomPaneView for SudoOnceApprovalOverlay {
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
            self.abort();
            return;
        }
        if key_event.modifiers == KeyModifiers::NONE
            && matches!(key_event.code, KeyCode::Char('y' | 'Y'))
        {
            self.approve();
        }
    }

    fn on_ctrl_c(&mut self) -> crate::bottom_pane::CancellationEvent {
        self.abort();
        crate::bottom_pane::CancellationEvent::Handled
    }

    fn is_complete(&self) -> bool {
        self.responder
            .as_ref()
            .is_none_or(SudoOnceApprovalResponder::is_closed)
    }

    fn terminal_title_requires_action(&self) -> bool {
        true
    }
}

impl Renderable for SudoOnceApprovalOverlay {
    fn desired_height(&self, width: u16) -> u16 {
        self.lines(width.saturating_sub(2).max(1)).len() as u16
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        Paragraph::new(self.lines(area.width.saturating_sub(2).max(1))).render(area, buf);
    }
}

impl SudoOnceApprovalOverlay {
    fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![
            "Sudo authorization required".bold().into(),
            ROOT_WARNING.red().bold().into(),
            "".into(),
            "Command:".bold().into(),
        ];
        lines.extend(Self::render_wrapped(&self.command_line(), width));
        lines.push("Working directory:".bold().into());
        let cwd = self.command.cwd().as_path().display().to_string();
        lines.extend(Self::render_wrapped(&cwd, width));
        if let Some(reason) = self.command.reason() {
            lines.push("Reason:".bold().into());
            lines.extend(Self::render_wrapped(reason, width));
        }
        lines.push("".into());
        lines.push("y: authorize once · Esc/Ctrl-C: abort".dim().into());
        lines
    }
}

impl Drop for SudoOnceApprovalOverlay {
    fn drop(&mut self) {
        if let Some(responder) = self.responder.take() {
            responder.abort();
        }
    }
}

#[cfg(test)]
#[path = "sudo_once_approval_overlay_tests.rs"]
mod tests;
