use anyhow::Result;
use codex_features::Feature;
use codex_protocol::models::PermissionProfile;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call_with_namespace;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once_match;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;

const FIRST_PROMPT: &str = "spawn the first worker";
const FIRST_TASK: &str = "first worker task";
const SECOND_TASK: &str = "second worker task";
const MULTI_AGENT_V2_NAMESPACE: &str = "collaboration";
const NARROWING_ROLE: &str = "auditor";
const NARROWING_TASK_NAME: &str = "auditor_task";

fn body_contains(request: &wiremock::Request, text: &str) -> bool {
    serde_json::from_slice::<serde_json::Value>(&request.body)
        .is_ok_and(|body| body.to_string().contains(text))
}

fn has_function_call_output(request: &wiremock::Request, call_id: &str) -> bool {
    serde_json::from_slice::<serde_json::Value>(&request.body).is_ok_and(|body| {
        body.get("input")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|item| {
                    item.get("type").and_then(serde_json::Value::as_str)
                        == Some("function_call_output")
                        && item.get("call_id").and_then(serde_json::Value::as_str) == Some(call_id)
                })
            })
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn v2_nested_spawn_checks_shared_active_execution_capacity() -> Result<()> {
    let server = start_mock_server().await;
    let first_args = serde_json::to_string(&json!({
        "message": FIRST_TASK,
        "task_name": "first",
    }))?;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| body_contains(request, FIRST_PROMPT),
        sse(vec![
            ev_response_created("first-response"),
            ev_function_call_with_namespace(
                "first-call",
                MULTI_AGENT_V2_NAMESPACE,
                "spawn_agent",
                &first_args,
            ),
            ev_completed("first-response"),
        ]),
    )
    .await;
    let second_args = serde_json::to_string(&json!({
        "message": SECOND_TASK,
        "task_name": "second",
    }))?;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            body_contains(request, FIRST_TASK) && !has_function_call_output(request, "first-call")
        },
        sse(vec![
            ev_response_created("first-worker-response"),
            ev_function_call_with_namespace(
                "second-call",
                MULTI_AGENT_V2_NAMESPACE,
                "spawn_agent",
                &second_args,
            ),
            ev_completed("first-worker-response"),
        ]),
    )
    .await;
    let second_followup = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| has_function_call_output(request, "second-call"),
        sse(vec![
            ev_response_created("second-followup-response"),
            ev_assistant_message("second-followup-message", "blocked"),
            ev_completed("second-followup-response"),
        ]),
    )
    .await;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| has_function_call_output(request, "first-call"),
        sse(vec![
            ev_response_created("first-followup-response"),
            ev_assistant_message("first-followup-message", "spawned"),
            ev_completed("first-followup-response"),
        ]),
    )
    .await;

    let mut builder = test_codex().with_model("koffing").with_config(|config| {
        config
            .features
            .enable(Feature::Collab)
            .expect("test config should allow feature update");
        config
            .features
            .enable(Feature::MultiAgentV2)
            .expect("test config should allow feature update");
        config.multi_agent_v2.max_concurrent_threads_per_session = 2;
    });
    let test = builder.build(&server).await?;
    test.submit_turn(FIRST_PROMPT).await?;

    let second_output = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(output) = second_followup.function_call_output_text("second-call") {
                return output;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert_eq!(
        second_output,
        "collab spawn failed: agent thread limit reached"
    );
    assert_eq!(test.thread_manager.list_thread_ids().await.len(), 2);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn v2_spawn_role_narrows_permissions() -> Result<()> {
    let server = start_mock_server().await;
    let spawn_args = serde_json::to_string(&json!({
        "message": "inspect permissions",
        "task_name": NARROWING_TASK_NAME,
        "agent_type": NARROWING_ROLE,
        "fork_turns": "none",
    }))?;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| body_contains(request, "start permission narrowing test"),
        sse(vec![
            ev_response_created("spawn-response"),
            ev_function_call_with_namespace(
                "narrowing-call",
                MULTI_AGENT_V2_NAMESPACE,
                "spawn_agent",
                &spawn_args,
            ),
            ev_completed("spawn-response"),
        ]),
    )
    .await;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            body_contains(request, "inspect permissions")
                && !has_function_call_output(request, "narrowing-call")
        },
        sse(vec![
            ev_response_created("child-response"),
            ev_assistant_message("child-message", "inspection started"),
            ev_completed("child-response"),
        ]),
    )
    .await;
    let followup = mount_sse_once_match(
        &server,
        |request: &wiremock::Request| has_function_call_output(request, "narrowing-call"),
        sse(vec![
            ev_response_created("spawn-followup-response"),
            ev_assistant_message("spawn-followup-message", "spawn started"),
            ev_completed("spawn-followup-response"),
        ]),
    )
    .await;

    let mut builder = test_codex().with_model("koffing").with_config(|config| {
        let role_path = config.codex_home.join("agent-role-auditor.toml");
        std::fs::write(&role_path, "sandbox_mode = \"read-only\"\n")
            .expect("write narrow role config");
        config.agent_roles.insert(
            NARROWING_ROLE.to_string(),
            codex_core::config::AgentRoleConfig {
                description: Some("Narrowed read-only auditor role".to_string()),
                config_file: Some(role_path.to_path_buf()),
                nickname_candidates: None,
            },
        );
        config
            .features
            .enable(Feature::Collab)
            .expect("test config should allow feature update");
        config
            .features
            .enable(Feature::MultiAgentV2)
            .expect("test config should allow feature update");
        config
            .permissions
            .set_permission_profile(PermissionProfile::workspace_write())
            .expect("set broad parent permission profile");
        config.multi_agent_v2.max_concurrent_threads_per_session = 2;
    });
    let test = builder.build(&server).await?;
    let root_thread_id = test.session_configured.thread_id;
    let initial_thread_count = test.thread_manager.list_thread_ids().await.len();

    test.submit_turn("start permission narrowing test").await?;

    let child_thread_id = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(thread_id) = test
                .thread_manager
                .list_thread_ids()
                .await
                .into_iter()
                .find(|thread_id| *thread_id != root_thread_id)
            {
                return thread_id;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    let root_snapshot = test
        .thread_manager
        .get_thread(root_thread_id)
        .await?
        .config_snapshot()
        .await;
    let child_snapshot = test
        .thread_manager
        .get_thread(child_thread_id)
        .await?
        .config_snapshot()
        .await;

    assert_eq!(
        child_snapshot.permission_profile,
        PermissionProfile::read_only(),
        "role config should enforce a narrowed permission profile"
    );
    assert_ne!(
        child_snapshot.permission_profile, root_snapshot.permission_profile,
        "child must not inherit a broader parent profile"
    );
    assert!(
        followup
            .function_call_output_text("narrowing-call")
            .is_some(),
        "spawned role thread should report successful result text"
    );
    assert_eq!(
        test.thread_manager.list_thread_ids().await.len(),
        initial_thread_count + 1
    );
    Ok(())
}
