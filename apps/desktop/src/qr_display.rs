//! Terminal QR rendering for --serve mode — PUC-003 Show QR Code (CLI fallback).

use qrcode::QrCode;

pub fn render_qr_ascii(payload: &str) -> Result<String, String> {
    let code = QrCode::new(payload.as_bytes()).map_err(|e| e.to_string())?;
    Ok(code
        .render::<char>()
        .quiet_zone(true)
        .module_dimensions(1, 1)
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_non_empty_ascii_qr() {
        let art = render_qr_ascii("{\"v\":1}").expect("qr");
        assert!(art.contains('█') || art.contains('#'));
        assert!(art.lines().count() > 5);
    }
}
