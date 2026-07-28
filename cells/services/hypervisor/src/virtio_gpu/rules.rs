//! Pure resource-table admission rules.

pub fn valid_new_resource_id(resource_id: u32, already_exists: bool) -> bool {
    resource_id != 0 && !already_exists
}
