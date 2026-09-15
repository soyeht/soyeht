use super::registry;
use crate::{
    core::Core,
    errors::{self, Result},
};
use serde_json::{Value, json};
use soyeht_protocol::{
    HarnessCustomCreateParams, HarnessCustomListResult, HarnessCustomRemoveParams,
    HarnessCustomRemoveResult, HarnessCustomUpdateParams,
};

impl Core {
    fn harness_is_bound(&self, harness: &str) -> bool {
        self.panes.values().any(|pane| {
            !pane.stored.closed
                && pane
                    .stored
                    .pane
                    .agent
                    .as_ref()
                    .is_some_and(|agent| agent.harness == harness)
        })
    }

    fn commit_harness_file_mutation(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
        result: Value,
    ) -> Result<Value> {
        Ok(self
            .mutate(connection, method, params, |_| {
                Ok((result.clone(), Vec::new()))
            })?
            .result)
    }

    fn create_custom_harness(&mut self, connection: &str, params: &Value) -> Result<Value> {
        if let Some(result) = self.repeated_mutation(connection, "harness.custom.create", params)? {
            return Ok(result);
        }
        let parsed: HarnessCustomCreateParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid custom harness parameters"))?;
        let harness = registry::create_custom(&self.directory, parsed.harness)?;
        self.commit_harness_file_mutation(
            connection,
            "harness.custom.create",
            params,
            json!({"harness":harness}),
        )
    }

    fn update_custom_harness(&mut self, connection: &str, params: &Value) -> Result<Value> {
        if let Some(result) = self.repeated_mutation(connection, "harness.custom.update", params)? {
            return Ok(result);
        }
        let parsed: HarnessCustomUpdateParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid custom harness parameters"))?;
        if self.harness_is_bound(&parsed.harness.id) {
            return Err(errors::pane_busy(
                "Close panes using this harness before editing it",
            ));
        }
        let harness = registry::update_custom(&self.directory, parsed.harness)?;
        self.commit_harness_file_mutation(
            connection,
            "harness.custom.update",
            params,
            json!({"harness":harness}),
        )
    }

    fn remove_custom_harness(&mut self, connection: &str, params: &Value) -> Result<Value> {
        if let Some(result) = self.repeated_mutation(connection, "harness.custom.remove", params)? {
            return Ok(result);
        }
        let parsed: HarnessCustomRemoveParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid custom harness parameters"))?;
        if self.harness_is_bound(&parsed.harness_id) {
            return Err(errors::pane_busy(
                "Close panes using this harness before removing it",
            ));
        }
        registry::remove_custom(&self.directory, &parsed.harness_id)?;
        let result = HarnessCustomRemoveResult {
            harness_id: parsed.harness_id,
            removed: true,
        };
        self.commit_harness_file_mutation(
            connection,
            "harness.custom.remove",
            params,
            json!(result),
        )
    }

    pub fn agent_custom_harness(
        &mut self,
        connection: &str,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        match method {
            "harness.custom.list" => Ok(json!(HarnessCustomListResult {
                items: registry::custom_launches(&self.directory)?,
            })),
            "harness.custom.create" => self.create_custom_harness(connection, params),
            "harness.custom.update" => self.update_custom_harness(connection, params),
            "harness.custom.remove" => self.remove_custom_harness(connection, params),
            _ => Err(errors::invalid("Unknown custom harness method")),
        }
    }
}
