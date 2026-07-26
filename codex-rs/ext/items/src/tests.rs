use pretty_assertions::assert_eq;
use serde_json::json;

use super::ExtensionItem;
use super::agent_mailbox_action::AGENT_MAILBOX_ACTION_PREVIEW_MAX_GRAPHEMES;
use super::agent_mailbox_action::AgentMailboxAction;
use super::agent_mailbox_action::AgentMailboxMessageCategory;
use super::agent_mailbox_action::AgentMailboxActionStatus;
use super::agent_mailbox_action::AgentMailboxMessagePreview;
use super::image_generation::ImageGenerationItem;
use super::memory_mutation::MEMORY_MUTATION_PATH_MAX_GRAPHEMES;
use super::memory_mutation::MEMORY_MUTATION_PREVIEW_MAX_GRAPHEMES;
use super::memory_mutation::MEMORY_MUTATION_TITLE_MAX_GRAPHEMES;
use super::memory_mutation::MemoryMutation;
use super::memory_mutation::MemoryMutationScope;
use super::memory_mutation::MemoryMutationStatus;
use super::sleep::SleepItem;
use super::web_search::WebSearchAction;
use super::web_search::WebSearchItem;

fn completed_image_generation_item() -> ExtensionItem {
    ExtensionItem::ImageGeneration(ImageGenerationItem {
        id: "image-1".to_string(),
        status: "completed".to_string(),
        revised_prompt: Some("A blue square".to_string()),
        result: "cG5n".to_string(),
        saved_path: None,
    })
}

#[test]
fn agent_mailbox_action_preserves_stable_wire_shape() {
    let item = ExtensionItem::AgentMailboxAction(
        AgentMailboxAction::read(
            "mailbox-1".to_string(),
            Some("/root/worker".to_string()),
            Some(AgentMailboxMessageCategory::Result),
            2,
        )
        .with_messages(vec![AgentMailboxMessagePreview::plaintext(
            "/root/worker".to_string(),
            AgentMailboxMessageCategory::Result,
            "  completed\twork\nignored",
        )])
        .with_status(AgentMailboxActionStatus::Succeeded),
    );
    let value = serde_json::to_value(&item).expect("serialize extension item");

    assert_eq!(
        value,
        json!({
            "kind": "agent_mailbox.action",
            "id": "mailbox-1",
            "status": "succeeded",
            "action": {
                "type": "read",
                "sender": "/root/worker",
                "category": "result",
                "limit": 2,
                "messages": [{
                    "sender": "/root/worker",
                    "category": "result",
                    "content": {
                        "type": "plaintext",
                        "preview": "completed work",
                    },
                }],
            },
        })
    );
    assert_eq!(
        serde_json::from_value::<ExtensionItem>(value).expect("deserialize extension item"),
        item
    );
}

#[test]
fn restored_agent_mailbox_action_bounds_paths_and_previews() {
    let path = "p".repeat(super::agent_mailbox_action::AGENT_MAILBOX_AGENT_PATH_MAX_GRAPHEMES + 1);
    let preview = format!("\n  {}\nignored", "v".repeat(AGENT_MAILBOX_ACTION_PREVIEW_MAX_GRAPHEMES + 1));
    let item = serde_json::from_value::<ExtensionItem>(json!({
        "kind": "agent_mailbox.action",
        "id": "mailbox-1",
        "status": "succeeded",
        "action": {
            "type": "send",
            "target": path,
            "recipient": path,
            "category": "result",
            "preview": preview,
        },
    }))
    .expect("deserialize agent mailbox action");
    let value = serde_json::to_value(item).expect("serialize restored mailbox action");

    assert_eq!(
        value["action"]["target"],
        json!("p".repeat(super::agent_mailbox_action::AGENT_MAILBOX_AGENT_PATH_MAX_GRAPHEMES))
    );
    assert_eq!(
        value["action"]["recipient"],
        json!("p".repeat(super::agent_mailbox_action::AGENT_MAILBOX_AGENT_PATH_MAX_GRAPHEMES))
    );
    assert_eq!(
        value["action"]["preview"],
        json!("v".repeat(AGENT_MAILBOX_ACTION_PREVIEW_MAX_GRAPHEMES))
    );
}

#[test]
fn image_generation_item_preserves_stable_wire_shape() {
    let item = completed_image_generation_item();
    let value = serde_json::to_value(&item).expect("serialize extension item");

    assert_eq!(
        value,
        json!({
            "kind": "image_gen.generation",
            "id": "image-1",
            "status": "completed",
            "revisedPrompt": "A blue square",
            "result": "cG5n",
        })
    );
    assert_eq!(
        serde_json::from_value::<ExtensionItem>(value).expect("deserialize extension item"),
        item
    );
}

#[test]
fn web_search_item_preserves_stable_wire_shape() {
    let item = ExtensionItem::WebSearch(WebSearchItem {
        id: "search-1".to_string(),
        query: "docs".to_string(),
        action: Some(WebSearchAction::Search {
            query: Some("docs".to_string()),
            queries: None,
        }),
        results: None,
    });
    let value = serde_json::to_value(&item).expect("serialize extension item");

    assert_eq!(
        value,
        json!({
            "kind": "web.search",
            "id": "search-1",
            "query": "docs",
            "action": {
                "type": "search",
                "query": "docs",
                "queries": null,
            },
            "results": null,
        })
    );
    assert_eq!(
        serde_json::from_value::<ExtensionItem>(value).expect("deserialize extension item"),
        item
    );
    assert_eq!(
        serde_json::from_value::<ExtensionItem>(json!({
            "kind": "web.search",
            "id": "search-1",
            "query": "docs",
            "action": {
                "type": "search",
                "query": "docs",
                "queries": null,
            },
        }))
        .expect("deserialize legacy extension item without results"),
        item
    );
}

#[test]
fn sleep_item_preserves_stable_wire_shape() {
    let item = ExtensionItem::Sleep(SleepItem {
        id: "sleep-1".to_string(),
        duration_ms: 1_000,
    });
    let value = serde_json::to_value(&item).expect("serialize extension item");

    assert_eq!(
        value,
        json!({
            "kind": "clock.sleep",
            "id": "sleep-1",
            "durationMs": 1_000,
        })
    );
    assert_eq!(
        serde_json::from_value::<ExtensionItem>(value).expect("deserialize extension item"),
        item
    );
}

#[test]
fn memory_mutation_item_preserves_stable_wire_shape() {
    let item = ExtensionItem::MemoryMutation(
        MemoryMutation::write(
            "memory-1".to_string(),
            MemoryMutationScope::Session,
            Some("Review Style".to_string()),
            "Keep review comments concise.",
        )
        .with_status(MemoryMutationStatus::Succeeded)
        .with_path("notes/review-style.md".to_string()),
    );
    let value = serde_json::to_value(&item).expect("serialize extension item");

    assert_eq!(
        value,
        json!({
            "kind": "memory.mutation",
            "id": "memory-1",
            "action": "write",
            "scope": "session",
            "status": "succeeded",
            "title": "Review Style",
            "path": "notes/review-style.md",
            "preview": "Keep review comments concise.",
        })
    );
    assert_eq!(
        serde_json::from_value::<ExtensionItem>(value).expect("deserialize extension item"),
        item
    );
}

#[test]
fn restored_memory_mutation_enforces_string_bounds() {
    let title = "t".repeat(MEMORY_MUTATION_TITLE_MAX_GRAPHEMES + 1);
    let path = "p".repeat(MEMORY_MUTATION_PATH_MAX_GRAPHEMES + 1);
    let preview = format!(
        "\n  {}\nignored",
        "v".repeat(MEMORY_MUTATION_PREVIEW_MAX_GRAPHEMES + 1)
    );
    let item = serde_json::from_value::<ExtensionItem>(json!({
        "kind": "memory.mutation",
        "id": "memory-1",
        "action": "write",
        "scope": "project",
        "status": "succeeded",
        "title": title,
        "path": path,
        "preview": preview,
    }))
    .expect("deserialize memory mutation");
    let value = serde_json::to_value(item).expect("serialize restored memory mutation");

    assert_eq!(
        value["title"],
        json!("t".repeat(MEMORY_MUTATION_TITLE_MAX_GRAPHEMES))
    );
    assert_eq!(
        value["path"],
        json!("p".repeat(MEMORY_MUTATION_PATH_MAX_GRAPHEMES))
    );
    assert_eq!(
        value["preview"],
        json!("v".repeat(MEMORY_MUTATION_PREVIEW_MAX_GRAPHEMES))
    );
}

#[test]
fn unknown_extension_kind_is_rejected() {
    let value = json!({
        "kind": "image_gen.unknown",
        "id": "image-1",
    });

    assert!(serde_json::from_value::<ExtensionItem>(value).is_err());
}

#[test]
fn malformed_known_extension_payload_is_rejected() {
    let value = json!({
        "kind": "image_gen.generation",
        "id": "image-1",
        "status": "completed",
    });

    assert!(serde_json::from_value::<ExtensionItem>(value).is_err());
}
