use super::*;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

const COMMAND: [&str; 2] = ["id", "-u"];
const CREDENTIAL: &str = "secret";

fn command() -> Arc<SudoOnceCommand> {
    Arc::new(SudoOnceCommand::new(
        ThreadId::new(),
        Arc::from(COMMAND.map(str::to_string)),
        AbsolutePathBuf::try_from("/tmp").expect("absolute cwd"),
        None,
    ))
}

#[tokio::test]
async fn approval_grant_retains_the_frozen_command_snapshot() {
    let (broker, mut prompts) = LocalSudoOnceBroker::new();
    let expected = command();
    let request = broker.request_approval(Arc::clone(&expected));
    tokio::pin!(request);

    let prompt = tokio::select! {
        prompt = prompts.recv() => prompt,
        _ = &mut request => panic!("approval resolved before the prompt"),
    };
    let Some(SudoOncePrompt::Approval(prompt)) = prompt else {
        panic!("expected approval prompt");
    };
    let (prompt_command, responder) = prompt.into_parts();
    assert!(Arc::ptr_eq(&expected, &prompt_command));
    assert!(responder.approve());
    let grant = request.await.expect("grant");
    assert!(Arc::ptr_eq(&expected, grant.command()));
    assert_eq!(grant.command().argv(), COMMAND);
}

#[tokio::test]
async fn dropped_approval_prompt_denies_the_command() {
    let (broker, mut prompts) = LocalSudoOnceBroker::new();
    let request = broker.request_approval(command());
    tokio::pin!(request);

    let prompt = tokio::select! {
        prompt = prompts.recv() => prompt,
        _ = &mut request => panic!("approval resolved before the prompt"),
    };
    drop(prompt);
    assert!(request.await.is_none());
}

#[tokio::test]
async fn credential_attempts_use_distinct_single_use_responders() {
    let (broker, mut prompts) = LocalSudoOnceBroker::new();
    let approval = broker.request_approval(command());
    tokio::pin!(approval);
    let prompt = tokio::select! {
        prompt = prompts.recv() => prompt,
        _ = &mut approval => panic!("approval resolved before the prompt"),
    };
    let Some(SudoOncePrompt::Approval(prompt)) = prompt else {
        panic!("expected approval prompt");
    };
    let (_, responder) = prompt.into_parts();
    assert!(responder.approve());
    let grant = approval.await.expect("grant");

    let first = broker.request_credential(&grant, 1);
    tokio::pin!(first);
    let prompt = tokio::select! {
        prompt = prompts.recv() => prompt,
        _ = &mut first => panic!("credential resolved before the prompt"),
    };
    let Some(SudoOncePrompt::Credential(prompt)) = prompt else {
        panic!("expected first credential prompt");
    };
    let (_, attempt, responder) = prompt.into_parts();
    assert_eq!(attempt, 1);
    assert!(responder.cancel());
    assert!(first.await.is_none());

    let second = broker.request_credential(&grant, 2);
    tokio::pin!(second);
    let prompt = tokio::select! {
        prompt = prompts.recv() => prompt,
        _ = &mut second => panic!("credential resolved before the prompt"),
    };
    let Some(SudoOncePrompt::Credential(prompt)) = prompt else {
        panic!("expected second credential prompt");
    };
    let (_, attempt, responder) = prompt.into_parts();
    assert_eq!(attempt, 2);
    assert!(responder.submit(SudoOnceCredential::new(CREDENTIAL.to_string())));
    assert_eq!(
        second.await.expect("credential").expose_secret(),
        CREDENTIAL
    );
}
