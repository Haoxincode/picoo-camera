use super::next_output_revision;

#[test]
fn output_revision_changes_only_for_a_new_negotiated_format() {
    assert_eq!(next_output_revision(7, false), Some(7));
    assert_eq!(next_output_revision(7, true), Some(8));
    assert_eq!(next_output_revision(u64::MAX, true), None);
}
