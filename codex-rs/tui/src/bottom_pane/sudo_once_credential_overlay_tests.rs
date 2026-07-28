use super::*;
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
fn credential_is_masked() {
    let (tx_raw, _rx) = unbounded_channel();
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
    insta::assert_snapshot!(rendered);
}
