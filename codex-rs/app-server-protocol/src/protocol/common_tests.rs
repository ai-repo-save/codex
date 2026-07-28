use super::*;
use anyhow::Result;
use codex_protocol::protocol::TurnAbortReason;
use pretty_assertions::assert_eq;
use serde_json::json;

const TEST_CREDENTIAL: &str = "credential-value";

#[test]
fn client_response_payload_returns_jsonrpc_parts_and_client_response() -> Result<()> {
    let (request_id, result, payload) =
        ClientResponsePayload::ThreadArchive(v2::ThreadArchiveResponse {})
            .into_jsonrpc_parts_and_payload(RequestId::Integer(7))?;

    assert_eq!(request_id, RequestId::Integer(7));
    assert_eq!(result, json!({}));

    let Some(ClientResponse::ThreadArchive {
        request_id,
        response: _,
    }) = payload.and_then(|payload| payload.into_client_response(RequestId::Integer(7)))
    else {
        panic!("expected thread/archive client response");
    };
    assert_eq!(request_id, RequestId::Integer(7));
    Ok(())
}

#[test]
fn interrupt_conversation_payload_stays_jsonrpc_only() -> Result<()> {
    let (request_id, result, payload) =
        ClientResponsePayload::InterruptConversation(v1::InterruptConversationResponse {
            abort_reason: TurnAbortReason::Interrupted,
        })
        .into_jsonrpc_parts_and_payload(RequestId::Integer(8))?;

    assert_eq!(request_id, RequestId::Integer(8));
    assert_eq!(
        result,
        json!({
            "abortReason": "interrupted",
        })
    );
    assert!(payload.is_none());
    Ok(())
}

#[test]
fn sudo_once_requests_use_dedicated_experimental_methods() -> Result<()> {
    let approval_json = json!({
        "method": "item/sudoOnce/requestApproval",
        "id": 11,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "item-1",
            "command": "id -u",
            "cwd": "/workspace",
            "reason": null
        }
    });
    let credential_json = json!({
        "method": "item/sudoOnce/requestCredential",
        "id": 12,
        "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "itemId": "item-1",
            "attempt": 2
        }
    });
    let approval: ServerRequest = serde_json::from_value(approval_json.clone())?;
    let credential: ServerRequest = serde_json::from_value(credential_json.clone())?;

    assert_eq!(serde_json::to_value(approval)?, approval_json);
    assert_eq!(serde_json::to_value(credential)?, credential_json);
    Ok(())
}

#[test]
fn sudo_once_credential_response_is_nullable_and_redacted() -> Result<()> {
    let response = ServerResponse::SudoOnceRequestCredential {
        request_id: RequestId::Integer(12),
        response: v2::SudoOnceRequestCredentialResponse {
            credential: Some(v2::SudoOnceCredential::new(TEST_CREDENTIAL.to_string())),
        },
    };
    let cancelled = ServerResponse::SudoOnceRequestCredential {
        request_id: RequestId::Integer(13),
        response: v2::SudoOnceRequestCredentialResponse { credential: None },
    };

    assert_eq!(
        serde_json::to_value(&response)?,
        json!({
            "method": "item/sudoOnce/requestCredential",
            "id": 12,
            "response": { "credential": TEST_CREDENTIAL }
        })
    );
    assert_eq!(
        serde_json::to_value(&cancelled)?,
        json!({
            "method": "item/sudoOnce/requestCredential",
            "id": 13,
            "response": { "credential": null }
        })
    );
    assert!(!format!("{response:?}").contains(TEST_CREDENTIAL));
    Ok(())
}
