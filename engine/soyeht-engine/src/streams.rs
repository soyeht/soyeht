use crate::{
    core::Core,
    errors::{self, Result},
    model::{self, Subscription},
};
use serde_json::{Value, json};
use soyeht_protocol::{frame::Frame, limits};
use soyeht_store::segments::Record;
use std::sync::{Arc, atomic::AtomicBool};

impl Core {
    pub fn subscribe_stream(&mut self, connection: &str, id: Value, params: &Value) -> Result<()> {
        let pane_id = errors::param_str(params, "paneId")?;
        let running = self
            .panes
            .get(pane_id)
            .filter(|p| !p.stored.closed)
            .ok_or_else(|| errors::not_found("Pane not found"))?;
        if running.stored.pane.mode != soyeht_protocol::PaneMode::Terminal {
            return Err(errors::invalid("Binary streams require a terminal pane"));
        }
        let stream_id = running
            .stored
            .pane
            .stream_id
            .clone()
            .ok_or_else(|| errors::internal("Terminal pane has no stream identity"))?;
        let generation = running.stored.pane.generation;
        let high = running.screen.next_offset();
        let tail = params["from"] == "tail";
        let from = if tail {
            running.screen.tail_start()
        } else {
            if params["from"]["streamId"] != stream_id {
                return Err(errors::resync_required("Stream identity changed"));
            }
            let offset = params["from"]["offset"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| errors::invalid("Invalid stream offset"))?;
            if offset < running.screen.first_offset() || offset > high {
                return Err(errors::resync_required(
                    "Stream offset outside retained range",
                ));
            }
            offset
        };
        let current = Arc::new(AtomicBool::new(true));
        let client = errors::invariant(
            self.clients.get_mut(connection),
            "Stream connection disappeared",
        )?;
        let binary_ref = client.next_binary_ref;
        client.next_binary_ref = binary_ref
            .checked_add(1)
            .ok_or_else(|| errors::resync_required("Connection stream references exhausted"))?;
        // Mapping precedes every binary frame. All tail boundaries were captured
        // while holding the same engine lock used by output ingestion.
        client.streams.remove(pane_id);
        client.outbox.send(model::response(id,json!({"streamId":stream_id,"paneId":pane_id,"generation":generation,"firstRetainedOffset":running.screen.first_offset().to_string(),"nextOffset":high.to_string(),"binaryPaneRef":binary_ref}))?);
        client.outbox.send_stream(model::notification("stream.replay_start",json!({"streamId":stream_id,"generation":generation,"fromOffset":from.to_string(),"toOffset":high.to_string(),"reset":tail}))?,Arc::clone(&current));
        client.streams.insert(
            pane_id.into(),
            Subscription {
                binary_ref,
                stream_id,
                generation,
                next: from,
                replay_until: high,
                replaying: true,
                current,
            },
        );
        Ok(())
    }

    pub fn pump_streams(&mut self) -> Result<()> {
        for client in self
            .clients
            .values_mut()
            .filter(|c| c.actor.is_some() && !c.outbox.is_closed())
        {
            let ids: Vec<_> = client.streams.keys().cloned().collect();
            for id in ids {
                let Some(pane) = self.panes.get(&id).filter(|p| !p.stored.closed) else {
                    client.streams.remove(&id);
                    continue;
                };
                let subscription = errors::invariant(
                    client.streams.get_mut(&id),
                    "Stream subscription disappeared during its pump",
                )?;
                if pane.stored.pane.stream_id.as_ref() != Some(&subscription.stream_id) {
                    client
                        .outbox
                        .close(crate::outbox::CloseReason::StreamChanged);
                    break; // Reconnect snapshot discovers the new stream generation.
                }
                let through = if subscription.replaying {
                    subscription.replay_until
                } else {
                    pane.screen.next_offset()
                };
                if subscription.next < pane.screen.first_offset() {
                    let to = pane.screen.first_offset().min(through);
                    client.outbox.send_stream(model::notification("stream.gap",json!({"streamId":subscription.stream_id,"fromOffset":subscription.next.to_string(),"toOffset":to.to_string()}))?,Arc::clone(&subscription.current));
                    subscription.next = to;
                }
                if subscription.next < through {
                    for record in pane.screen.read(
                        subscription.next,
                        through,
                        limits::TERMINAL_PAYLOAD_BYTES,
                    )? {
                        let end = record.end();
                        let frame = match record {
                            Record::Data { offset, bytes } => Frame::Terminal {
                                pane_ref: subscription.binary_ref,
                                generation: subscription.generation,
                                offset,
                                bytes,
                            },
                            Record::Gap { from, to } => model::notification(
                                "stream.gap",
                                json!({"streamId":subscription.stream_id,"fromOffset":from.to_string(),"toOffset":to.to_string()}),
                            )?,
                        };
                        if !client
                            .outbox
                            .send_stream(frame, Arc::clone(&subscription.current))
                        {
                            break;
                        }
                        subscription.next = end;
                    }
                }
                if subscription.replaying && subscription.next == subscription.replay_until {
                    client.outbox.send_stream(model::notification("stream.replay_done",json!({"streamId":subscription.stream_id,"nextOffset":subscription.next.to_string()}))?,Arc::clone(&subscription.current));
                    subscription.replaying = false;
                }
            }
        }
        Ok(())
    }
}
