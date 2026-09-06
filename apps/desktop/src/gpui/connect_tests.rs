const CONNECT_SOURCE: &str = include_str!("connect.rs");

#[test]
fn desktop_disconnect_is_bound_to_the_owner_command_without_a_dialog() {
    let button = CONNECT_SOURCE
        .lines()
        .skip_while(|line| !line.contains("Button::new(\"disconnect-active-device\")"))
        .take(32)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!button.is_empty(), "Live disconnect button");
    assert!(button.contains("this.disconnect_active_sender(cx)"));
    assert!(!button.contains("open_disconnect_dialog"));
}

#[test]
fn live_preview_top_inset_does_not_depend_on_sidebar_state() {
    let preview_stage = CONNECT_SOURCE
        .lines()
        .skip_while(|line| !line.contains("the page-level p_4 already"))
        .take(12)
        .collect::<Vec<_>>()
        .join("\n");

    assert!(preview_stage.contains(".pt_4()"));
    assert!(!preview_stage.contains("sidebar_collapsed"));
    assert!(!preview_stage.contains(".p_4()"));
}
