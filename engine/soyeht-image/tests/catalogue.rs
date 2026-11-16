#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::Value;
use soyeht_image::{Error, find, registry};
use std::collections::BTreeSet;

#[test]
fn the_registry_lists_local_and_cloud_entries_with_unique_ids_and_a_license_each() {
    let entries = registry().unwrap();
    assert!(entries.len() >= 2);
    let ids: BTreeSet<_> = entries.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(ids.len(), entries.len(), "ids are unique");
    let localities: BTreeSet<_> = entries
        .iter()
        .map(|entry| format!("{:?}", entry.locality))
        .collect();
    assert_eq!(localities.len(), 2, "a local and a cloud entry");
    for entry in &entries {
        assert!(!entry.license.name.is_empty(), "{}", entry.id);
        assert!(
            entry.timeout_seconds > 0 && entry.size_multiple > 0,
            "{}",
            entry.id
        );
    }
}

#[test]
fn a_comfyui_entry_carries_a_graph_whose_bindings_all_resolve() {
    let mut found = 0;
    for entry in registry().unwrap() {
        let Some(recipe) = entry.recipe else { continue };
        found += 1;
        let bindings = &recipe.bindings;
        for slot in [
            &bindings.image,
            &bindings.prompt,
            &bindings.seed,
            &bindings.width,
            &bindings.height,
        ] {
            assert!(
                recipe.graph[&slot.0]["inputs"].get(&slot.1).is_some(),
                "{} has no input {}.{}",
                entry.id,
                slot.0,
                slot.1
            );
        }
        assert!(recipe.graph.get(&bindings.output).is_some());
    }
    assert!(found >= 1);
}

#[test]
fn a_descriptor_serializes_without_its_graph_and_a_cloud_entry_needs_a_key() {
    for entry in registry().unwrap() {
        let value: Value = serde_json::to_value(&entry).unwrap();
        assert!(value.get("recipe").is_none());
        if format!("{:?}", entry.locality) == "Cloud" {
            assert!(entry.needs_key, "{}", entry.id);
        }
    }
}

#[test]
fn an_unknown_provider_is_named() {
    assert!(matches!(
        find("nothing-like-it"),
        Err(Error::UnknownProvider(id)) if id == "nothing-like-it"
    ));
}

fn cloud() -> soyeht_image::Descriptor {
    registry()
        .unwrap()
        .into_iter()
        .find(|entry| format!("{:?}", entry.locality) == "Cloud")
        .unwrap()
}

fn local() -> soyeht_image::Descriptor {
    registry()
        .unwrap()
        .into_iter()
        .find(|entry| format!("{:?}", entry.locality) == "Local")
        .unwrap()
}

#[test]
fn a_cloud_entry_takes_only_https_and_needs_the_person_to_name_its_address_and_model() {
    assert!(matches!(
        cloud().choose(None, Some("m")),
        Err(Error::NeedsEndpoint)
    ));
    assert!(matches!(
        cloud().choose(Some("http://example.test/v1"), Some("m")),
        Err(Error::InvalidEndpoint(_))
    ));
    assert!(matches!(
        cloud().choose(Some("https://example.test/v1"), None),
        Err(Error::NeedsModel)
    ));
    let choice = cloud()
        .choose(Some("https://example.test/v1/"), Some("m"))
        .unwrap();
    assert_eq!(choice.model.as_deref(), Some("m"));
}

#[test]
fn an_address_never_holds_a_secret_or_anything_but_a_host_and_a_path() {
    for bad in [
        "https://user:pass@example.test/v1",
        "https://user@example.test/v1",
        "https://example.test/v1?key=secret",
        "https://example.test/v1#part",
        "ftp://example.test",
        "not an address",
    ] {
        assert!(
            matches!(
                local().choose(Some(bad), None),
                Err(Error::InvalidEndpoint(_))
            ),
            "{bad}"
        );
    }
}

#[test]
fn a_local_entry_starts_from_its_own_address_and_takes_the_persons_over_it() {
    let own = local().choose(None, None).unwrap();
    assert!(own.endpoint.as_str().starts_with("http://"));
    let moved = local()
        .choose(Some("http://192.168.1.20:8188"), None)
        .unwrap();
    assert_eq!(moved.endpoint.host_str(), Some("192.168.1.20"));
}
