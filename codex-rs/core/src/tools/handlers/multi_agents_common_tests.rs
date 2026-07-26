use super::*;
use codex_network_proxy::NetworkProxyConfig;
use codex_protocol::models::ActivePermissionProfile;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::NetworkSandboxPolicy;
use pretty_assertions::assert_eq;
use std::sync::Arc;

fn enabled_network_profile() -> PermissionProfile {
    PermissionProfile::workspace_write_with(
        &[],
        NetworkSandboxPolicy::Enabled,
        /*exclude_tmpdir_env_var*/ false,
        /*exclude_slash_tmp*/ false,
    )
}

fn proxy_spec_for_domain(domain: &str) -> NetworkProxySpec {
    let mut config = NetworkProxyConfig {
        enabled: true,
        ..Default::default()
    };
    config.set_allowed_domains(vec![domain.to_string()]);
    NetworkProxySpec::from_config_and_constraints(
        config,
        /*requirements*/ None,
        &enabled_network_profile(),
    )
    .expect("network proxy spec should be valid")
}

#[test]
fn network_proxy_meet_rejects_incompatible_restrictions() {
    let parent = proxy_spec_for_domain("parent.example");
    let role = proxy_spec_for_domain("role.example");

    let error = meet_network_proxy_specs(Some(&parent), Some(&role), &enabled_network_profile())
        .expect_err("different proxy restrictions must fail closed");

    assert_eq!(
        error,
        "parent and role network proxy policies cannot be safely intersected"
    );
}

#[test]
fn network_proxy_meet_keeps_the_only_restrictive_proxy() {
    let parent = proxy_spec_for_domain("parent.example");
    let permission_profile = enabled_network_profile();

    let actual = meet_network_proxy_specs(Some(&parent), None, &permission_profile)
        .expect("a single restrictive proxy should be retained");

    assert_eq!(
        actual,
        Some(
            parent
                .recompute_for_permission_profile(&permission_profile)
                .expect("proxy should recompute")
        )
    );
}

#[tokio::test]
async fn runtime_permission_reapply_restores_parent_workspace_roots_and_identity() {
    let mut config = crate::config::test_config().await;
    let parent_root =
        AbsolutePathBuf::from_absolute_path("/parent-root").expect("parent root is absolute");
    let role_root =
        AbsolutePathBuf::from_absolute_path("/role-root").expect("role root is absolute");
    let profile_root =
        AbsolutePathBuf::from_absolute_path("/profile-root").expect("profile root is absolute");
    let active_profile = ActivePermissionProfile::new("parent-profile");
    let canonical_profile = config.permissions.permission_profile().clone();
    config
        .permissions
        .set_permission_profile_from_session_snapshot(
            PermissionProfileSnapshot::active_with_profile_workspace_roots(
                canonical_profile,
                active_profile.clone(),
                vec![profile_root.clone()],
            ),
        )
        .expect("parent profile should be accepted");
    config.workspace_roots = vec![parent_root.clone()];
    config.workspace_roots_explicit = true;
    config
        .permissions
        .set_workspace_roots(vec![parent_root.clone()]);
    let expected_effective_profile = config.permissions.effective_permission_profile();
    let parent_approval_policy = config.permissions.approval_policy.value();
    let parent_approvals_reviewer = config.approvals_reviewer;
    let parent_cwd = config.cwd.clone();
    let parent_baseline = runtime_permission_baseline(&config);

    config.workspace_roots = vec![role_root.clone()];
    config.workspace_roots_explicit = false;
    config.permissions.set_workspace_roots(vec![role_root]);

    reapply_runtime_permissions_after_role(
        &mut config,
        parent_approval_policy,
        parent_approvals_reviewer,
        parent_cwd,
        parent_baseline,
    )
    .expect("parent runtime boundary should reapply");

    assert_eq!(config.workspace_roots, vec![parent_root.clone()]);
    assert!(config.workspace_roots_explicit);
    assert_eq!(config.permissions.workspace_roots(), &[parent_root]);
    assert_eq!(
        config.permissions.profile_workspace_roots(),
        &[profile_root]
    );
    assert_eq!(
        config.permissions.active_permission_profile(),
        Some(active_profile)
    );
    assert_eq!(
        config.permissions.effective_permission_profile(),
        expected_effective_profile
    );
}

#[tokio::test]
async fn spawn_config_preserves_matching_active_profile_identity() {
    let (_session, mut turn) = crate::session::tests::make_session_and_context().await;
    let mut config = (*turn.config).clone();
    let workspace_root =
        AbsolutePathBuf::from_absolute_path("/runtime-root").expect("runtime root is absolute");
    let profile_root =
        AbsolutePathBuf::from_absolute_path("/profile-root").expect("profile root is absolute");
    let active_profile = ActivePermissionProfile::new("parent-profile");
    config.workspace_roots = vec![workspace_root.clone()];
    config
        .permissions
        .set_workspace_roots(vec![workspace_root.clone()]);
    config
        .permissions
        .set_permission_profile_from_session_snapshot(
            PermissionProfileSnapshot::active_with_profile_workspace_roots(
                config.permissions.permission_profile().clone(),
                active_profile.clone(),
                vec![profile_root.clone()],
            ),
        )
        .expect("active profile should be accepted");
    turn.permission_profile = config.permissions.effective_permission_profile();
    turn.config = Arc::new(config);

    let child_config = build_agent_spawn_config(
        &BaseInstructions {
            text: "base".to_string(),
        },
        &turn,
    )
    .expect("spawn config should build");

    assert_eq!(
        child_config.permissions.active_permission_profile(),
        Some(active_profile)
    );
    assert_eq!(
        child_config.permissions.workspace_roots(),
        &[workspace_root]
    );
    assert_eq!(
        child_config.permissions.profile_workspace_roots(),
        &[profile_root]
    );
    assert_eq!(
        child_config.permissions.effective_permission_profile(),
        turn.permission_profile
    );
}
