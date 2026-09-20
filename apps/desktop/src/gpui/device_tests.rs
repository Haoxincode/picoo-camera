const DEVICE_SOURCE: &str = include_str!("device.rs");

#[test]
fn trusted_device_delete_runs_owner_command_without_dialog() {
    let button = DEVICE_SOURCE
        .lines()
        .skip_while(|line| !line.contains("Button::new(format!(\"remove-trusted-{}\""))
        .take(24)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!button.is_empty(), "trusted device delete button");
    assert!(button.contains("this.remove_trusted_device_request"));
    assert!(!button.contains("open_remove_trusted_dialog"));
}

#[test]
fn reset_all_pairings_runs_owner_command_without_dialog() {
    let button = DEVICE_SOURCE
        .lines()
        .skip_while(|line| !line.contains("Button::new(\"reset-all-pairings\""))
        .take(20)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!button.is_empty(), "reset all pairings button");
    assert!(button.contains("this.clear_trusted_devices_request"));
    assert!(!button.contains("open_reset_trusted_dialog"));
}
