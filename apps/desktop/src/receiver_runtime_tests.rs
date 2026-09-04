use super::{
    distinguishable_fingerprint_prefix, format_last_connected_ms,
    format_last_connected_relative_ms, reset_receiver_trust_at, sanitize_receiver_stats,
    TrustedDeviceSummary,
};

#[test]
fn explicit_pairing_reset_replaces_a_corrupt_store() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "picoo-receiver-trust-reset-{}-{nonce}.json",
        std::process::id()
    ));
    std::fs::write(&path, b"corrupt trust store").expect("corrupt fixture");

    reset_receiver_trust_at(&path).expect("reset trust");

    let store =
        picoo_pairing::TrustedDeviceStore::load_from_path(&path).expect("reload reset trust store");
    assert!(store.is_empty());
    std::fs::remove_file(path).expect("cleanup fixture");
}

#[test]
fn format_last_connected_utc_date_or_dash() {
    assert_eq!(format_last_connected_ms(0), "—");
    assert_eq!(format_last_connected_ms(1_577_836_800_000), "2020-01-01");
}

#[test]
fn format_last_connected_relative_age_is_compact_and_clock_skew_safe() {
    const DAY_MS: u64 = 86_400_000;
    let now = 2_000_000_000_000;
    assert_eq!(format_last_connected_relative_ms(0, now), "时间未知");
    assert_eq!(format_last_connected_relative_ms(now, now), "今天");
    assert_eq!(format_last_connected_relative_ms(now - DAY_MS, now), "昨天");
    assert_eq!(
        format_last_connected_relative_ms(now - 12 * DAY_MS, now),
        "12 天前"
    );
    assert_eq!(format_last_connected_relative_ms(now + DAY_MS, now), "今天");
}

#[test]
fn receiver_stats_reject_non_finite_windows() {
    assert!(sanitize_receiver_stats(&picoo_metrics::ReceiverStats {
        rtt_ms: f64::NAN,
        ..Default::default()
    })
    .is_none());
}

#[test]
fn receiver_stats_clamp_finite_presentation_values() {
    let sanitized = sanitize_receiver_stats(&picoo_metrics::ReceiverStats {
        packet_loss: 2.0,
        jitter_ms: -3.0,
        jitter_buffer_target_ms: -1.0,
        jitter_buffer_actual_delay_ms: -2.0,
        jitter_buffer_occupancy_ms: -3.0,
        ..Default::default()
    })
    .expect("finite window remains present");
    assert_eq!(sanitized.rtt_ms, 0.0);
    assert_eq!(sanitized.packet_loss, 1.0);
    assert_eq!(sanitized.jitter_ms, 0.0);
    assert_eq!(sanitized.frame_age_ms, 0.0);
    assert_eq!(sanitized.jitter_buffer_target_ms, 0.0);
    assert_eq!(sanitized.jitter_buffer_actual_delay_ms, 0.0);
    assert_eq!(sanitized.jitter_buffer_occupancy_ms, 0.0);
}

#[test]
fn receiver_stats_drop_invalid_optional_timeline_values() {
    let sanitized = sanitize_receiver_stats(&picoo_metrics::ReceiverStats {
        capture_to_encode_ms: Some(-1.0),
        decode_ms: Some(f64::NAN),
        end_to_end_latency_ms: Some(42.0),
        ..Default::default()
    })
    .expect("core window remains valid");

    assert_eq!(sanitized.capture_to_encode_ms, Some(0.0));
    assert_eq!(sanitized.decode_ms, None);
    assert_eq!(sanitized.end_to_end_latency_ms, Some(42.0));
}

#[test]
fn fingerprint_prefix_is_eight_hex_and_expands_on_collision() {
    let devices = vec![
        TrustedDeviceSummary {
            device_id: "a".into(),
            device_name: "Pixel".into(),
            certificate_fingerprint: "12345678aaaabbbb".into(),
            identity_prefix: String::new(),
            last_connected_at_ms: 0,
            platform: "Android",
        },
        TrustedDeviceSummary {
            device_id: "b".into(),
            device_name: "Pixel".into(),
            certificate_fingerprint: "12345678ccccdddd".into(),
            identity_prefix: String::new(),
            last_connected_at_ms: 0,
            platform: "Android",
        },
    ];
    assert_eq!(
        distinguishable_fingerprint_prefix(&devices[0], &devices),
        "12345678aaaa"
    );
    let unique = TrustedDeviceSummary {
        device_id: "c".into(),
        device_name: "iPhone".into(),
        certificate_fingerprint: "abcdef0123456789".into(),
        identity_prefix: String::new(),
        last_connected_at_ms: 0,
        platform: "iOS",
    };
    assert_eq!(
        distinguishable_fingerprint_prefix(&unique, &devices),
        "abcdef01"
    );
}
