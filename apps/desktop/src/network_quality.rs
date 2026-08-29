//! Live Network Quality label — REQ-PICOO-UI-004 / PRD §16.
//!
//! Thresholds aligned with Android `LinkQuality.label` (PUC-005).

#![cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]

/// Coarse Wi-Fi / link quality from packet loss + RTT (or latency proxy).
pub fn network_quality_label(packet_loss: f64, rtt_or_latency_ms: f64) -> &'static str {
    if packet_loss >= 0.05 || rtt_or_latency_ms >= 120.0 {
        "较差 (Poor)"
    } else if packet_loss >= 0.02 || rtt_or_latency_ms >= 60.0 {
        "一般 (Fair)"
    } else if packet_loss > 0.0 || rtt_or_latency_ms >= 30.0 {
        "良好 (Good)"
    } else {
        "极佳 (Excellent)"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_match_android_link_quality_thresholds() {
        assert_eq!(network_quality_label(0.0, 10.0), "极佳 (Excellent)");
        assert_eq!(network_quality_label(0.01, 40.0), "良好 (Good)");
        assert_eq!(network_quality_label(0.03, 20.0), "一般 (Fair)");
        assert_eq!(network_quality_label(0.06, 20.0), "较差 (Poor)");
        assert_eq!(network_quality_label(0.0, 150.0), "较差 (Poor)");
    }
}
