use super::registry;
use crate::{
    core::Core,
    errors::{self, Result},
};
use serde_json::{Value, json};
use soyeht_protocol::{
    HarnessCustomCreateParams, HarnessCustomListResult, HarnessCustomRemoveParams,
    HarnessCustomRemoveResult, HarnessCustomUpdateParams, method,
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
        let result = self
            .mutate(connection, method, params, |_| {
                Ok((result.clone(), Vec::new()))
            })?
            .result;
        self.catalog_epoch = self.catalog_epoch.wrapping_add(1);
        Ok(result)
    }

    fn create_custom_harness(&mut self, connection: &str, params: &Value) -> Result<Value> {
        if let Some(result) =
            self.repeated_mutation(connection, method::HARNESS_CUSTOM_CREATE, params)?
        {
            return Ok(result);
        }
        let parsed: HarnessCustomCreateParams = serde_json::from_value(params.clone())
            .map_err(|_| errors::invalid("Invalid custom harness parameters"))?;
        let harness = registry::create_custom(&self.directory, parsed.harness)?;
        self.commit_harness_file_mutation(
            connection,
            method::HARNESS_CUSTOM_CREATE,
            params,
            json!({"harness":harness}),
        )
    }

    fn update_custom_harness(&mut self, connection: &str, params: &Value) -> Result<Value> {
        if let Some(result) =
            self.repeated_mutation(connection, method::HARNESS_CUSTOM_UPDATE, params)?
        {
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
            method::HARNESS_CUSTOM_UPDATE,
            params,
            json!({"harness":harness}),
        )
    }

    fn remove_custom_harness(&mut self, connection: &str, params: &Value) -> Result<Value> {
        if let Some(result) =
            self.repeated_mutation(connection, method::HARNESS_CUSTOM_REMOVE, params)?
        {
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
            method::HARNESS_CUSTOM_REMOVE,
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
            method::HARNESS_CUSTOM_LIST => Ok(json!(HarnessCustomListResult {
                items: registry::custom_launches(&self.directory)?,
            })),
            method::HARNESS_CUSTOM_CREATE => self.create_custom_harness(connection, params),
            method::HARNESS_CUSTOM_UPDATE => self.update_custom_harness(connection, params),
            method::HARNESS_CUSTOM_REMOVE => self.remove_custom_harness(connection, params),
            _ => Err(errors::invalid("Unknown custom harness method")),
        }
    }
}
