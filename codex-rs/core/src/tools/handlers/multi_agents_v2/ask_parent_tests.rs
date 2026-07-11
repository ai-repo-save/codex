use super::*;

#[tokio::test]
async fn ready_parent_reply_wins_over_ready_cancellation() {
    let (sender, receiver) = oneshot::channel();
    sender
        .send(ParentRequestOutcome::Answered("approved".to_string()))
        .expect("receiver should remain open");
    let cancellation_token = CancellationToken::new();
    cancellation_token.cancel();

    let outcome = wait_for_parent_outcome(receiver, &cancellation_token, Duration::ZERO).await;

    assert!(matches!(
        outcome,
        Some(ParentRequestOutcome::Answered(answer)) if answer == "approved"
    ));
}
