//! What `soyeht service ...` prints (contract 13, section 3): the state of the profile's services
//! after the command. The command line writes it and the window of the Linux and Windows app
//! reads it, so the two share this one definition instead of each declaring the JSON.

use serde::{Deserialize, Serialize};

/// The state of one service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceState {
    /// `ptyd` or `engine`.
    pub component: String,
    /// The label, unit or task name of the system.
    pub name: String,
    /// The system will start it at login.
    pub registered: bool,
    /// It runs now.
    pub running: bool,
    /// Only in the answer to `status --programs`: its definition on disk is exactly what `install`
    /// would write for those programs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<bool>,
}

/// The report: every service of the profile that the command touched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceReport {
    pub services: Vec<ServiceState>,
}

impl ServiceReport {
    /// How many of the services are registered, and how many the report lists.
    pub fn registered(&self) -> (usize, usize) {
        let registered = self
            .services
            .iter()
            .filter(|state| state.registered)
            .count();
        (registered, self.services.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(component: &str, registered: bool) -> ServiceState {
        ServiceState {
            component: component.into(),
            name: format!("{component}.service"),
            registered,
            running: false,
            current: None,
        }
    }

    #[test]
    fn the_report_is_the_json_of_contract_13_and_reads_back() {
        let report = ServiceReport {
            services: vec![state("ptyd", true), state("engine", false)],
        };
        let printed = serde_json::to_value(&report).unwrap();
        assert_eq!(
            printed,
            serde_json::json!({"services": [
                {"component": "ptyd", "name": "ptyd.service", "registered": true, "running": false},
                {"component": "engine", "name": "engine.service", "registered": false, "running": false},
            ]})
        );
        assert_eq!(
            serde_json::from_value::<ServiceReport>(printed).unwrap(),
            report
        );
        assert_eq!(report.registered(), (1, 2));
    }

    #[test]
    fn current_is_there_only_for_a_status_with_programs_and_other_keys_are_ignored() {
        let text = r#"{"dataDeleted":false,"services":[
            {"component":"ptyd","name":"a","registered":true,"running":true,"current":false}]}"#;
        let report: ServiceReport = serde_json::from_str(text).unwrap();
        assert_eq!(report.services[0].current, Some(false));
        assert!(
            serde_json::to_string(&state("ptyd", true))
                .unwrap()
                .find("current")
                .is_none()
        );
    }
}
