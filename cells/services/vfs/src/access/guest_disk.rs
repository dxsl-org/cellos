use crate::caller::Caller;

pub(crate) const GUEST_DISK_PATH: &str = "/mnt/sd/guest_disk.img";
const GUEST_DISK_BASENAME: &str = "guest_disk.img";
const GUEST_DISK_83_BASENAME: &str = "guest.img";
const GUEST_DISK_83_ALT_BASENAME: &str = "guestdsk.img";

fn strip_mnt_sd_prefix(path: &str) -> Option<&str> {
    if path.is_char_boundary(8) && path[..8].eq_ignore_ascii_case("/mnt/sd/") {
        Some(&path[8..])
    } else if path.is_char_boundary(7) && path[..7].eq_ignore_ascii_case("/mnt/sd") {
        let rest = &path[7..];
        if rest.is_empty() {
            Some("")
        } else {
            rest.strip_prefix('/')
        }
    } else {
        None
    }
}

fn is_sfn_alias(name: &str) -> bool {
    let Some(dot) = name.rfind('.') else {
        return false;
    };
    let (base, ext) = (&name[..dot], &name[dot + 1..]);
    if !ext.eq_ignore_ascii_case("img") || base.len() > 8 || !base.contains('~') {
        return false;
    }
    let Some(tilde) = base.find('~') else {
        return false;
    };
    let prefix = &base[..tilde];
    let suffix = &base[tilde + 1..];
    let prefix_ok = prefix.eq_ignore_ascii_case("guest")
        || prefix.eq_ignore_ascii_case("guest_")
        || prefix.eq_ignore_ascii_case("guestd");
    let suffix_ok = !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit());
    prefix_ok && suffix_ok
}

pub(crate) fn is_guest_disk_path(path: &str) -> bool {
    let Some(rel) = strip_mnt_sd_prefix(path) else {
        return false;
    };
    if rel.is_empty() || rel.contains('/') {
        return false;
    }
    rel.eq_ignore_ascii_case(GUEST_DISK_BASENAME)
        || rel.eq_ignore_ascii_case(GUEST_DISK_83_BASENAME)
        || rel.eq_ignore_ascii_case(GUEST_DISK_83_ALT_BASENAME)
        || is_sfn_alias(rel)
}

pub(crate) fn contains_guest_disk(path: &str) -> bool {
    if is_guest_disk_path(path) {
        return true;
    }
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("/mnt")
        || trimmed.eq_ignore_ascii_case("/mnt/sd")
    {
        return true;
    }
    if let Some(rel) = strip_mnt_sd_prefix(path) {
        let first_comp = rel.split('/').next().unwrap_or("");
        if !first_comp.is_empty() && is_guest_disk_path(&alloc::format!("/mnt/sd/{first_comp}")) {
            return true;
        }
    }
    false
}

pub(crate) fn live_hypervisor_matches(caller: Caller, lookup: fn(u16) -> Option<usize>) -> bool {
    if caller.cell.0 == 0 || caller.generation == 0 || caller.sender_tid == 0 {
        return false;
    }
    usize::try_from(caller.sender_tid)
        .ok()
        .and_then(|sender| {
            lookup(api::hypervisor::HYPERVISOR_SERVICE_ID).filter(|live| *live == sender)
        })
        .is_some()
}
