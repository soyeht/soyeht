//! The image catalogue (ADR 0166, ADR 0083's shape): a descriptor per provider, one registry, and
//! nothing that knows a model by name. The entries are data (`data/catalogue.json`); the code
//! reads them and never matches on an id or a model.
//!
//! A descriptor says where the work runs (`Locality`), which adapter speaks to it, the address and
//! model it starts with, whether it needs the person's own key, what it receives, and its license
//! (a name and a `commercialUse` flag the screens show before the first use; nothing enforces
//! it). The person's own address and model, given for one call, replace the defaults
//! (`Descriptor::choose`).

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

const CATALOGUE: &str = include_str!("../data/catalogue.json");

/// Where the work runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Locality {
    /// A computer of the person's own.
    Local,
    /// Someone else's computer, reached over the internet with the person's own key.
    Cloud,
}

/// Which protocol the provider speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Adapter {
    /// `POST /images/edits`, as the OpenAI image-edit endpoint and the services that copy it.
    OpenaiImages,
    /// A ComfyUI server: upload the picture, queue a graph, wait, fetch the result.
    Comfyui,
}

impl Adapter {
    /// Whether the protocol carries a model name.
    pub fn takes_model(self) -> bool {
        matches!(self, Self::OpenaiImages)
    }

    /// Whether the protocol carries a seed, so that another seed gives another picture.
    pub fn takes_seed(self) -> bool {
        matches!(self, Self::Comfyui)
    }
}

/// What a provider receives; the screen names it before the first use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Sent {
    Picture,
    Text,
}

/// The license of the provider's model or service, as data for the screens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct License {
    pub name: String,
    /// False for research-only terms; absent when the terms are not known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commercial_use: Option<bool>,
}

/// A `[node, input]` of a ComfyUI graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Slot(pub String, pub String);

/// Where a ComfyUI graph takes each value of one edit, and which node saves the picture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bindings {
    pub image: Slot,
    pub prompt: Slot,
    pub seed: Slot,
    pub width: Slot,
    pub height: Slot,
    pub output: String,
}

/// A ComfyUI graph in its API form, with the places the edit's values go.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub bindings: Bindings,
    pub graph: Value,
}

/// One image provider the person can choose.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Descriptor {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub locality: Locality,
    pub adapter: Adapter,
    /// The model name the entry starts with; absent when the person names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The address the entry starts with; absent when the person gives it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    pub needs_key: bool,
    pub timeout_seconds: u64,
    /// Both sides of the picture must be a multiple of this.
    pub size_multiple: u32,
    pub license: License,
    pub sends: Vec<Sent>,
    #[serde(default, skip_serializing)]
    pub recipe: Option<Recipe>,
}

#[derive(Deserialize)]
struct Document {
    entries: Vec<Descriptor>,
}

/// Every provider of the catalogue, in the order the screen lists them.
pub fn registry() -> Result<Vec<Descriptor>> {
    serde_json::from_str::<Document>(CATALOGUE)
        .map(|document| document.entries)
        .map_err(|_| Error::Data("catalogue.json"))
}

/// The provider with this id.
pub fn find(id: &str) -> Result<Descriptor> {
    registry()?
        .into_iter()
        .find(|descriptor| descriptor.id == id)
        .ok_or_else(|| Error::UnknownProvider(id.to_owned()))
}

/// A provider with the address and model one call will use. Only `Descriptor::choose` makes one,
/// so the rules about an address cannot be bypassed.
#[derive(Debug)]
pub struct Choice {
    pub(crate) descriptor: Descriptor,
    pub(crate) endpoint: Url,
    pub(crate) model: Option<String>,
}

impl Descriptor {
    /// The address of one call: the person's own, else the entry's. A cloud entry takes only an
    /// https address; a local one may take http. An address never carries a user name, a password,
    /// a query or a fragment, so nothing secret can end up in a URL.
    fn address(&self, endpoint: Option<&str>) -> Result<Url> {
        let text = endpoint
            .or(self.endpoint.as_deref())
            .ok_or(Error::NeedsEndpoint)?;
        let endpoint = Url::parse(text).map_err(|_| Error::InvalidEndpoint("not an address"))?;
        let scheme_ok = match self.locality {
            Locality::Cloud => endpoint.scheme() == "https",
            Locality::Local => matches!(endpoint.scheme(), "http" | "https"),
        };
        if !scheme_ok {
            return Err(Error::InvalidEndpoint(match self.locality {
                Locality::Cloud => "a cloud provider is reached over https",
                Locality::Local => "only http and https are used",
            }));
        }
        if endpoint.host_str().is_none() {
            return Err(Error::InvalidEndpoint("no host"));
        }
        if !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(Error::InvalidEndpoint(
                "no user name, password, query or fragment",
            ));
        }
        Ok(endpoint)
    }

    /// The address and port a key for this entry is stored under, for the address the person
    /// names or else the entry's own.
    pub fn key_host(&self, endpoint: Option<&str>) -> Result<String> {
        Ok(host_key(&self.address(endpoint)?))
    }

    /// The address and model of one call: the person's own, else the entry's. A model is refused
    /// where the protocol takes none, so it is never silently dropped.
    pub fn choose(self, endpoint: Option<&str>, model: Option<&str>) -> Result<Choice> {
        let endpoint = self.address(endpoint)?;
        if model.is_some() && !self.adapter.takes_model() {
            return Err(Error::NotUsed("model"));
        }
        let model = model
            .map(str::to_owned)
            .or_else(|| self.model.clone())
            .filter(|model| !model.is_empty());
        if self.adapter.takes_model() && model.is_none() {
            return Err(Error::NeedsModel);
        }
        Ok(Choice {
            descriptor: self,
            endpoint,
            model,
        })
    }
}

impl Choice {
    /// The address and the port the key of this call belongs to.
    pub fn key_host(&self) -> String {
        host_key(&self.endpoint)
    }

    /// A cloud provider is reached over https only.
    pub(crate) fn https_only(&self) -> bool {
        self.descriptor.locality == Locality::Cloud
    }

    /// The address of `segments` below the endpoint, with `query` pairs. Every part is
    /// percent-encoded, so text a server sent cannot change the path or add a part.
    pub(crate) fn url(&self, segments: &[&str], query: &[(&str, &str)]) -> Result<String> {
        if segments.iter().any(|part| matches!(*part, "" | "." | "..")) {
            return Err(Error::InvalidRequest("a path part that moves up or stays"));
        }
        let mut url = self.endpoint.clone();
        url.path_segments_mut()
            .map_err(|()| Error::InvalidEndpoint("not an address"))?
            .pop_if_empty()
            .extend(segments);
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        Ok(url.to_string())
    }

    /// Whether the address is this computer.
    pub(crate) fn is_loopback(&self) -> bool {
        match self.endpoint.host() {
            Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
            Some(url::Host::Ipv4(address)) => address.is_loopback(),
            Some(url::Host::Ipv6(address)) => address.is_loopback(),
            None => false,
        }
    }
}

/// `host:port` of an address: what a key is stored under.
fn host_key(address: &Url) -> String {
    format!(
        "{}:{}",
        address.host_str().unwrap_or_default(),
        address.port_or_known_default().unwrap_or_default()
    )
}

/// [`host_key`] of an address given as text, whatever entry it is for.
pub fn host_of(address: &str) -> Result<String> {
    Url::parse(address)
        .map(|address| host_key(&address))
        .map_err(|_| Error::InvalidEndpoint("not an address"))
}
