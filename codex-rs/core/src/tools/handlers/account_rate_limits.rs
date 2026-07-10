use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::account_rate_limits_spec::GET_ACCOUNT_RATE_LIMITS_TOOL_NAME;
use crate::tools::handlers::account_rate_limits_spec::create_get_account_rate_limits_tool;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_backend_client::Client as BackendClient;
use codex_model_provider_info::OPENAI_PROVIDER_ID;
use codex_protocol::account::PlanType;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::protocol::RateLimitReachedType;
use codex_protocol::protocol::RateLimitSnapshot;
use codex_protocol::protocol::RateLimitWindow;
use codex_tools::ToolName;
use codex_tools::ToolSpec;
use serde::Serialize;
use serde_json::Value as JsonValue;

const MAX_RATE_LIMIT_BUCKETS: usize = 4;
const MAX_BACKEND_STRING_BYTES: usize = 256;
const MAX_SERIALIZED_OUTPUT_BYTES: usize = 8 * 1024;
const CLIENT_ERROR_MESSAGE: &str = "failed to construct account rate limits client";
const FETCH_ERROR_MESSAGE: &str = "failed to fetch account rate limits";
const SERIALIZATION_ERROR_MESSAGE: &str = "failed to serialize account rate limits response";
const OUTPUT_TOO_LARGE_ERROR_MESSAGE: &str = "account rate limits response exceeded size limit";

pub struct AccountRateLimitsHandler;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AccountRateLimitsUnavailableReason {
    NotLoggedIn,
    ApiKeyAuth,
    CustomProvider,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct AccountRateLimitsResponse {
    available: bool,
    unavailable_reason: Option<AccountRateLimitsUnavailableReason>,
    total_rate_limit_count: usize,
    truncated: bool,
    rate_limits: Vec<AccountRateLimitSnapshot>,
}

impl AccountRateLimitsResponse {
    fn unavailable(reason: AccountRateLimitsUnavailableReason) -> Self {
        Self {
            available: false,
            unavailable_reason: Some(reason),
            total_rate_limit_count: 0,
            truncated: false,
            rate_limits: Vec::new(),
        }
    }

    fn available(rate_limits: Vec<RateLimitSnapshot>) -> Self {
        let total_rate_limit_count = rate_limits.len();
        let mut truncated = total_rate_limit_count > MAX_RATE_LIMIT_BUCKETS;
        let rate_limits = rate_limits
            .into_iter()
            .take(MAX_RATE_LIMIT_BUCKETS)
            .map(|snapshot| AccountRateLimitSnapshot::from_backend(snapshot, &mut truncated))
            .collect();
        Self {
            available: true,
            unavailable_reason: None,
            total_rate_limit_count,
            truncated,
            rate_limits,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct AccountRateLimitSnapshot {
    limit_id: Option<String>,
    limit_name: Option<String>,
    primary: Option<AccountRateLimitWindow>,
    secondary: Option<AccountRateLimitWindow>,
    credits: Option<AccountCreditsSnapshot>,
    individual_limit: Option<AccountSpendControlLimitSnapshot>,
    plan_type: Option<PlanType>,
    rate_limit_reached_type: Option<RateLimitReachedType>,
}

impl AccountRateLimitSnapshot {
    fn from_backend(snapshot: RateLimitSnapshot, truncated: &mut bool) -> Self {
        Self {
            limit_id: truncate_optional_backend_string(snapshot.limit_id, truncated),
            limit_name: truncate_optional_backend_string(snapshot.limit_name, truncated),
            primary: snapshot.primary.map(AccountRateLimitWindow::from),
            secondary: snapshot.secondary.map(AccountRateLimitWindow::from),
            credits: snapshot.credits.map(|credits| AccountCreditsSnapshot {
                has_credits: credits.has_credits,
                unlimited: credits.unlimited,
                balance: truncate_optional_backend_string(credits.balance, truncated),
            }),
            individual_limit: snapshot.individual_limit.map(|limit| {
                AccountSpendControlLimitSnapshot {
                    limit: truncate_backend_string(limit.limit, truncated),
                    used: truncate_backend_string(limit.used, truncated),
                    remaining_percent: limit.remaining_percent,
                    resets_at: limit.resets_at,
                }
            }),
            plan_type: snapshot.plan_type,
            rate_limit_reached_type: snapshot.rate_limit_reached_type,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct AccountCreditsSnapshot {
    has_credits: bool,
    unlimited: bool,
    balance: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct AccountSpendControlLimitSnapshot {
    limit: String,
    used: String,
    remaining_percent: i32,
    resets_at: i64,
}

fn truncate_optional_backend_string(value: Option<String>, truncated: &mut bool) -> Option<String> {
    value.map(|value| truncate_backend_string(value, truncated))
}

fn truncate_backend_string(mut value: String, truncated: &mut bool) -> String {
    if value.len() > MAX_BACKEND_STRING_BYTES {
        let mut boundary = MAX_BACKEND_STRING_BYTES;
        while !value.is_char_boundary(boundary) {
            boundary -= 1;
        }
        value.truncate(boundary);
        *truncated = true;
    }
    value
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct AccountRateLimitWindow {
    used_percent: f64,
    remaining_percent: f64,
    window_minutes: Option<i64>,
    resets_at: Option<i64>,
}

impl From<RateLimitWindow> for AccountRateLimitWindow {
    fn from(window: RateLimitWindow) -> Self {
        Self {
            used_percent: window.used_percent,
            remaining_percent: (100.0 - window.used_percent).clamp(0.0, 100.0),
            window_minutes: window.window_minutes,
            resets_at: window.resets_at,
        }
    }
}

#[derive(Debug)]
struct AccountRateLimitsOutput {
    response: JsonValue,
    text: String,
}

impl AccountRateLimitsOutput {
    fn new(response: AccountRateLimitsResponse) -> Result<Self, FunctionCallError> {
        let text = serde_json::to_string(&response).map_err(|err| {
            tracing::warn!(%err, "failed to serialize account rate limits response");
            FunctionCallError::RespondToModel(SERIALIZATION_ERROR_MESSAGE.to_string())
        })?;
        if text.len() > MAX_SERIALIZED_OUTPUT_BYTES {
            tracing::warn!(
                serialized_bytes = text.len(),
                max_serialized_bytes = MAX_SERIALIZED_OUTPUT_BYTES,
                "account rate limits response exceeded size limit"
            );
            return Err(FunctionCallError::RespondToModel(
                OUTPUT_TOO_LARGE_ERROR_MESSAGE.to_string(),
            ));
        }
        let response = serde_json::from_str(&text).map_err(|err| {
            tracing::warn!(%err, "failed to parse serialized account rate limits response");
            FunctionCallError::RespondToModel(SERIALIZATION_ERROR_MESSAGE.to_string())
        })?;
        Ok(Self { response, text })
    }
}

impl ToolOutput for AccountRateLimitsOutput {
    fn log_preview(&self) -> String {
        self.text.clone()
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, _payload: &ToolPayload) -> ResponseInputItem {
        let mut output = FunctionCallOutputPayload::from_text(self.text.clone());
        output.success = Some(true);
        ResponseInputItem::FunctionCallOutput {
            call_id: call_id.to_string(),
            output,
        }
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        self.response.clone()
    }
}

impl ToolExecutor<ToolInvocation> for AccountRateLimitsHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(GET_ACCOUNT_RATE_LIMITS_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        create_get_account_rate_limits_tool()
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(invocation))
    }
}

impl AccountRateLimitsHandler {
    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let ToolInvocation { turn, payload, .. } = invocation;
        if !matches!(payload, ToolPayload::Function { .. }) {
            return Err(FunctionCallError::RespondToModel(format!(
                "{GET_ACCOUNT_RATE_LIMITS_TOOL_NAME} handler received unsupported payload"
            )));
        }

        if turn.config.model_provider_id != OPENAI_PROVIDER_ID {
            return Self::output(AccountRateLimitsResponse::unavailable(
                AccountRateLimitsUnavailableReason::CustomProvider,
            ));
        }

        let Some(auth_manager) = turn.auth_manager.as_ref() else {
            return Self::output(AccountRateLimitsResponse::unavailable(
                AccountRateLimitsUnavailableReason::NotLoggedIn,
            ));
        };
        let Some(auth) = auth_manager.auth().await else {
            return Self::output(AccountRateLimitsResponse::unavailable(
                AccountRateLimitsUnavailableReason::NotLoggedIn,
            ));
        };
        if !auth.uses_codex_backend() {
            return Self::output(AccountRateLimitsResponse::unavailable(
                AccountRateLimitsUnavailableReason::ApiKeyAuth,
            ));
        }

        let client = BackendClient::from_auth(turn.config.chatgpt_base_url.clone(), &auth)
            .map_err(|err| {
                tracing::warn!(%err, "failed to construct account rate limits client");
                FunctionCallError::RespondToModel(CLIENT_ERROR_MESSAGE.to_string())
            })?;
        let rate_limits = client.get_rate_limits_many().await.map_err(|err| {
            tracing::warn!(%err, "failed to fetch account rate limits");
            FunctionCallError::RespondToModel(FETCH_ERROR_MESSAGE.to_string())
        })?;
        if rate_limits.is_empty() {
            tracing::warn!("account rate limits response contained no snapshots");
            return Err(FunctionCallError::RespondToModel(
                FETCH_ERROR_MESSAGE.to_string(),
            ));
        }

        Self::output(AccountRateLimitsResponse::available(rate_limits))
    }

    fn output(
        response: AccountRateLimitsResponse,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        Ok(boxed_tool_output(AccountRateLimitsOutput::new(response)?))
    }
}

impl CoreToolRuntime for AccountRateLimitsHandler {}

#[cfg(test)]
#[path = "account_rate_limits_tests.rs"]
mod tests;
