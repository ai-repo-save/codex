use super::*;

#[tokio::test]
async fn claimed_parent_reply_wins_over_cancellation() {
    let control = AgentControl::default();
    let child_thread_id = codex_protocol::ThreadId::new();
    let parent_thread_id = codex_protocol::ThreadId::new();
    let (request_id, receiver) = control.register_parent_request(child_thread_id, parent_thread_id);
    let claim = control
        .claim_parent_reply(&request_id, parent_thread_id, child_thread_id)
        .expect("parent should claim pending request");
    let cancellation_token = CancellationToken::new();
    cancellation_token.cancel();

    let wait = wait_for_parent_outcome(
        receiver,
        &cancellation_token,
        Duration::from_secs(60),
        &control,
        &request_id,
    );
    let deliver = claim.deliver("approved".to_string());
    let (outcome, delivered) = tokio::join!(wait, deliver);

    assert_eq!(outcome, ParentWaitResult::Answered("approved".to_string()));
    assert_eq!(delivered, Ok(()));
}

#[tokio::test]
async fn claimed_parent_reply_wins_over_timeout() {
    let control = AgentControl::default();
    let child_thread_id = codex_protocol::ThreadId::new();
    let parent_thread_id = codex_protocol::ThreadId::new();
    let (request_id, receiver) = control.register_parent_request(child_thread_id, parent_thread_id);
    let claim = control
        .claim_parent_reply(&request_id, parent_thread_id, child_thread_id)
        .expect("parent should claim pending request");
    let cancellation_token = CancellationToken::new();

    let wait = wait_for_parent_outcome(
        receiver,
        &cancellation_token,
        Duration::ZERO,
        &control,
        &request_id,
    );
    let deliver = claim.deliver("approved".to_string());
    let (outcome, delivered) = tokio::join!(wait, deliver);

    assert_eq!(outcome, ParentWaitResult::Answered("approved".to_string()));
    assert_eq!(delivered, Ok(()));
}
