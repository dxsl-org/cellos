use super::super::path::FileReadPlan;
use crate::ViError;
use alloc::vec;

#[test]
fn parse_rejects_non_absolute_or_root_only_path() {
    assert_eq!(FileReadPlan::parse("etc/hosts"), Err(ViError::InvalidInput));
    assert_eq!(FileReadPlan::parse("/"), Err(ViError::InvalidInput));
    assert_eq!(
        FileReadPlan::parse("/etc/../hosts"),
        Err(ViError::InvalidInput)
    );
}

#[test]
fn parse_splits_parent_directories_and_file_name() {
    let plan = FileReadPlan::parse("/etc/cellos/cluster.cfg").expect("plan");
    assert_eq!(plan.parents, vec!["etc", "cellos"]);
    assert_eq!(plan.file_name, "cluster.cfg");
}
