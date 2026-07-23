use super::*;
use codex_protocol::openai_models::ReasoningEffort;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn response_requests_omit_absent_reasoning_and_preserve_explicit_effort() {
    let http_without_reasoning = request(/*reasoning*/ None);
    let websocket_without_reasoning = ResponseCreateWsRequest::from(&http_without_reasoning);
    assert_eq!(
        (
            serde_json::to_value(&http_without_reasoning)
                .expect("serialize HTTP request")
                .get("reasoning")
                .cloned(),
            serde_json::to_value(websocket_without_reasoning)
                .expect("serialize WebSocket request")
                .get("reasoning")
                .cloned(),
        ),
        (None, None)
    );

    let expected = Some(json!({"effort": "ultra"}));
    let http_with_reasoning = request(Some(Reasoning {
        effort: Some(ReasoningEffort::Ultra),
        summary: None,
        context: None,
    }));
    let websocket_with_reasoning = ResponseCreateWsRequest::from(&http_with_reasoning);
    assert_eq!(
        (
            serde_json::to_value(&http_with_reasoning)
                .expect("serialize HTTP request")
                .get("reasoning")
                .cloned(),
            serde_json::to_value(websocket_with_reasoning)
                .expect("serialize WebSocket request")
                .get("reasoning")
                .cloned(),
        ),
        (expected.clone(), expected)
    );
}

fn request(reasoning: Option<Reasoning>) -> ResponsesApiRequest {
    ResponsesApiRequest {
        model: "gpt-test".to_string(),
        instructions: String::new(),
        input: Vec::new(),
        tools: None,
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        reasoning,
        store: false,
        stream: true,
        stream_options: None,
        include: Vec::new(),
        max_output_tokens: None,
        service_tier: None,
        prompt_cache_key: None,
        text: None,
        client_metadata: None,
    }
}
