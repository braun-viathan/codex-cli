use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::signal::unix::SignalKind;
use tokio::signal::unix::signal;

use super::UpdateLoopControl;
use super::update_modes_for_identities;
use super::update_once_for_daemon;
use crate::Daemon;
use crate::RestartMode;
use crate::UpdaterRefreshMode;
use crate::managed_install::executable_identity_from_bytes;
use crate::settings::DaemonSettings;

#[test]
fn unchanged_updater_uses_version_based_restart() {
    assert_eq!(
        update_modes_for_identities(
            &executable_identity_from_bytes(b"same"),
            &executable_identity_from_bytes(b"same"),
        ),
        (RestartMode::IfVersionChanged, UpdaterRefreshMode::None)
    );
}

#[test]
fn changed_updater_forces_refresh_even_when_version_may_match() {
    assert_eq!(
        update_modes_for_identities(
            &executable_identity_from_bytes(b"old"),
            &executable_identity_from_bytes(b"new"),
        ),
        (
            RestartMode::Always,
            UpdaterRefreshMode::ReexecIfManagedBinaryChanged,
        )
    );
}

#[tokio::test]
async fn custom_binary_mode_stops_before_fetching_standalone_update() {
    let temp_dir = TempDir::new().expect("temp dir");
    let daemon = Daemon {
        socket_path: temp_dir.path().join("app-server-control.sock"),
        pid_file: temp_dir.path().join("app-server.pid"),
        update_pid_file: temp_dir.path().join("app-server-updater.pid"),
        operation_lock_file: temp_dir.path().join("daemon.lock"),
        settings_file: temp_dir.path().join("settings.json"),
        managed_codex_bin: temp_dir.path().join("managed-codex"),
    };
    DaemonSettings {
        remote_control_enabled: true,
        custom_codex_bin: Some(temp_dir.path().join("custom-codex")),
    }
    .save(&daemon.settings_file)
    .await
    .expect("save custom settings");

    let running_updater_identity = executable_identity_from_bytes(b"updater");
    let mut terminate = signal(SignalKind::terminate()).expect("install signal handler");

    assert_eq!(
        update_once_for_daemon(&daemon, &running_updater_identity, &mut terminate)
            .await
            .expect("custom mode stops updater"),
        UpdateLoopControl::Stop
    );
}
