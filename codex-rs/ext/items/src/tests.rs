use pretty_assertions::assert_eq;
use serde_json::json;

use super::ExtensionItem;
use super::image_generation::ImageGenerationItem;
use super::memory_mutation::MemoryMutation;
use super::memory_mutation::MemoryMutationScope;
use super::memory_mutation::MemoryMutationStatus;
use super::memory_mutation::MEMORY_MUTATION_PATH_MAX_GRAPHEMES;
use super::memory_mutation::MEMORY_MUTATION_PREVIEW_MAX_GRAPHEMES;
use super::memory_mutation::MEMORY_MUTATION_TITLE_MAX_GRAPHEMES;
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
