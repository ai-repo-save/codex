use crate::tools::handlers::ListContextAnchorsHandler;
use crate::tools::handlers::RequestContextCompactionHandler;
use crate::tools::handlers::RewindContextToAnchorHandler;
use crate::tools::handlers::SaveContextAnchorHandler;
use crate::tools::registry::ToolExposure;
use crate::tools::registry::ToolRegistry;

/// Fork-owned context session-control tools.
///
/// These bypass code mode (`DirectModelOnly`) so turn-level side effects run
/// even when nested tools are otherwise hidden from the model.
pub(super) fn add_to(registry: &mut ToolRegistry) {
    registry.add_with_exposure(
        RequestContextCompactionHandler,
        ToolExposure::DirectModelOnly,
    );
    registry.add_with_exposure(SaveContextAnchorHandler, ToolExposure::DirectModelOnly);
    registry.add_with_exposure(ListContextAnchorsHandler, ToolExposure::DirectModelOnly);
    registry.add_with_exposure(RewindContextToAnchorHandler, ToolExposure::DirectModelOnly);
}
