use std::sync::Arc;

use super::*;
use codex_protocol::ThreadId;
use codex_sudo_once::LocalSudoOnceBroker;
use codex_sudo_once::SudoOnceCommand;
use codex_sudo_once::SudoOnceGrant;
use codex_sudo_once::SudoOncePrompt;
use codex_utils_absolute_path::AbsolutePathBuf;
use crossterm::event::KeyModifiers;

const TEST_COMMAND: [&str; 3] = ["apt", "install", "example package"];
const TEST_REASON: &str = "Install the requested package.";

fn command() -> Arc<SudoOnceCommand> {
    Arc::new(SudoOnceCommand::new(
        ThreadId::new(),
        Arc::from(TEST_COMMAND.map(str::to_string)),
        AbsolutePathBuf::try_from("/workspace").expect("absolute cwd"),
        Some(TEST_REASON.to_string()),
    ))
}

async fn approval_prompt() -> SudoOnceApprovalOverlay {
    let (broker, mut prompts) = LocalSudoOnceBroker::new();
    let request = broker.request_approval(command());
    tokio::pin!(request);
    let prompt = tokio::select! {
        prompt = prompts.recv() => prompt,
        _ = &mut request => panic!("approval resolved before the prompt"),
    };
    let Some(SudoOncePrompt::Approval(prompt)) = prompt else {
        panic!("expected approval prompt");
    };
    let (command, responder) = prompt.into_parts();
    SudoOnceApprovalOverlay::new(command, responder)
}

async fn pending_approval() -> (
    SudoOnceApprovalOverlay,
    tokio::task::JoinHandle<Option<SudoOnceGrant>>,
) {
    let (broker, mut prompts) = LocalSudoOnceBroker::new();
    let request = tokio::spawn(async move { broker.request_approval(command()).await });
    let Some(SudoOncePrompt::Approval(prompt)) = prompts.recv().await else {
        panic!("expected approval prompt");
    };
    let (command, responder) = prompt.into_parts();
    (SudoOnceApprovalOverlay::new(command, responder), request)
}

fn render_overlay(overlay: &SudoOnceApprovalOverlay) -> String {
    let area = Rect::new(0, 0, 80, overlay.desired_height(/*width*/ 80));
    let mut buffer = Buffer::empty(area);
    overlay.render(area, &mut buffer);
    format!("{buffer:?}")
}

#[tokio::test]
async fn approval_snapshot_shows_the_frozen_command_and_warning() {
    let overlay = approval_prompt().await;
    insta::assert_snapshot!(render_overlay(&overlay));
}

#[tokio::test]
async fn escape_aborts_the_single_use_approval() {
    let (broker, mut prompts) = LocalSudoOnceBroker::new();
    let request = broker.request_approval(command());
    tokio::pin!(request);
    let prompt = tokio::select! {
        prompt = prompts.recv() => prompt,
        _ = &mut request => panic!("approval resolved before the prompt"),
    };
    let Some(SudoOncePrompt::Approval(prompt)) = prompt else {
        panic!("expected approval prompt");
    };
    let (command, responder) = prompt.into_parts();
    let mut overlay = SudoOnceApprovalOverlay::new(command, responder);
    overlay.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(request.await.is_none());
    assert!(overlay.is_complete());
}

#[tokio::test]
async fn only_unmodified_y_keys_authorize() {
    for code in ['y', 'Y'] {
        let (mut overlay, request) = pending_approval().await;
        overlay.handle_key_event(KeyEvent::new(KeyCode::Char(code), KeyModifiers::NONE));
        assert!(overlay.is_complete());
        assert!(request.await.expect("approval task completed").is_some());
    }
}

#[tokio::test]
async fn modified_y_keys_do_not_authorize() {
    let modifiers = [
        KeyModifiers::SHIFT,
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::SUPER,
        KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
    ];
    for modifiers in modifiers {
        let (mut overlay, request) = pending_approval().await;
        overlay.handle_key_event(KeyEvent::new(KeyCode::Char('y'), modifiers));
        assert!(!overlay.is_complete());
        drop(overlay);
        assert!(request.await.expect("approval task completed").is_none());
    }
}
