use codex_extension_api::JsonToolOutput;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_items::memory_mutation::MemoryMutation;
use codex_extension_items::memory_mutation::MemoryMutationScope;
use codex_extension_items::memory_mutation::MemoryMutationStatus;
use codex_otel::MetricsClient;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::ADD_AD_HOC_NOTE_TOOL_NAME;
use crate::backend::AddAdHocMemoryNoteRequest;
use crate::backend::AddAdHocMemoryNoteResponse;
use crate::metrics::record_tool_call;
use crate::scoped::MemoryToolBackends;

use super::backend_error_to_function_call;
use super::memory_function_tool;
use super::memory_tool_name;
use super::parse_args;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddAdHocNoteArgs {
    /// Name of the note file to create, in
    /// YYYY-MM-DDTHH-MM-SS-<slug>.md format. The slug must use only lowercase
    /// ASCII letters, digits, and hyphens.
    #[schemars(
        length(min = 24, max = 128),
        regex(pattern = r"^\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}-[a-z0-9][a-z0-9-]{0,79}\.md$")
    )]
    filename: String,
    /// Verbatim Markdown note to append to the ad-hoc memory notes.
    #[schemars(length(min = 1))]
    note: String,
}

#[derive(Clone)]
pub(super) struct AddAdHocNoteTool {
    pub(super) backends: MemoryToolBackends,
    pub(super) metrics_client: Option<MetricsClient>,
}

impl ToolExecutor<ToolCall> for AddAdHocNoteTool {
    fn tool_name(&self) -> ToolName {
        memory_tool_name(ADD_AD_HOC_NOTE_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        memory_function_tool::<AddAdHocNoteArgs, AddAdHocMemoryNoteResponse>(
            ADD_AD_HOC_NOTE_TOOL_NAME,
            "Create one append-only ad-hoc memory note after the user explicitly asks Codex to remember, forget, or update something.",
        )
    }

    fn handle(&self, call: ToolCall) -> codex_extension_api::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(call))
    }
}

impl AddAdHocNoteTool {
    async fn handle_call(
        &self,
        call: ToolCall,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, codex_extension_api::FunctionCallError>
    {
        let backends = self.backends.clone();
        let args: AddAdHocNoteArgs = parse_args(&call)?;
        let path = format!("extensions/ad_hoc/notes/{}", args.filename);
        let mutation = MemoryMutation::write(
            call.call_id.clone(),
            MemoryMutationScope::Global,
            /*title*/ None,
            &args.note,
        );
        call.turn_item_emitter
            .emit_started(super::memory_mutation_turn_item(mutation.clone()))
            .await;
        let response = backends
            .add_global_ad_hoc_note(AddAdHocMemoryNoteRequest {
                filename: args.filename,
                note: args.note,
            })
            .await;
        record_tool_call(
            self.metrics_client.as_ref(),
            ADD_AD_HOC_NOTE_TOOL_NAME,
            "ad_hoc_notes",
            response.is_ok(),
            "not_applicable",
        );
        match response {
            Ok(response) => {
                call.turn_item_emitter
                    .emit_completed(super::memory_mutation_turn_item(
                        mutation
                            .with_status(MemoryMutationStatus::Succeeded)
                            .with_path(path),
                    ))
                    .await;
                Ok(Box::new(JsonToolOutput::new(json!(response))))
            }
            Err(error) => {
                call.turn_item_emitter
                    .emit_completed(super::memory_mutation_turn_item(
                        mutation.with_status(MemoryMutationStatus::Failed),
                    ))
                    .await;
                Err(backend_error_to_function_call(error))
            }
        }
    }
}
