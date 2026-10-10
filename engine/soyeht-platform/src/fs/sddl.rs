//! Reading an access list written in SDDL text, the way Windows prints a DACL.

/// Trustees that count as the owner's own: the system, the administrators and the owner
/// rights placeholders. Everything else is somebody else.
const OWN: [&str; 6] = ["SY", "S-1-5-18", "BA", "S-1-5-32-544", "OW", "CO"];

/// Whether every entry that allows access in `sddl` names `user` or one of the trustees that
/// count as the owner's own. A missing access list allows everybody, and so does a null one
/// (`D:NO_ACCESS_CONTROL`); only an empty list (`D:P`) allows nobody.
pub(super) fn allows_only_owner(sddl: &str, user: &str) -> bool {
    let Some(start) = sddl.find("D:") else {
        return false;
    };
    let dacl = &sddl[start + 2..];
    if dacl.contains("NO_ACCESS_CONTROL") {
        return false;
    }
    // The access list ends where a system access list starts, if one follows it.
    let dacl = dacl.split("S:").next().unwrap_or(dacl);
    dacl.split('(').skip(1).all(|entry| {
        let entry = entry.split(')').next().unwrap_or("");
        let fields: Vec<&str> = entry.split(';').collect();
        let allows = matches!(fields.first(), Some(&("A" | "OA" | "XA" | "ZA")));
        let trustee = fields.get(5).copied().unwrap_or("");
        !allows || trustee == user || OWN.contains(&trustee)
    })
}

#[cfg(test)]
mod tests {
    use super::allows_only_owner;

    const USER: &str = "S-1-5-21-1-2-3-1001";

    #[test]
    fn only_the_user_the_system_and_administrators_are_private() {
        assert!(allows_only_owner(
            &format!("D:PAI(A;OICI;FA;;;{USER})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"),
            USER
        ));
        assert!(allows_only_owner("D:P", USER));
    }

    #[test]
    fn everybody_users_and_strangers_are_not() {
        assert!(!allows_only_owner(
            &format!("D:(A;;FA;;;{USER})(A;;FR;;;WD)"),
            USER
        ));
        assert!(!allows_only_owner("D:(A;;FR;;;BU)", USER));
        assert!(!allows_only_owner("D:(A;;FA;;;S-1-5-21-9-9-9-1002)", USER));
        assert!(!allows_only_owner("O:BAG:BA", USER));
    }

    #[test]
    fn a_null_access_list_allows_everybody() {
        assert!(!allows_only_owner("D:NO_ACCESS_CONTROL", USER));
        assert!(!allows_only_owner("D:PNO_ACCESS_CONTROL", USER));
    }

    #[test]
    fn a_deny_entry_for_somebody_else_does_not_make_it_open() {
        assert!(allows_only_owner(
            &format!("D:(D;;FA;;;WD)(A;;FA;;;{USER})"),
            USER
        ));
    }
}
