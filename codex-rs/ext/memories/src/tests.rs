use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

use codex_extension_api::ContextContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ExtensionTurnItem;
use codex_extension_api::NoopTurnItemEmitter;
use codex_extension_api::PromptSlot;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolContributor;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolPayload;
use codex_extension_api::TurnItem;
use codex_extension_api::TurnItemEmissionFuture;
use codex_extension_api::TurnItemEmitter;
use codex_extension_items::ExtensionItem;
use codex_extension_items::memory_mutation::MEMORY_MUTATION_PATH_MAX_GRAPHEMES;
use codex_extension_items::memory_mutation::MEMORY_MUTATION_PREVIEW_MAX_GRAPHEMES;
use codex_extension_items::memory_mutation::MEMORY_MUTATION_TITLE_MAX_GRAPHEMES;
use codex_extension_items::memory_mutation::MemoryMutation;
use codex_extension_items::memory_mutation::MemoryMutationScope;
use codex_extension_items::memory_mutation::MemoryMutationStatus;
use codex_tools::ToolOutput;
use codex_utils_absolute_path::test_support::PathBufExt;
use codex_utils_absolute_path::test_support::PathExt;
use codex_utils_absolute_path::test_support::test_path_buf;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::approx_token_count;
use pretty_assertions::assert_eq;
use serde_json::json;
use unicode_segmentation::UnicodeSegmentation;

use crate::extension::MemoriesExtension;
use crate::extension::MemoriesExtensionConfig;
use crate::scoped::GLOBAL_MEMORY_MAINTENANCE_POLICY;
use crate::scoped::MemoryScope;
use crate::scoped::MemoryToolBackends;
use crate::scoped::PROJECT_MEMORY_MAINTENANCE_POLICY;
use crate::scoped::SESSION_CONTEXT_TOKEN_LIMIT;
use crate::scoped::SESSION_MEMORY_MAINTENANCE_POLICY;

#[test]
fn memory_tool_namespace_matches_responses_api_identifier() {
    assert!(!crate::MEMORY_TOOLS_NAMESPACE.is_empty());
    assert!(
        crate::MEMORY_TOOLS_NAMESPACE
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    );
}

#[test]
fn tools_are_not_contributed_without_thread_config() {
    let extension = MemoriesExtension::default();

    assert!(
        extension
            .tools(
                &ExtensionData::new("session"),
                &ExtensionData::new("thread")
            )
            .is_empty()
    );
}

#[test]
fn tools_are_not_contributed_when_disabled() {
    let extension = MemoriesExtension::default();
    let thread_store = ExtensionData::new("thread");
    thread_store.insert(MemoriesExtensionConfig {
        global_enabled: false,
        scoped_enabled: false,
        dedicated_tools: true,
        codex_home: test_path_buf("/tmp/codex-home").abs(),
        project_root: test_path_buf("/tmp/project").abs(),
    });

    assert!(
        extension
            .tools(&ExtensionData::new("session"), &thread_store)
            .is_empty()
    );
}

#[test]
fn tools_are_not_contributed_when_dedicated_tools_disabled() {
    let extension = MemoriesExtension::default();
    let thread_store = ExtensionData::new("thread");
    thread_store.insert(MemoriesExtensionConfig {
        global_enabled: true,
        scoped_enabled: false,
        dedicated_tools: false,
        codex_home: test_path_buf("/tmp/codex-home").abs(),
        project_root: test_path_buf("/tmp/project").abs(),
    });

    assert!(
        extension
            .tools(&ExtensionData::new("session"), &thread_store)
            .is_empty()
    );
}

#[test]
fn tools_are_contributed_when_enabled_with_dedicated_tools() {
    let extension = MemoriesExtension::default();
    let thread_store = ExtensionData::new("thread");
    thread_store.insert(MemoriesExtensionConfig {
        global_enabled: true,
        scoped_enabled: false,
        dedicated_tools: true,
        codex_home: test_path_buf("/tmp/codex-home").abs(),
        project_root: test_path_buf("/tmp/project").abs(),
    });

    let tool_names = extension
        .tools(&ExtensionData::new("session"), &thread_store)
        .into_iter()
        .map(|tool| tool.tool_name())
        .collect::<Vec<_>>();

    assert_eq!(
        tool_names,
        vec![
            memory_tool_name(crate::ADD_AD_HOC_NOTE_TOOL_NAME),
            memory_tool_name(crate::DELETE_TOOL_NAME),
            memory_tool_name(crate::LIST_TOOL_NAME),
            memory_tool_name(crate::READ_TOOL_NAME),
            memory_tool_name(crate::SEARCH_TOOL_NAME),
        ]
    );
}

#[test]
fn scoped_tools_are_contributed_without_global_memories() {
    let extension = MemoriesExtension::default();
    let thread_store = ExtensionData::new("thread-1");
    thread_store.insert(MemoriesExtensionConfig {
        global_enabled: false,
        scoped_enabled: true,
        dedicated_tools: true,
        codex_home: test_path_buf("/tmp/codex-home").abs(),
        project_root: test_path_buf("/tmp/project").abs(),
    });

    let tool_names = extension
        .tools(&ExtensionData::new("session"), &thread_store)
        .into_iter()
        .map(|tool| tool.tool_name())
        .collect::<Vec<_>>();

    assert_eq!(
        tool_names,
        vec![
            memory_tool_name(crate::WRITE_NOTE_TOOL_NAME),
            memory_tool_name(crate::DELETE_TOOL_NAME),
            memory_tool_name(crate::LIST_TOOL_NAME),
            memory_tool_name(crate::READ_TOOL_NAME),
            memory_tool_name(crate::SEARCH_TOOL_NAME),
        ]
    );
}

#[test]
fn install_registers_dedicated_tool_contributor() {
    let mut builder = ExtensionRegistryBuilder::<codex_core::config::Config>::new();
    crate::install(&mut builder, /*metrics_client*/ None);
    let registry = builder.build();
    let thread_store = ExtensionData::new("thread");
    thread_store.insert(MemoriesExtensionConfig {
        global_enabled: true,
        scoped_enabled: false,
        dedicated_tools: true,
        codex_home: test_path_buf("/tmp/codex-home").abs(),
        project_root: test_path_buf("/tmp/project").abs(),
    });

    let tool_names = registry
        .tool_contributors()
        .iter()
        .flat_map(|contributor| contributor.tools(&ExtensionData::new("session"), &thread_store))
        .map(|tool| tool.tool_name())
        .collect::<Vec<_>>();

    assert_eq!(
        tool_names,
        vec![
            memory_tool_name(crate::ADD_AD_HOC_NOTE_TOOL_NAME),
            memory_tool_name(crate::DELETE_TOOL_NAME),
            memory_tool_name(crate::LIST_TOOL_NAME),
            memory_tool_name(crate::READ_TOOL_NAME),
            memory_tool_name(crate::SEARCH_TOOL_NAME),
        ]
    );
}

#[test]
fn ad_hoc_tool_definition_includes_filename_contract() {
    let tool = memory_tool(
        Path::new("/tmp/codex-home/memories"),
        crate::ADD_AD_HOC_NOTE_TOOL_NAME,
    );
    let spec = serde_json::to_value(tool.spec()).expect("serialize tool spec");

    let filename = spec
        .pointer("/tools/0/parameters/properties/filename")
        .expect("filename parameter should be in tool schema");
    assert_eq!(filename.pointer("/type"), Some(&json!("string")));
    assert!(
        filename
            .pointer("/description")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|description| description.contains("YYYY-MM-DDTHH-MM-SS-<slug>.md"))
    );
}

#[test]
fn scoped_tool_definitions_expose_scope_specific_maintenance_policy() {
    let backends = MemoryToolBackends::new(
        &test_path_buf("/tmp/codex-home").abs(),
        /*global_enabled*/ true,
        /*scoped_enabled*/ true,
        "thread-1",
        &test_path_buf("/tmp/project").abs(),
    );
    let write_spec = serde_json::to_value(
        memory_tool_from_backends(backends.clone(), crate::WRITE_NOTE_TOOL_NAME).spec(),
    )
    .expect("serialize write tool spec");
    let delete_spec =
        serde_json::to_value(memory_tool_from_backends(backends, crate::DELETE_TOOL_NAME).spec())
            .expect("serialize delete tool spec");
    let write_description = write_spec
        .pointer("/tools/0/description")
        .and_then(serde_json::Value::as_str)
        .expect("write tool description");
    let delete_description = delete_spec
        .pointer("/tools/0/description")
        .and_then(serde_json::Value::as_str)
        .expect("delete tool description");

    assert!(write_description.contains(SESSION_MEMORY_MAINTENANCE_POLICY));
    assert!(write_description.contains(PROJECT_MEMORY_MAINTENANCE_POLICY));
    assert!(delete_description.contains(SESSION_MEMORY_MAINTENANCE_POLICY));
    assert!(delete_description.contains(PROJECT_MEMORY_MAINTENANCE_POLICY));
    assert!(delete_description.contains(GLOBAL_MEMORY_MAINTENANCE_POLICY));
}

#[tokio::test]
async fn prompt_contribution_uses_memory_summary_when_enabled() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let memories_dir = tempdir.path().join("memories");
    tokio::fs::create_dir_all(&memories_dir)
        .await
        .expect("create memories dir");
    tokio::fs::write(
        memories_dir.join("memory_summary.md"),
        "Remember repository-specific implementation preferences.",
    )
    .await
    .expect("write memory summary");

    let extension = MemoriesExtension::default();
    let thread_store = ExtensionData::new("thread");
    thread_store.insert(MemoriesExtensionConfig {
        global_enabled: true,
        scoped_enabled: false,
        dedicated_tools: false,
        codex_home: tempdir.path().abs(),
        project_root: test_path_buf("/tmp/project").abs(),
    });

    let fragments = extension
        .contribute_thread_context(&ExtensionData::new("session"), &thread_store)
        .await;

    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0].slot(), PromptSlot::DeveloperPolicy);
    assert!(
        fragments[0]
            .text()
            .contains("Remember repository-specific implementation preferences.")
    );
}

#[tokio::test]
async fn prompt_contribution_includes_scoped_memory_when_global_memories_are_disabled() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let project_root = tempfile::tempdir().expect("project root");
    let backends = MemoryToolBackends::new(
        &tempdir.path().abs(),
        /*global_enabled*/ false,
        /*scoped_enabled*/ true,
        "thread-1",
        &project_root.path().abs(),
    );
    backends
        .write_note(
            MemoryScope::Session,
            "Current task preference".to_string(),
            "Remember this only for the current session.".to_string(),
        )
        .await
        .expect("write session note");

    let extension = MemoriesExtension::default();
    let thread_store = ExtensionData::new("thread-1");
    thread_store.insert(MemoriesExtensionConfig {
        global_enabled: false,
        scoped_enabled: true,
        dedicated_tools: true,
        codex_home: tempdir.path().abs(),
        project_root: project_root.path().abs(),
    });

    let fragments = extension
        .contribute_thread_context(&ExtensionData::new("session"), &thread_store)
        .await;

    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0].slot(), PromptSlot::ContextualUser);
    assert!(fragments[0].text().contains("<scoped_memory_context>"));
    assert!(
        fragments[0]
            .text()
            .contains(SESSION_MEMORY_MAINTENANCE_POLICY)
    );
    assert!(
        fragments[0]
            .text()
            .contains(PROJECT_MEMORY_MAINTENANCE_POLICY)
    );
    assert!(
        fragments[0]
            .text()
            .contains("Remember this only for the current session.")
    );
}

#[tokio::test]
async fn add_ad_hoc_note_tool_creates_note_file() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let memory_root = tempdir.path().join("memories");
    let tool = memory_tool(&memory_root, crate::ADD_AD_HOC_NOTE_TOOL_NAME);
    let payload = ToolPayload::Function {
        arguments: json!({
            "filename": "2026-05-26T13-42-08-remember-review-style.md",
            "note": "Remember to keep PR review comments concise.",
        })
        .to_string(),
    };

    let output = tool
        .handle(ToolCall {
            turn_id: "turn-1".to_string(),
            call_id: "call-1".to_string(),
            tool_name: memory_tool_name(crate::ADD_AD_HOC_NOTE_TOOL_NAME),
            model: "gpt-test".to_string(),
            codex_turn_metadata: None,
            truncation_policy: TruncationPolicy::Bytes(1024),
            conversation_history: codex_extension_api::ConversationHistory::default(),
            turn_item_emitter: Arc::new(NoopTurnItemEmitter),
            environments: Vec::new(),
            payload: payload.clone(),
        })
        .await
        .expect("ad-hoc note should be written");

    assert_eq!(
        output.post_tool_use_response("call-1", &payload),
        Some(json!({}))
    );
    assert_eq!(
        tokio::fs::read_to_string(
            memory_root
                .join("extensions/ad_hoc/notes")
                .join("2026-05-26T13-42-08-remember-review-style.md")
        )
        .await
        .expect("read ad-hoc note"),
        "Remember to keep PR review comments concise."
    );
}

#[tokio::test]
async fn memory_mutation_activities_cover_scopes_and_string_boundaries() {
    const FILENAME: &str = "2026-05-26T13-42-08-memory-mutation.md";
    const TITLE_GRAPHEME: &str = "👩‍💻";
    const PREVIEW_GRAPHEME: &str = "é";

    let tempdir = tempfile::tempdir().expect("tempdir");
    let project_root = tempfile::tempdir().expect("project root");
    let backends = MemoryToolBackends::new(
        &tempdir.path().abs(),
        /*global_enabled*/ true,
        /*scoped_enabled*/ true,
        "thread-1",
        &project_root.path().abs(),
    );
    let emitter = Arc::new(RecordingTurnItemEmitter::default());
    let title = TITLE_GRAPHEME.repeat(MEMORY_MUTATION_TITLE_MAX_GRAPHEMES + 1);
    let preview = format!(
        "\n  alpha\tbeta {}\nignored second line",
        PREVIEW_GRAPHEME.repeat(MEMORY_MUTATION_PREVIEW_MAX_GRAPHEMES)
    );

    let ad_hoc_tool = memory_tool_from_backends(backends.clone(), crate::ADD_AD_HOC_NOTE_TOOL_NAME);
    run_memory_tool_with_emitter(
        &ad_hoc_tool,
        crate::ADD_AD_HOC_NOTE_TOOL_NAME,
        "global-write",
        ToolPayload::Function {
            arguments: json!({ "filename": FILENAME, "note": preview }).to_string(),
        },
        emitter.clone(),
    )
    .await
    .expect("global note should be written");

    let write_tool = memory_tool_from_backends(backends.clone(), crate::WRITE_NOTE_TOOL_NAME);
    for (scope, call_id) in [("session", "session-write"), ("project", "project-write")] {
        run_memory_tool_with_emitter(
            &write_tool,
            crate::WRITE_NOTE_TOOL_NAME,
            call_id,
            ToolPayload::Function {
                arguments: json!({ "scope": scope, "title": title, "note": preview }).to_string(),
            },
            emitter.clone(),
        )
        .await
        .expect("scoped note should be written");
    }

    let delete_tool = memory_tool_from_backends(backends, crate::DELETE_TOOL_NAME);
    run_memory_tool_with_emitter(
        &delete_tool,
        crate::DELETE_TOOL_NAME,
        "global-delete",
        ToolPayload::Function {
            arguments: json!({
                "scope": "global",
                "path": format!("extensions/ad_hoc/notes/{FILENAME}"),
            })
            .to_string(),
        },
        emitter.clone(),
    )
    .await
    .expect("global note should be deleted");

    let long_path = "a".repeat(MEMORY_MUTATION_PATH_MAX_GRAPHEMES + 1);
    let delete_error = run_memory_tool_with_emitter(
        &delete_tool,
        crate::DELETE_TOOL_NAME,
        "path-boundary",
        ToolPayload::Function {
            arguments: json!({ "scope": "session", "path": long_path }).to_string(),
        },
        emitter.clone(),
    )
    .await
    .expect_err("missing note should fail");
    assert!(delete_error.to_string().contains("not found"));

    let started = emitter.started();
    let completed = emitter.completed();
    assert_eq!(started.len(), 5);
    assert_eq!(completed.len(), 5);
    assert_eq!(
        mutation_value(&started[0]),
        json!({
            "kind": "memory.mutation",
            "id": "global-write",
            "action": "write",
            "scope": "global",
            "status": "inProgress",
            "title": null,
            "path": null,
            "preview": format!("alpha beta {}", PREVIEW_GRAPHEME.repeat(149)),
        })
    );
    assert_eq!(mutation_value(&completed[0])["status"], json!("succeeded"));
    assert_eq!(
        mutation_value(&completed[0])["path"],
        json!(format!("extensions/ad_hoc/notes/{FILENAME}"))
    );
    assert_eq!(mutation_value(&completed[1])["scope"], json!("session"));
    assert_eq!(mutation_value(&completed[2])["scope"], json!("project"));
    assert_eq!(mutation_value(&completed[3])["action"], json!("delete"));
    assert_eq!(mutation_value(&completed[3])["status"], json!("succeeded"));
    assert_eq!(mutation_value(&completed[4])["status"], json!("failed"));

    for item in started.iter().chain(&completed) {
        let value = mutation_value(item);
        for field in ["title", "preview", "path"] {
            let Some(value) = value[field].as_str() else {
                continue;
            };
            let limit = match field {
                "title" => MEMORY_MUTATION_TITLE_MAX_GRAPHEMES,
                "preview" => MEMORY_MUTATION_PREVIEW_MAX_GRAPHEMES,
                "path" => MEMORY_MUTATION_PATH_MAX_GRAPHEMES,
                _ => unreachable!("checked mutation string field"),
            };
            assert!(value.graphemes(true).count() <= limit);
        }
    }
    assert_eq!(
        mutation_value(&completed[1])["title"],
        json!(TITLE_GRAPHEME.repeat(MEMORY_MUTATION_TITLE_MAX_GRAPHEMES))
    );
    assert_eq!(
        mutation_value(&completed[4])["path"],
        json!("a".repeat(MEMORY_MUTATION_PATH_MAX_GRAPHEMES))
    );
}

#[tokio::test]
async fn memory_mutation_failure_item_omits_backend_error_body() {
    const ERROR_SENTINEL: &str = "memory-mutation-error-sentinel";
    let tempdir = tempfile::tempdir().expect("tempdir");
    let memory_root = tempdir.path().join(ERROR_SENTINEL);
    tokio::fs::write(&memory_root, "not a directory")
        .await
        .expect("write memory root file");
    let tool = memory_tool(&memory_root, crate::ADD_AD_HOC_NOTE_TOOL_NAME);
    let emitter = Arc::new(RecordingTurnItemEmitter::default());

    let error = run_memory_tool_with_emitter(
        &tool,
        crate::ADD_AD_HOC_NOTE_TOOL_NAME,
        "error-write",
        ToolPayload::Function {
            arguments: json!({
                "filename": "2026-05-26T13-42-08-error.md",
                "note": "ordinary note",
            })
            .to_string(),
        },
        emitter.clone(),
    )
    .await
    .expect_err("non-directory root should fail");
    assert!(error.to_string().contains(ERROR_SENTINEL));

    let completed = emitter.completed();
    assert_eq!(completed.len(), 1);
    assert_eq!(mutation_value(&completed[0])["status"], json!("failed"));
    let emitted_items = completed.iter().map(mutation_value).collect::<Vec<_>>();
    assert!(
        !serde_json::to_string(&emitted_items)
            .expect("serialize mutation items")
            .contains(ERROR_SENTINEL)
    );
}

#[tokio::test]
async fn memory_read_operations_do_not_emit_mutation_activities() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let memory_root = tempdir.path().join("memories");
    tokio::fs::create_dir_all(&memory_root)
        .await
        .expect("create memories dir");
    tokio::fs::write(memory_root.join("MEMORY.md"), "Remember this memory.")
        .await
        .expect("write memory");
    let emitter = Arc::new(RecordingTurnItemEmitter::default());

    for (tool_name, call_id, payload) in [
        (
            crate::LIST_TOOL_NAME,
            "list-memory",
            ToolPayload::Function {
                arguments: json!({}).to_string(),
            },
        ),
        (
            crate::READ_TOOL_NAME,
            "read-memory",
            ToolPayload::Function {
                arguments: json!({ "path": "MEMORY.md" }).to_string(),
            },
        ),
        (
            crate::SEARCH_TOOL_NAME,
            "search-memory",
            ToolPayload::Function {
                arguments: json!({ "queries": ["Remember"] }).to_string(),
            },
        ),
    ] {
        let tool = memory_tool(&memory_root, tool_name);
        run_memory_tool_with_emitter(&tool, tool_name, call_id, payload, emitter.clone())
            .await
            .expect("memory read operation should succeed");
    }

    assert!(emitter.started().is_empty());
    assert!(emitter.completed().is_empty());
}

#[tokio::test]
async fn write_note_tool_creates_scoped_note_readable_by_scope() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let project_root = tempfile::tempdir().expect("project root");
    let backends = MemoryToolBackends::new(
        &tempdir.path().abs(),
        /*global_enabled*/ false,
        /*scoped_enabled*/ true,
        "thread-1",
        &project_root.path().abs(),
    );
    let write_tool = memory_tool_from_backends(backends.clone(), crate::WRITE_NOTE_TOOL_NAME);
    let write_payload = ToolPayload::Function {
        arguments: json!({
            "scope": "session",
            "title": "Review Style",
            "note": "Remember to keep review comments concise.",
        })
        .to_string(),
    };

    let write_output = write_tool
        .handle(ToolCall {
            turn_id: "turn-1".to_string(),
            call_id: "call-1".to_string(),
            tool_name: memory_tool_name(crate::WRITE_NOTE_TOOL_NAME),
            model: "gpt-test".to_string(),
            codex_turn_metadata: None,
            truncation_policy: TruncationPolicy::Bytes(1024),
            conversation_history: codex_extension_api::ConversationHistory::default(),
            turn_item_emitter: Arc::new(NoopTurnItemEmitter),
            environments: Vec::new(),
            payload: write_payload.clone(),
        })
        .await
        .expect("scoped note should be written");
    let write_response = write_output
        .post_tool_use_response("call-1", &write_payload)
        .expect("write response");
    let path = write_response
        .pointer("/path")
        .and_then(serde_json::Value::as_str)
        .expect("write response path");
    assert_eq!(write_response.pointer("/scope"), Some(&json!("session")));

    let read_tool = memory_tool_from_backends(backends, crate::READ_TOOL_NAME);
    let read_payload = ToolPayload::Function {
        arguments: json!({
            "scope": "session",
            "path": path,
        })
        .to_string(),
    };

    let read_output = read_tool
        .handle(ToolCall {
            turn_id: "turn-1".to_string(),
            call_id: "call-2".to_string(),
            tool_name: memory_tool_name(crate::READ_TOOL_NAME),
            model: "gpt-test".to_string(),
            codex_turn_metadata: None,
            truncation_policy: TruncationPolicy::Bytes(1024),
            conversation_history: codex_extension_api::ConversationHistory::default(),
            turn_item_emitter: Arc::new(NoopTurnItemEmitter),
            environments: Vec::new(),
            payload: read_payload.clone(),
        })
        .await
        .expect("scoped note should be readable");

    assert_eq!(
        read_output.post_tool_use_response("call-2", &read_payload),
        Some(json!({
            "path": path,
            "content": "Remember to keep review comments concise.",
            "start_line_number": 1,
            "truncated": false
        }))
    );
}

#[tokio::test]
async fn delete_tool_removes_global_file_from_memory_reads() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let memory_root = tempdir.path().join("memories");
    tokio::fs::create_dir_all(&memory_root)
        .await
        .expect("create memories dir");
    tokio::fs::write(memory_root.join("MEMORY.md"), "obsolete memory\n")
        .await
        .expect("write memory");
    let delete_tool = memory_tool(&memory_root, crate::DELETE_TOOL_NAME);
    let delete_payload = ToolPayload::Function {
        arguments: json!({
            "path": "MEMORY.md",
        })
        .to_string(),
    };

    let delete_response = run_memory_tool(
        &delete_tool,
        crate::DELETE_TOOL_NAME,
        "call-1",
        delete_payload,
    )
    .await
    .expect("delete should succeed");

    assert_eq!(
        delete_response,
        Some(json!({
            "scope": "global",
            "path": "MEMORY.md",
            "deleted": true
        }))
    );
    let mut deleted_entries = tokio::fs::read_dir(memory_root.join(".deleted"))
        .await
        .expect("deleted directory should exist");
    let deleted_entry = deleted_entries
        .next_entry()
        .await
        .expect("read deleted entry")
        .expect("deleted file should be preserved");
    assert_eq!(
        tokio::fs::read_to_string(deleted_entry.path())
            .await
            .expect("read deleted file"),
        "obsolete memory\n"
    );
    assert!(
        deleted_entries
            .next_entry()
            .await
            .expect("read deleted entries")
            .is_none()
    );
    let read_tool = memory_tool(&memory_root, crate::READ_TOOL_NAME);
    let read_payload = ToolPayload::Function {
        arguments: json!({
            "path": "MEMORY.md",
        })
        .to_string(),
    };
    let err = run_memory_tool(&read_tool, crate::READ_TOOL_NAME, "call-2", read_payload)
        .await
        .expect_err("deleted file should not be readable");

    assert!(err.to_string().contains("not found"));
}

#[tokio::test]
async fn replacing_scoped_notes_removes_obsolete_notes_from_context() {
    assert_replacing_scoped_note_removes_obsolete_note_from_context(MemoryScope::Session).await;
    assert_replacing_scoped_note_removes_obsolete_note_from_context(MemoryScope::Project).await;
}

#[tokio::test]
async fn rewind_context_includes_each_completed_session_write_once() {
    const NOTE: &str = "Remember this after rewinding.";

    let tempdir = tempfile::tempdir().expect("tempdir");
    let project_root = tempfile::tempdir().expect("project root");
    let backends = MemoryToolBackends::new(
        &tempdir.path().abs(),
        /*global_enabled*/ false,
        /*scoped_enabled*/ true,
        "thread-1",
        &project_root.path().abs(),
    );
    let response = backends
        .write_note(
            MemoryScope::Session,
            "Rewind reminder".to_string(),
            NOTE.to_string(),
        )
        .await
        .expect("write session note");
    let mutation = MemoryMutation::write(
        "mutation-1".to_string(),
        MemoryMutationScope::Session,
        Some("Rewind reminder".to_string()),
        NOTE,
    )
    .with_status(MemoryMutationStatus::Succeeded)
    .with_path(response.path.clone());
    let completed_items = vec![
        TurnItem::Extension(ExtensionItem::MemoryMutation(mutation.clone())),
        TurnItem::Extension(ExtensionItem::MemoryMutation(mutation)),
        TurnItem::Extension(ExtensionItem::MemoryMutation(
            MemoryMutation::write(
                "project-mutation".to_string(),
                MemoryMutationScope::Project,
                Some("Project reminder".to_string()),
                NOTE,
            )
            .with_status(MemoryMutationStatus::Succeeded)
            .with_path(response.path.clone()),
        )),
        TurnItem::Extension(ExtensionItem::MemoryMutation(
            MemoryMutation::write(
                "failed-mutation".to_string(),
                MemoryMutationScope::Session,
                Some("Failed reminder".to_string()),
                NOTE,
            )
            .with_status(MemoryMutationStatus::Failed)
            .with_path(response.path.clone()),
        )),
    ];

    let context = backends
        .rewind_session_context_fragment(&completed_items)
        .await
        .expect("successful session write should contribute context");
    assert_eq!(context.matches(NOTE).count(), 1);

    backends
        .delete(
            Some(MemoryScope::Session),
            crate::backend::DeleteMemoryRequest {
                path: response.path,
            },
        )
        .await
        .expect("delete session note");
    assert_eq!(
        backends
            .rewind_session_context_fragment(&completed_items)
            .await,
        None
    );
}

#[tokio::test]
async fn rewind_session_context_fragment_respects_model_visible_token_limit() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let project_root = tempfile::tempdir().expect("project root");
    let backends = MemoryToolBackends::new(
        &tempdir.path().abs(),
        /*global_enabled*/ false,
        /*scoped_enabled*/ true,
        "thread-1",
        &project_root.path().abs(),
    );
    let oversized_note = "memory ".repeat(SESSION_CONTEXT_TOKEN_LIMIT);
    let session_response = backends
        .write_note(
            MemoryScope::Session,
            "Oversized session memory".to_string(),
            oversized_note.clone(),
        )
        .await
        .expect("write oversized session note");

    let completed_items = vec![TurnItem::Extension(ExtensionItem::MemoryMutation(
        MemoryMutation::write(
            "mutation-1".to_string(),
            MemoryMutationScope::Session,
            Some("Oversized session memory".to_string()),
            &oversized_note,
        )
        .with_status(MemoryMutationStatus::Succeeded)
        .with_path(session_response.path),
    ))];
    let rewind_context = backends
        .rewind_session_context_fragment(&completed_items)
        .await
        .expect("oversized rewind note should contribute context");
    assert!(
        approx_token_count(&rewind_context) <= SESSION_CONTEXT_TOKEN_LIMIT,
        "rewind context used {} approximate tokens",
        approx_token_count(&rewind_context)
    );
}

async fn assert_replacing_scoped_note_removes_obsolete_note_from_context(scope: MemoryScope) {
    const OBSOLETE_NOTE: &str = "Forget this after the delete tool runs.";
    const REPLACEMENT_NOTE: &str = "Keep this corrected preference.";

    let tempdir = tempfile::tempdir().expect("tempdir");
    let project_root = tempfile::tempdir().expect("project root");
    let backends = MemoryToolBackends::new(
        &tempdir.path().abs(),
        /*global_enabled*/ false,
        /*scoped_enabled*/ true,
        "thread-1",
        &project_root.path().abs(),
    );
    let write_tool = memory_tool_from_backends(backends.clone(), crate::WRITE_NOTE_TOOL_NAME);
    let write_payload = ToolPayload::Function {
        arguments: json!({
            "scope": scope.as_str(),
            "title": "Temporary Preference",
            "note": OBSOLETE_NOTE,
        })
        .to_string(),
    };
    let write_response = run_memory_tool(
        &write_tool,
        crate::WRITE_NOTE_TOOL_NAME,
        "call-1",
        write_payload,
    )
    .await
    .expect("write should succeed")
    .expect("write response");
    let path = write_response
        .pointer("/path")
        .and_then(serde_json::Value::as_str)
        .expect("write response path");
    assert!(
        backends
            .scoped_context_fragment()
            .await
            .is_some_and(|context| context.contains(OBSOLETE_NOTE))
    );

    let replacement_payload = ToolPayload::Function {
        arguments: json!({
            "scope": scope.as_str(),
            "title": "Corrected Preference",
            "note": REPLACEMENT_NOTE,
        })
        .to_string(),
    };
    run_memory_tool(
        &write_tool,
        crate::WRITE_NOTE_TOOL_NAME,
        "call-2",
        replacement_payload,
    )
    .await
    .expect("replacement write should succeed");

    let delete_tool = memory_tool_from_backends(backends.clone(), crate::DELETE_TOOL_NAME);
    let delete_payload = ToolPayload::Function {
        arguments: json!({
            "scope": scope.as_str(),
            "path": path,
        })
        .to_string(),
    };
    let delete_response = run_memory_tool(
        &delete_tool,
        crate::DELETE_TOOL_NAME,
        "call-3",
        delete_payload,
    )
    .await
    .expect("delete should succeed");

    assert_eq!(
        delete_response,
        Some(json!({
            "scope": scope.as_str(),
            "path": path,
            "deleted": true
        }))
    );
    let context = backends
        .scoped_context_fragment()
        .await
        .expect("replacement note should remain in context");
    assert!(!context.contains(OBSOLETE_NOTE));
    assert!(context.contains(REPLACEMENT_NOTE));
}

#[tokio::test]
async fn delete_tool_rejects_directories_and_keeps_them() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let memory_root = tempdir.path().join("memories");
    tokio::fs::create_dir_all(memory_root.join("notes"))
        .await
        .expect("create memories dir");
    let delete_tool = memory_tool(&memory_root, crate::DELETE_TOOL_NAME);
    let payload = ToolPayload::Function {
        arguments: json!({
            "path": "notes",
        })
        .to_string(),
    };

    let err = run_memory_tool(&delete_tool, crate::DELETE_TOOL_NAME, "call-1", payload)
        .await
        .expect_err("directory delete should fail");

    assert!(err.to_string().contains("not a file"));
    assert!(
        tokio::fs::metadata(memory_root.join("notes"))
            .await
            .expect("directory should remain")
            .is_dir()
    );
}

#[tokio::test]
async fn delete_tool_rejects_path_traversal() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let memory_root = tempdir.path().join("memories");
    tokio::fs::create_dir_all(&memory_root)
        .await
        .expect("create memories dir");
    let delete_tool = memory_tool(&memory_root, crate::DELETE_TOOL_NAME);
    let payload = ToolPayload::Function {
        arguments: json!({
            "path": "../outside.md",
        })
        .to_string(),
    };

    let err = run_memory_tool(&delete_tool, crate::DELETE_TOOL_NAME, "call-1", payload)
        .await
        .expect_err("path traversal should fail");

    assert!(err.to_string().contains("memories root"));
}

#[tokio::test]
async fn read_tool_requires_scope_when_global_memories_are_disabled() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let project_root = tempfile::tempdir().expect("project root");
    let backends = MemoryToolBackends::new(
        &tempdir.path().abs(),
        /*global_enabled*/ false,
        /*scoped_enabled*/ true,
        "thread-1",
        &project_root.path().abs(),
    );
    let read_tool = memory_tool_from_backends(backends, crate::READ_TOOL_NAME);
    let payload = ToolPayload::Function {
        arguments: json!({
            "path": "MEMORY.md",
        })
        .to_string(),
    };

    let result = read_tool
        .handle(ToolCall {
            turn_id: "turn-1".to_string(),
            call_id: "call-1".to_string(),
            tool_name: memory_tool_name(crate::READ_TOOL_NAME),
            model: "gpt-test".to_string(),
            codex_turn_metadata: None,
            truncation_policy: TruncationPolicy::Bytes(1024),
            conversation_history: codex_extension_api::ConversationHistory::default(),
            turn_item_emitter: Arc::new(NoopTurnItemEmitter),
            environments: Vec::new(),
            payload,
        })
        .await;

    let err = match result {
        Ok(_) => panic!("global-disabled read without scope should fail"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("scope"));
}

#[tokio::test]
async fn add_ad_hoc_note_tool_rejects_paths_as_filenames() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let memory_root = tempdir.path().join("memories");
    let tool = memory_tool(&memory_root, crate::ADD_AD_HOC_NOTE_TOOL_NAME);
    let payload = ToolPayload::Function {
        arguments: json!({
            "filename": "../2026-05-26T13-42-08-remember-review-style.md",
            "note": "Remember to keep PR review comments concise.",
        })
        .to_string(),
    };

    let result = tool
        .handle(ToolCall {
            turn_id: "turn-1".to_string(),
            call_id: "call-1".to_string(),
            tool_name: memory_tool_name(crate::ADD_AD_HOC_NOTE_TOOL_NAME),
            model: "gpt-test".to_string(),
            codex_turn_metadata: None,
            truncation_policy: TruncationPolicy::Bytes(1024),
            conversation_history: codex_extension_api::ConversationHistory::default(),
            turn_item_emitter: Arc::new(NoopTurnItemEmitter),
            environments: Vec::new(),
            payload,
        })
        .await;
    let err = match result {
        Ok(_) => panic!("path-like filename should be rejected"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("filename"));
    assert!(err.to_string().contains("YYYY-MM-DDTHH-MM-SS"));
}

#[tokio::test]
async fn read_tool_reads_memory_file() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let memory_root = tempdir.path().join("memories");
    tokio::fs::create_dir_all(&memory_root)
        .await
        .expect("create memories dir");
    tokio::fs::write(
        memory_root.join("MEMORY.md"),
        "first line\nsecond needle line\nthird line\n",
    )
    .await
    .expect("write memory");
    let tool = memory_tool(&memory_root, crate::READ_TOOL_NAME);
    let payload = ToolPayload::Function {
        arguments: json!({
            "path": "MEMORY.md",
            "line_offset": 2,
            "max_lines": 1
        })
        .to_string(),
    };

    let output = tool
        .handle(ToolCall {
            turn_id: "turn-1".to_string(),
            call_id: "call-1".to_string(),
            tool_name: memory_tool_name(crate::READ_TOOL_NAME),
            model: "gpt-test".to_string(),
            codex_turn_metadata: None,
            truncation_policy: TruncationPolicy::Bytes(1024),
            conversation_history: codex_extension_api::ConversationHistory::default(),
            turn_item_emitter: Arc::new(NoopTurnItemEmitter),
            environments: Vec::new(),
            payload: payload.clone(),
        })
        .await
        .expect("read should succeed");

    assert_eq!(
        output.post_tool_use_response("call-1", &payload),
        Some(json!({
            "path": "MEMORY.md",
            "content": "second needle line\n",
            "start_line_number": 2,
            "truncated": true
        }))
    );
}

#[tokio::test]
async fn search_tool_accepts_multiple_queries() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let memory_root = tempdir.path().join("memories");
    tokio::fs::create_dir_all(&memory_root)
        .await
        .expect("create memories dir");
    tokio::fs::write(
        memory_root.join("MEMORY.md"),
        "alpha only\nneedle only\nalpha needle\n",
    )
    .await
    .expect("write memory");
    let tool = memory_tool(&memory_root, crate::SEARCH_TOOL_NAME);
    let payload = ToolPayload::Function {
        arguments: json!({
            "queries": ["alpha", "needle"],
            "case_sensitive": false
        })
        .to_string(),
    };

    let output = tool
        .handle(ToolCall {
            turn_id: "turn-1".to_string(),
            call_id: "call-1".to_string(),
            tool_name: memory_tool_name(crate::SEARCH_TOOL_NAME),
            model: "gpt-test".to_string(),
            codex_turn_metadata: None,
            truncation_policy: TruncationPolicy::Bytes(1024),
            conversation_history: codex_extension_api::ConversationHistory::default(),
            turn_item_emitter: Arc::new(NoopTurnItemEmitter),
            environments: Vec::new(),
            payload: payload.clone(),
        })
        .await
        .expect("search should succeed");

    assert_eq!(
        output.post_tool_use_response("call-1", &payload),
        Some(json!({
            "queries": ["alpha", "needle"],
            "match_mode": {
                "type": "any"
            },
            "path": null,
            "matches": [
                {
                    "path": "MEMORY.md",
                    "match_line_number": 1,
                    "content_start_line_number": 1,
                    "content": "alpha only",
                    "matched_queries": ["alpha"]
                },
                {
                    "path": "MEMORY.md",
                    "match_line_number": 2,
                    "content_start_line_number": 2,
                    "content": "needle only",
                    "matched_queries": ["needle"]
                },
                {
                    "path": "MEMORY.md",
                    "match_line_number": 3,
                    "content_start_line_number": 3,
                    "content": "alpha needle",
                    "matched_queries": ["alpha", "needle"]
                }
            ],
            "next_cursor": null,
            "truncated": false
        }))
    );
}

#[tokio::test]
async fn search_tool_accepts_windowed_all_match_mode() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let memory_root = tempdir.path().join("memories");
    tokio::fs::create_dir_all(&memory_root)
        .await
        .expect("create memories dir");
    tokio::fs::write(memory_root.join("MEMORY.md"), "alpha\nmiddle\nneedle\n")
        .await
        .expect("write memory");
    let tool = memory_tool(&memory_root, crate::SEARCH_TOOL_NAME);
    let payload = ToolPayload::Function {
        arguments: json!({
            "queries": ["alpha", "needle"],
            "match_mode": {
                "type": "all_within_lines",
                "line_count": 3
            }
        })
        .to_string(),
    };

    let output = tool
        .handle(ToolCall {
            turn_id: "turn-1".to_string(),
            call_id: "call-1".to_string(),
            tool_name: memory_tool_name(crate::SEARCH_TOOL_NAME),
            model: "gpt-test".to_string(),
            codex_turn_metadata: None,
            truncation_policy: TruncationPolicy::Bytes(1024),
            conversation_history: codex_extension_api::ConversationHistory::default(),
            turn_item_emitter: Arc::new(NoopTurnItemEmitter),
            environments: Vec::new(),
            payload: payload.clone(),
        })
        .await
        .expect("search should succeed");

    assert_eq!(
        output.post_tool_use_response("call-1", &payload),
        Some(json!({
            "queries": ["alpha", "needle"],
            "match_mode": {
                "type": "all_within_lines",
                "line_count": 3
            },
            "path": null,
            "matches": [
                {
                    "path": "MEMORY.md",
                    "match_line_number": 1,
                    "content_start_line_number": 1,
                    "content": "alpha\nmiddle\nneedle",
                    "matched_queries": ["alpha", "needle"]
                }
            ],
            "next_cursor": null,
            "truncated": false
        }))
    );
}

#[tokio::test]
async fn search_tool_rejects_legacy_single_query() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let memory_root = tempdir.path().join("memories");
    tokio::fs::create_dir_all(&memory_root)
        .await
        .expect("create memories dir");
    let tool = memory_tool(&memory_root, crate::SEARCH_TOOL_NAME);
    let payload = ToolPayload::Function {
        arguments: json!({
            "query": "needle",
        })
        .to_string(),
    };

    let result = tool
        .handle(ToolCall {
            turn_id: "turn-1".to_string(),
            call_id: "call-1".to_string(),
            tool_name: memory_tool_name(crate::SEARCH_TOOL_NAME),
            model: "gpt-test".to_string(),
            codex_turn_metadata: None,
            truncation_policy: TruncationPolicy::Bytes(1024),
            conversation_history: codex_extension_api::ConversationHistory::default(),
            turn_item_emitter: Arc::new(NoopTurnItemEmitter),
            environments: Vec::new(),
            payload,
        })
        .await;
    let err = match result {
        Ok(_) => panic!("legacy query field should be rejected"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("unknown field"));
    assert!(err.to_string().contains("query"));
}

fn memory_tool(memory_root: &Path, tool_name: &str) -> Arc<dyn ToolExecutor<ToolCall>> {
    memory_tool_from_backends(
        MemoryToolBackends::from_global_memory_root(memory_root),
        tool_name,
    )
}

fn memory_tool_from_backends(
    backends: MemoryToolBackends,
    tool_name: &str,
) -> Arc<dyn ToolExecutor<ToolCall>> {
    let expected_tool_name = memory_tool_name(tool_name);
    crate::tools::memory_tools(backends, /*metrics_client*/ None)
        .into_iter()
        .find(|tool| tool.tool_name() == expected_tool_name)
        .unwrap_or_else(|| panic!("{tool_name} tool should be registered"))
}

async fn run_memory_tool(
    tool: &Arc<dyn ToolExecutor<ToolCall>>,
    tool_name: &str,
    call_id: &str,
    payload: ToolPayload,
) -> Result<Option<serde_json::Value>, codex_extension_api::FunctionCallError> {
    run_memory_tool_with_emitter(
        tool,
        tool_name,
        call_id,
        payload,
        Arc::new(NoopTurnItemEmitter),
    )
    .await
}

async fn run_memory_tool_with_emitter(
    tool: &Arc<dyn ToolExecutor<ToolCall>>,
    tool_name: &str,
    call_id: &str,
    payload: ToolPayload,
    turn_item_emitter: Arc<dyn TurnItemEmitter>,
) -> Result<Option<serde_json::Value>, codex_extension_api::FunctionCallError> {
    let output = tool
        .handle(ToolCall {
            turn_id: "turn-1".to_string(),
            call_id: call_id.to_string(),
            tool_name: memory_tool_name(tool_name),
            model: "gpt-test".to_string(),
            codex_turn_metadata: None,
            truncation_policy: TruncationPolicy::Bytes(1024),
            conversation_history: codex_extension_api::ConversationHistory::default(),
            turn_item_emitter,
            environments: Vec::new(),
            payload: payload.clone(),
        })
        .await?;

    Ok(output.post_tool_use_response(call_id, &payload))
}

#[derive(Default)]
struct RecordingTurnItemEmitter {
    started: Mutex<Vec<ExtensionTurnItem>>,
    completed: Mutex<Vec<ExtensionTurnItem>>,
}

impl RecordingTurnItemEmitter {
    fn started(&self) -> Vec<ExtensionTurnItem> {
        self.started.lock().expect("started item lock").clone()
    }

    fn completed(&self) -> Vec<ExtensionTurnItem> {
        self.completed.lock().expect("completed item lock").clone()
    }
}

impl TurnItemEmitter for RecordingTurnItemEmitter {
    fn emit_started<'a>(&'a self, item: ExtensionTurnItem) -> TurnItemEmissionFuture<'a> {
        Box::pin(async move {
            self.started.lock().expect("started item lock").push(item);
        })
    }

    fn emit_completed<'a>(&'a self, item: ExtensionTurnItem) -> TurnItemEmissionFuture<'a> {
        Box::pin(async move {
            self.completed
                .lock()
                .expect("completed item lock")
                .push(item);
        })
    }
}

fn mutation_value(item: &ExtensionTurnItem) -> serde_json::Value {
    let ExtensionItem::MemoryMutation(_) = &item.item else {
        panic!("memory tools should only emit memory mutation items");
    };
    serde_json::to_value(&item.item).expect("serialize mutation item")
}

fn memory_tool_name(tool_name: &str) -> ToolName {
    ToolName::namespaced(crate::MEMORY_TOOLS_NAMESPACE, tool_name)
}
