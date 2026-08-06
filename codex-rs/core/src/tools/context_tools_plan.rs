use crate::tools::handlers::ListContextAnchorsHandler;
use crate::tools::handlers::RequestContextCompactionHandler;
use crate::tools::handlers::RewindContextToAnchorHandler;
use crate::tools::handlers::SaveContextAnchorHandler;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExposure;
use crate::tools::registry::override_tool_exposure;
use std::sync::Arc;

/// Fork-owned context session-control tools.
///
/// These bypass code mode (`DirectModelOnly`) so turn-level side effects run
/// even when nested tools are otherwise hidden from the model.
pub(super) fn build() -> Vec<Arc<dyn CoreToolRuntime>> {
    vec![
        override_tool_exposure(
            Arc::new(RequestContextCompactionHandler),
            ToolExposure::DirectModelOnly,
        ),
        override_tool_exposure(
            Arc::new(SaveContextAnchorHandler),
            ToolExposure::DirectModelOnly,
        ),
        override_tool_exposure(
            Arc::new(ListContextAnchorsHandler),
            ToolExposure::DirectModelOnly,
        ),
        override_tool_exposure(
            Arc::new(RewindContextToAnchorHandler),
            ToolExposure::DirectModelOnly,
        ),
    ]
}
