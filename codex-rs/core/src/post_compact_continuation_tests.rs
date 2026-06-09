use codex_protocol::models::ContentItem;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ResponseItem;

use super::build_mid_turn_continuation_supplement;

fn assistant_text(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: text.to_string(),
        }],
        phase: Some(MessagePhase::Commentary),
    }
}

#[test]
fn builds_supplement_from_last_three_assistant_text_items() {
    let supplement = build_mid_turn_continuation_supplement(&[
        assistant_text("first progress"),
        assistant_text("second progress"),
        assistant_text("third progress"),
        assistant_text("fourth progress"),
    ])
    .expect("assistant progress should produce supplement");

    assert!(!supplement.contains("first progress"));
    assert!(supplement.contains("second progress"));
    assert!(supplement.contains("third progress"));
    assert!(supplement.contains("fourth progress"));
    assert!(
        supplement.contains("如果任务尚未完成，应从这里继续，不要因为压缩而中断或丢弃后续工作。")
    );
}

#[test]
fn ignores_non_textual_execution_artifacts() {
    let supplement = build_mid_turn_continuation_supplement(&[
        assistant_text("safe progress"),
        ResponseItem::FunctionCall {
            id: None,
            name: "shell".to_string(),
            namespace: None,
            arguments: "VERY_LARGE_COMMAND_SHOULD_NOT_APPEAR".to_string(),
            call_id: "call-1".to_string(),
        },
        ResponseItem::FunctionCallOutput {
            call_id: "call-1".to_string(),
            output: FunctionCallOutputPayload::from_text(
                "VERY_LARGE_TOOL_OUTPUT_SHOULD_NOT_APPEAR".to_string(),
            ),
        },
        ResponseItem::Compaction {
            encrypted_content: "ENCRYPTED_COMPACTION_SHOULD_NOT_APPEAR".to_string(),
        },
    ])
    .expect("assistant progress should produce supplement");

    assert!(supplement.contains("safe progress"));
    assert!(!supplement.contains("VERY_LARGE_COMMAND_SHOULD_NOT_APPEAR"));
    assert!(!supplement.contains("VERY_LARGE_TOOL_OUTPUT_SHOULD_NOT_APPEAR"));
    assert!(!supplement.contains("ENCRYPTED_COMPACTION_SHOULD_NOT_APPEAR"));
}

#[test]
fn ignores_final_answer_messages() {
    let supplement = build_mid_turn_continuation_supplement(&[ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: "final answer should not be treated as mid-turn progress".to_string(),
        }],
        phase: Some(MessagePhase::FinalAnswer),
    }]);

    assert_eq!(supplement, None);
}
