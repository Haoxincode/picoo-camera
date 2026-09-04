use anyhow::{bail, Result};
use xshell::{cmd, Shell};

use crate::TestSuite;

pub(crate) fn run(suite: TestSuite) -> Result<()> {
    let sh = Shell::new()?;
    match suite {
        TestSuite::Ios => crate::apple::ios::test_ios(&sh)?,
        TestSuite::Macos => crate::apple::macos::test_macos(&sh)?,
        TestSuite::Windows => {
            if !cfg!(target_os = "windows") {
                bail!("Windows tests must run on a Windows host");
            }
            cmd!(
                sh,
                "cargo clippy -p picoo-frame-hub -p picoo-windows-vcam-source --all-targets -- -D warnings"
            )
            .run()?;
            cmd!(
                sh,
                "cargo clippy -p picoo-desktop --all-targets --features gpui-ui,windows-vcam -- -D warnings"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-frame-hub -p picoo-windows-vcam-source"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-desktop --features gpui-ui,windows-vcam"
            )
            .run()?;
            cmd!(sh, "cargo test -p picoo-media-decode --features windows-mf").run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --features windows-mf --lib paired_avcc_length_prefixed_au_reaches_frame_hub"
            )
            .run()?;
        }
        TestSuite::Protocol => {
            cmd!(
                sh,
                "cargo test -p picoo-protocol -p picoo-packet -p picoo-transport -p picoo-testkit -p picoo-sim"
            )
            .run()?;
        }
        TestSuite::Linux => {
            // REQ-PICOO-VCAM-004 / DISCOVERY-005 / SESSION-005..007 / PAIRING-003 — no Win11 GUI.
            cmd!(sh, "bash scripts/validate_wix_scaffold.sh").run()?;
            cmd!(sh, "cargo test -p picoo-windows-vcam-source").run()?;
            cmd!(sh, "bash scripts/check_discovery_txt_keys.sh").run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib soak_harness_smoke_five_seconds"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib unpaired_video_keeps_shared_ring_on_placeholder"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib paired_loopback_remains_usable_under_five_percent_loss"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib paired_loopback_e2e_latency_p50_under_budget"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib paired_openh264_remains_usable_under_five_percent_loss"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib paired_openh264_e2e_latency_p50_under_budget"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-protocol --lib control_envelope_tests"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib capabilities_720_only_are_applied_before_sender_stream_config"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib manual_endpoint_connects_to_streaming"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib reconnect_churn_smoke_five_rounds"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib reconnect_churn_fifteen_rounds"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib public_key_change_rejects_auto_connect"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib unpaired_start_stream_is_rejected"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib unpaired_stop_stream_is_ignored_without_teardown"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib paired_start_stop_stream_and_camera_command_roundtrip"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-discovery --lib synthetic_advertise_to_list_p50_under_two_seconds"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-ffi --lib export_diagnostics_with_session_includes_redacted_host"
            )
            .run()?;
        }
    }
    Ok(())
}
