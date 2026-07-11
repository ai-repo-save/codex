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

const CLIENT_ERROR_MESSAGE: &str = "failed to construct account rate limits client";
const FETCH_ERROR_MESSAGE: &str = "failed to fetch account rate limits";
const SERIALIZATION_ERROR_MESSAGE: &str = "failed to serialize account rate limits response";

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
    rate_limits: Vec<AccountRateLimitSnapshot>,
}

impl AccountRateLimitsResponse {
    fn unavailable(reason: AccountRateLimitsUnavailableReason) -> Self {
        Self {
            available: false,
            unavailable_reason: Some(reason),
            rate_limits: Vec::new(),
        }
    }

    fn available(rate_limits: Vec<RateLimitSnapshot>) -> Self {
        let rate_limits = rate_limits
            .into_iter()
            .map(AccountRateLimitSnapshot::from)
            .collect();
        Self {
            available: true,
            unavailable_reason: None,
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

impl From<RateLimitSnapshot> for AccountRateLimitSnapshot {
    fn from(snapshot: RateLimitSnapshot) -> Self {
        Self {
            limit_id: snapshot.limit_id,
            limit_name: snapshot.limit_name,
            primary: snapshot.primary.map(AccountRateLimitWindow::from),
            secondary: snapshot.secondary.map(AccountRateLimitWindow::from),
            credits: snapshot.credits.map(|credits| AccountCreditsSnapshot {
                has_credits: credits.has_credits,
                unlimited: credits.unlimited,
                balance: credits.balance,
            }),
            individual_limit: snapshot.individual_limit.map(|limit| {
                AccountSpendControlLimitSnapshot {
                    limit: limit.limit,
                    used: limit.used,
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
