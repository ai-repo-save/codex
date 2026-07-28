use super::*;
use crate::app_command::AppCommand;
use crate::app_event::AppEvent;
use crossterm::event::KeyModifiers;
use tokio::sync::mpsc::unbounded_channel;

const TEST_CREDENTIAL: &str = "password";
const TEST_ITEM_ID: &str = "sudo-item";

fn request() -> SudoOnceCredentialRequest {
    SudoOnceCredentialRequest {
        thread_id: ThreadId::new(),
        item_id: TEST_ITEM_ID.to_string(),
        attempt: 0,
    }
}

fn render_overlay(overlay: &SudoOnceCredentialOverlay) -> String {
    let area = Rect::new(0, 0, 80, overlay.desired_height(/*width*/ 80));
    let mut buffer = Buffer::empty(area);
    overlay.render(area, &mut buffer);
    format!("{buffer:?}")
}

#[test]
fn credential_is_masked_and_never_serialized_with_the_command() {
    let (tx_raw, mut rx) = unbounded_channel();
    let tx = AppEventSender::new(tx_raw);
    let mut overlay = SudoOnceCredentialOverlay::new(
        request(),
        tx,
        /*has_input_focus*/ true,
        /*enhanced_keys_supported*/ false,
        /*disable_paste_burst*/ true,
    );

    for character in TEST_CREDENTIAL.chars() {
        overlay.handle_key_event(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }

    let rendered = render_overlay(&overlay);
    assert!(!rendered.contains(TEST_CREDENTIAL));
    assert!(rendered.contains("********"));
    overlay.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let event = rx.try_recv().expect("credential response should be sent");
    let AppEvent::SubmitThreadOp { op, .. } = event else {
        panic!("expected a thread-scoped credential response");
    };
    assert!(matches!(op, AppCommand::SudoOnceCredential { .. }));
    assert!(!format!("{op:?}").contains(TEST_CREDENTIAL));
    assert!(
        !serde_json::to_string(&op)
            .expect("credential command should serialize safely")
            .contains(TEST_CREDENTIAL)
    );
}

#[test]
fn esc_submits_a_null_credential() {
    let (tx_raw, mut rx) = unbounded_channel();
    let tx = AppEventSender::new(tx_raw);
    let mut overlay = SudoOnceCredentialOverlay::new(
        request(),
        tx,
        /*has_input_focus*/ true,
        /*enhanced_keys_supported*/ false,
        /*disable_paste_burst*/ true,
    );
    overlay.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    let event = rx.try_recv().expect("cancel response should be sent");
    let AppEvent::SubmitThreadOp { op, .. } = event else {
        panic!("expected a thread-scoped credential response");
    };
    assert_eq!(
        op,
        AppCommand::sudo_once_credential(TEST_ITEM_ID.to_string(), None)
    );
}
