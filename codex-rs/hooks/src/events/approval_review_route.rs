//! Approval-review route hook execution.
//!
//! This event runs after a request has reached an approval decision point and
//! before Codex chooses between the configured auto-reviewer and the user.
//! Handlers can select the reviewer route for this request, or decline to
//! decide and let the static config path continue.

use std::path::PathBuf;

use super::common;
use crate::engine::CommandShell;
use crate::engine::ConfiguredHandler;
use crate::engine::PromptHookRunner;
use crate::engine::command_runner::CommandRunResult;
use crate::engine::dispatcher;
use crate::engine::output_parser;
use crate::schema::ApprovalReviewRouteCommandInput;
use crate::schema::SubagentCommandInputFields;
use codex_protocol::ThreadId;
use codex_protocol::protocol::HookCompletedEvent;
use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookHandlerType;
use codex_protocol::protocol::HookOutputEntry;
use codex_protocol::protocol::HookOutputEntryKind;
use codex_protocol::protocol::HookRunStatus;
use codex_protocol::protocol::HookRunSummary;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ApprovalReviewRouteRequest {
    pub session_id: ThreadId,
    pub turn_id: String,
    pub subagent: Option<common::SubagentHookContext>,
    pub cwd: PathBuf,
    pub transcript_path: Option<PathBuf>,
    pub model: String,
    pub permission_mode: String,
    pub tool_name: String,
    pub matcher_aliases: Vec<String>,
    pub run_id_suffix: String,
    pub tool_input: Value,
    pub approval_kind: String,
    pub approval_policy: String,
    pub strict_auto_review: bool,
    pub static_auto_review_enabled: bool,
    pub retry_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalReviewRouteDecision {
    AutoReview,
    User,
}

#[derive(Debug)]
pub struct ApprovalReviewRouteOutcome {
    pub hook_events: Vec<HookCompletedEvent>,
    pub decision: Option<ApprovalReviewRouteDecision>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ApprovalReviewRouteHandlerData {
    decision: Option<ApprovalReviewRouteDecision>,
    force_user: bool,
}

pub(crate) fn preview(
    handlers: &[ConfiguredHandler],
    request: &ApprovalReviewRouteRequest,
) -> Vec<HookRunSummary> {
    let matcher_inputs = common::matcher_inputs(&request.tool_name, &request.matcher_aliases);
    dispatcher::select_handlers_for_matcher_inputs(
        handlers,
        HookEventName::ApprovalReviewRoute,
        &matcher_inputs,
    )
    .into_iter()
    .map(|handler| {
        common::hook_run_for_tool_use(
            dispatcher::running_summary(&handler),
            &request.run_id_suffix,
        )
    })
    .collect()
}

pub(crate) async fn run(
    handlers: &[ConfiguredHandler],
    shell: &CommandShell,
    prompt_runner: Option<&dyn PromptHookRunner>,
    request: ApprovalReviewRouteRequest,
) -> ApprovalReviewRouteOutcome {
    let matcher_inputs = common::matcher_inputs(&request.tool_name, &request.matcher_aliases);
    let matched = dispatcher::select_handlers_for_matcher_inputs(
        handlers,
        HookEventName::ApprovalReviewRoute,
        &matcher_inputs,
    );
    if matched.is_empty() {
        return ApprovalReviewRouteOutcome {
            hook_events: Vec::new(),
            decision: None,
        };
    }

    let input_json = match serde_json::to_string(&build_command_input(&request)) {
        Ok(input_json) => input_json,
        Err(error) => {
            let hook_events = common::serialization_failure_hook_events_for_tool_use(
                matched,
                Some(request.turn_id.clone()),
                format!("failed to serialize approval review route hook input: {error}"),
                &request.run_id_suffix,
            );
            return ApprovalReviewRouteOutcome {
                hook_events,
                decision: None,
            };
        }
    };

    let results = dispatcher::execute_handlers(
        matched,
        input_json,
        dispatcher::HandlerExecutionContext {
            shell,
            prompt_runner,
            cwd: request.cwd.as_path(),
            turn_id: Some(request.turn_id.clone()),
        },
        parse_completed,
    )
    .await;
    let decision = resolve_approval_review_route_decision(results.iter().map(|result| result.data));

    ApprovalReviewRouteOutcome {
        hook_events: results
            .into_iter()
            .map(|result| {
                common::hook_completed_for_tool_use(result.completed, &request.run_id_suffix)
            })
            .collect(),
        decision,
    }
}

fn resolve_approval_review_route_decision(
    handler_data: impl IntoIterator<Item = ApprovalReviewRouteHandlerData>,
) -> Option<ApprovalReviewRouteDecision> {
    let mut decision = None;
    for data in handler_data {
        if data.force_user {
            return Some(ApprovalReviewRouteDecision::User);
        }
        if data.decision.is_some() {
            decision = data.decision;
        }
    }
    decision
}

fn build_command_input(request: &ApprovalReviewRouteRequest) -> ApprovalReviewRouteCommandInput {
    let subagent = SubagentCommandInputFields::from(request.subagent.as_ref());
    ApprovalReviewRouteCommandInput {
        session_id: request.session_id.to_string(),
        turn_id: request.turn_id.clone(),
        agent_id: subagent.agent_id,
        agent_type: subagent.agent_type,
        transcript_path: crate::schema::NullableString::from_path(request.transcript_path.clone()),
        cwd: request.cwd.display().to_string(),
        hook_event_name: "ApprovalReviewRoute".to_string(),
        model: request.model.clone(),
        permission_mode: request.permission_mode.clone(),
        tool_name: request.tool_name.clone(),
        tool_input: request.tool_input.clone(),
        approval_kind: request.approval_kind.clone(),
        approval_policy: request.approval_policy.clone(),
        strict_auto_review: request.strict_auto_review,
        static_auto_review_enabled: request.static_auto_review_enabled,
        retry_reason: request.retry_reason.clone(),
    }
}

fn parse_completed(
    handler: &ConfiguredHandler,
    run_result: CommandRunResult,
    turn_id: Option<String>,
) -> dispatcher::ParsedHandler<ApprovalReviewRouteHandlerData> {
    let mut entries = Vec::new();
    let mut status = HookRunStatus::Completed;
    let mut decision = None;

    match run_result.error.as_deref() {
        Some(error) => {
            status = HookRunStatus::Failed;
            entries.push(HookOutputEntry {
                kind: HookOutputEntryKind::Error,
                text: error.to_string(),
            });
        }
        None => match run_result.exit_code {
            Some(0) => {
                let trimmed_stdout = run_result.stdout.trim();
                if trimmed_stdout.is_empty() {
                    if handler.handler_type() == HookHandlerType::Prompt {
                        status = HookRunStatus::Failed;
                        entries.push(HookOutputEntry {
                            kind: HookOutputEntryKind::Error,
                            text: "prompt hook returned empty output".to_string(),
                        });
                    }
                } else if let Some(parsed) =
                    output_parser::parse_approval_review_route(&run_result.stdout)
                {
                    if let Some(system_message) = parsed.universal.system_message {
                        entries.push(HookOutputEntry {
                            kind: HookOutputEntryKind::Warning,
                            text: system_message,
                        });
                    }
                    if let Some(invalid_reason) = parsed.invalid_reason {
                        status = HookRunStatus::Failed;
                        entries.push(HookOutputEntry {
                            kind: HookOutputEntryKind::Error,
                            text: invalid_reason,
                        });
                    } else {
                        decision = parsed.reviewer.map(|reviewer| match reviewer {
                            output_parser::ApprovalReviewRouteReviewer::AutoReview => {
                                ApprovalReviewRouteDecision::AutoReview
                            }
                            output_parser::ApprovalReviewRouteReviewer::User => {
                                ApprovalReviewRouteDecision::User
                            }
                        });
                    }
                } else if handler.handler_type() == HookHandlerType::Prompt
                    || output_parser::looks_like_json(&run_result.stdout)
                {
                    status = HookRunStatus::Failed;
                    entries.push(HookOutputEntry {
                        kind: HookOutputEntryKind::Error,
                        text: "hook returned invalid approval-review-route JSON output".to_string(),
                    });
                }
            }
            Some(code) => {
                status = HookRunStatus::Failed;
                entries.push(HookOutputEntry {
                    kind: HookOutputEntryKind::Error,
                    text: format!("ApprovalReviewRoute hook exited with code {code}"),
                });
            }
            None => {
                status = HookRunStatus::Failed;
                entries.push(HookOutputEntry {
                    kind: HookOutputEntryKind::Error,
                    text: "ApprovalReviewRoute hook exited without an exit code".to_string(),
                });
            }
        },
    }

    let force_user = status == HookRunStatus::Failed && handler.fail_closed();

    dispatcher::ParsedHandler {
        completed: HookCompletedEvent {
            turn_id,
            run: dispatcher::completed_summary(handler, &run_result, status, entries),
        },
        data: ApprovalReviewRouteHandlerData {
            decision,
            force_user,
        },
        completion_order: 0,
    }
}

#[cfg(test)]
#[path = "approval_review_route_tests.rs"]
mod tests;
