pub(crate) fn context_anchor_saved_summary(anchor_id: &str, label: Option<&str>) -> String {
    let mut summary = format!("Context anchor saved: {anchor_id}");
    if let Some(label) = label
        && !label.trim().is_empty()
    {
        summary.push_str(" · ");
        summary.push_str(label.trim());
    }
    summary
}

pub(crate) fn context_anchor_rewound_summary(
    anchor_id: &str,
    dropped_turns: u32,
    response_items_reclaimed: u64,
    approx_tokens_reclaimed: u64,
) -> String {
    let noun = if dropped_turns == 1 { "turn" } else { "turns" };
    let item_noun = if response_items_reclaimed == 1 {
        "item"
    } else {
        "items"
    };
    format!(
        "Context rewound to anchor: {anchor_id} · reclaimed ~{approx_tokens_reclaimed} tokens ({response_items_reclaimed} {item_noun}) · dropped {dropped_turns} user {noun}"
    )
}
