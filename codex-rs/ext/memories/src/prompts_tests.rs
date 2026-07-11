use super::*;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use tempfile::tempdir;
use tokio::fs as tokio_fs;

const GLOBAL_UPDATE_HEADING: &str = "Updating global memories:";
const GLOBAL_DELETE_SCOPE: &str = "with `scope: \"global\"`";
const OLD_UNSCOPED_UPDATE_GATE: &str = "You can update the memories **only**";

#[tokio::test]
async fn build_memory_tool_developer_instructions_renders_embedded_template() {
    let temp = tempdir().unwrap();
    let codex_home = AbsolutePathBuf::from_absolute_path(temp.path()).unwrap();
    let memories_dir = codex_home.join("memories");
    tokio_fs::create_dir_all(&memories_dir).await.unwrap();
    tokio_fs::write(
        memories_dir.join("memory_summary.md"),
        "Short memory summary for tests.",
    )
    .await
    .unwrap();

    let instructions = build_memory_tool_developer_instructions(&codex_home)
        .await
        .unwrap();

    assert!(instructions.contains(&format!(
        "- {}/memory_summary.md (already provided below; do NOT open again)",
        memories_dir.display()
    )));
    assert!(instructions.contains("Short memory summary for tests."));
    assert!(instructions.contains(GLOBAL_UPDATE_HEADING));
    assert!(instructions.contains(GLOBAL_DELETE_SCOPE));
    assert!(!instructions.contains(OLD_UNSCOPED_UPDATE_GATE));
    assert_eq!(
        instructions
            .matches("========= MEMORY_SUMMARY BEGINS =========")
            .count(),
        1
    );
}
