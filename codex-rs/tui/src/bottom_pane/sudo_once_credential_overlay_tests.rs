use std::sync::Arc;

use super::*;
use codex_protocol::ThreadId;
use codex_sudo_once::LocalSudoOnceBroker;
use codex_sudo_once::SudoOnceCommand;
use codex_sudo_once::SudoOncePrompt;
use codex_utils_absolute_path::AbsolutePathBuf;
use crossterm::event::KeyModifiers;

const TEST_COMMAND: [&str; 2] = ["id", "-u"];
const TEST_CREDENTIAL: &str = "password";

fn command() -> Arc<SudoOnceCommand> {
    Arc::new(SudoOnceCommand::new(
        ThreadId::new(),
        Arc::from(TEST_COMMAND.map(str::to_string)),
        AbsolutePathBuf::try_from("/workspace").expect("absolute cwd"),
        None,
    ))
}

async fn approved_broker() -> (
    LocalSudoOnceBroker,
    codex_sudo_once::SudoOncePromptReceiver,
    codex_sudo_once::SudoOnceGrant,
) {
    let (broker, mut prompts) = LocalSudoOnceBroker::new();
    let command = command();
    let approval = broker.request_approval(Arc::clone(&command));
    tokio::pin!(approval);
    let prompt = tokio::select! {
        prompt = prompts.recv() => prompt,
        _ = &mut approval => panic!("approval resolved before the prompt"),
    };
    let Some(SudoOncePrompt::Approval(prompt)) = prompt else {
        panic!("expected approval prompt");
    };
    let (_, responder) = prompt.into_parts();
    assert!(responder.approve());
    let grant = approval.await.expect("grant");
    (broker, prompts, grant)
}

fn render_overlay(overlay: &SudoOnceCredentialOverlay) -> String {
    let area = Rect::new(0, 0, 80, overlay.desired_height(/*width*/ 80));
    let mut buffer = Buffer::empty(area);
    overlay.render(area, &mut buffer);
    format!("{buffer:?}")
}

#[tokio::test]
async fn credential_is_masked() {
    let (broker, mut prompts, grant) = approved_broker().await;
    let request = broker.request_credential(&grant, 1);
    tokio::pin!(request);
    let prompt = tokio::select! {
        prompt = prompts.recv() => prompt,
        _ = &mut request => panic!("credential resolved before the prompt"),
    };
    let Some(SudoOncePrompt::Credential(prompt)) = prompt else {
        panic!("expected credential prompt");
    };
    let (command, attempt, responder) = prompt.into_parts();
    let mut overlay = SudoOnceCredentialOverlay::new(command, attempt, responder);
    for character in TEST_CREDENTIAL.chars() {
        overlay.handle_key_event(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }

    let rendered = render_overlay(&overlay);
    assert!(!rendered.contains(TEST_CREDENTIAL));
    assert!(rendered.contains("********"));
    insta::assert_snapshot!(rendered);
}

#[tokio::test]
async fn enter_submits_the_single_use_credential() {
    let (broker, mut prompts, grant) = approved_broker().await;
    let request = broker.request_credential(&grant, 1);
    tokio::pin!(request);
    let prompt = tokio::select! {
        prompt = prompts.recv() => prompt,
        _ = &mut request => panic!("credential resolved before the prompt"),
    };
    let Some(SudoOncePrompt::Credential(prompt)) = prompt else {
        panic!("expected credential prompt");
    };
    let (command, attempt, responder) = prompt.into_parts();
    let mut overlay = SudoOnceCredentialOverlay::new(command, attempt, responder);
    for character in TEST_CREDENTIAL.chars() {
        overlay.handle_key_event(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    overlay.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        request.await.expect("submitted credential").expose_secret(),
        TEST_CREDENTIAL
    );
    assert!(overlay.is_complete());
}
