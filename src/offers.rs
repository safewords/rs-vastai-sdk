use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// How an offer would be rented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OfferKind {
    /// Fixed price; nobody can take the machine away while it is rented.
    #[serde(rename = "on-demand")]
    OnDemand,
    /// A bid. Runs while it is the highest bid on the machine, and is
    /// **stopped** — not destroyed — when somebody outbids it or rents the
    /// machine on demand. It resumes if it becomes the highest bid again.
    #[serde(rename = "bid")]
    Interruptible,
}

/// A search over the market, as Vast's `POST /bundles/` takes it: one
/// `{field: {op: value}}` per filter, plus `type`, `order` and `limit`.
///
/// `rentable = true` and `rented = false` are set by default, because an offer
/// that cannot be rented is never what a caller wants.
#[derive(Debug, Clone)]
pub struct OfferQuery {
    filters: Map<String, Value>,
    kind: OfferKind,
    order: Vec<(String, bool)>,
    limit: Option<u32>,
}

impl Default for OfferQuery {
    fn default() -> Self {
        Self::new()
    }
}

impl OfferQuery {
    pub fn new() -> Self {
        let mut q = Self {
            filters: Map::new(),
            kind: OfferKind::OnDemand,
            order: Vec::new(),
            limit: None,
        };
        q = q.eq("rentable", true).eq("rented", false);
        q
    }

    /// No default filters: rented and unrentable offers too. What a machine
    /// you were just outbid on now costs is only on an offer like that.
    pub fn including_rented() -> Self {
        Self {
            filters: Map::new(),
            kind: OfferKind::OnDemand,
            order: Vec::new(),
            limit: None,
        }
    }

    pub fn kind(mut self, kind: OfferKind) -> Self {
        self.kind = kind;
        self
    }

    fn op(mut self, field: &str, op: &str, value: Value) -> Self {
        let entry = self
            .filters
            .entry(field.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(ops) = entry {
            ops.insert(op.to_string(), value);
        }
        self
    }

    pub fn eq(self, field: &str, value: impl Into<Value>) -> Self {
        self.op(field, "eq", value.into())
    }

    pub fn neq(self, field: &str, value: impl Into<Value>) -> Self {
        self.op(field, "neq", value.into())
    }

    pub fn gte(self, field: &str, value: impl Into<Value>) -> Self {
        self.op(field, "gte", value.into())
    }

    pub fn lte(self, field: &str, value: impl Into<Value>) -> Self {
        self.op(field, "lte", value.into())
    }

    /// `field` is one of `values`.
    pub fn any_of<V: Into<Value>>(self, field: &str, values: impl IntoIterator<Item = V>) -> Self {
        let values: Vec<Value> = values.into_iter().map(Into::into).collect();
        self.op(field, "in", Value::Array(values))
    }

    /// Sort by `field`, ascending or not. Repeat for tie-breakers.
    pub fn order_by(mut self, field: &str, ascending: bool) -> Self {
        self.order.push((field.to_string(), ascending));
        self
    }

    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    /// The request body.
    pub fn to_body(&self) -> Value {
        let mut body = self.filters.clone();
        body.insert(
            "type".into(),
            serde_json::to_value(self.kind).unwrap_or_default(),
        );
        if !self.order.is_empty() {
            body.insert(
                "order".into(),
                Value::Array(
                    self.order
                        .iter()
                        .map(|(f, asc)| json!([f, if *asc { "asc" } else { "desc" }]))
                        .collect(),
                ),
            );
        }
        if let Some(limit) = self.limit {
            body.insert("limit".into(), limit.into());
        }
        Value::Object(body)
    }
}

/// One rentable slice of a machine.
///
/// Only the fields worth deciding on are typed; everything else Vast sends is
/// kept in [`Offer::extra`].
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Offer {
    /// The ask id: what [`crate::Client::create_instance`] rents.
    pub id: u64,
    pub machine_id: u64,
    #[serde(default)]
    pub host_id: Option<u64>,
    pub gpu_name: String,
    pub num_gpus: u32,
    /// The share of the machine's GPUs this offer rents: 1.0 is the whole
    /// machine, 0.125 one card of eight. On driver 580 and later NVENC
    /// refuses to open a session (`NV_ENC_ERR_NO_ENCODE_DEVICE`) in a
    /// container that holds only part of a machine, so a caller that encodes
    /// should read this.
    #[serde(default)]
    pub gpu_frac: Option<f64>,
    /// VRAM per GPU, MiB.
    #[serde(default)]
    pub gpu_ram: f64,
    /// Compute capability ×100: 890 is Ada, 1200 is consumer Blackwell.
    #[serde(default)]
    pub compute_cap: Option<u32>,
    #[serde(default)]
    pub driver_version: Option<String>,
    #[serde(default)]
    pub cuda_max_good: Option<f64>,
    #[serde(default)]
    pub cpu_name: Option<String>,
    /// Cores this slice gets, not the machine's.
    #[serde(default)]
    pub cpu_cores_effective: f64,
    /// RAM this slice gets, MiB.
    #[serde(default)]
    pub cpu_ram: f64,
    /// Free disk, GB.
    #[serde(default)]
    pub disk_space: f64,
    /// Measured, Mbit/s.
    #[serde(default)]
    pub inet_down: f64,
    #[serde(default)]
    pub inet_up: f64,
    /// Dollars per GB.
    #[serde(default)]
    pub inet_down_cost: f64,
    #[serde(default)]
    pub inet_up_cost: f64,
    /// GPU, CPU and RAM for the whole offer, $/h.
    #[serde(default)]
    pub dph_base: f64,
    /// `dph_base` plus storage at the default disk, $/h.
    #[serde(default)]
    pub dph_total: f64,
    /// Lowest acceptable bid for the whole offer, $/h. Equals `dph_base` on
    /// the interruptible listing.
    #[serde(default)]
    pub min_bid: f64,
    /// Storage, $ per GB per month.
    #[serde(default)]
    pub storage_cost: f64,
    /// Vast's reliability estimate, 0–1.
    #[serde(default)]
    pub reliability2: f64,
    #[serde(default)]
    pub verification: Option<String>,
    #[serde(default)]
    pub geolocation: Option<String>,
    /// Seconds the host has committed to keep the offer listed.
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub dlperf: Option<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Offer {
    /// The lowest acceptable bid per GPU — the unit a bid is sent in.
    pub fn min_bid_per_gpu(&self) -> f64 {
        self.min_bid / f64::from(self.num_gpus.max(1))
    }

    /// The on-demand price per GPU.
    pub fn dph_per_gpu(&self) -> f64 {
        self.dph_total / f64::from(self.num_gpus.max(1))
    }

    /// Storage for `disk_gb` GB, $/h. Vast bills it whether or not the
    /// instance is running, until it is destroyed.
    pub fn storage_dph(&self, disk_gb: f64) -> f64 {
        // Vast's per-month figures are 30-day months.
        self.storage_cost * disk_gb / (30.0 * 24.0)
    }
}

/// The body of `POST /bundles/`.
#[derive(Debug, Deserialize)]
pub(crate) struct OffersResponse {
    pub offers: Vec<Offer>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_serialises_as_vasts_filter_object() {
        let body = OfferQuery::new()
            .kind(OfferKind::Interruptible)
            .any_of("gpu_name", ["RTX 4060", "L4"])
            .gte("reliability2", 0.98)
            .lte("dph_total", 0.5)
            .order_by("dph_total", true)
            .limit(10)
            .to_body();

        assert_eq!(body["type"], "bid");
        assert_eq!(body["rentable"]["eq"], true);
        assert_eq!(body["rented"]["eq"], false);
        assert_eq!(body["gpu_name"]["in"], json!(["RTX 4060", "L4"]));
        assert_eq!(body["reliability2"]["gte"], 0.98);
        assert_eq!(body["dph_total"]["lte"], 0.5);
        assert_eq!(body["order"], json!([["dph_total", "asc"]]));
        assert_eq!(body["limit"], 10);
    }

    #[test]
    fn two_bounds_on_one_field_both_survive() {
        let body = OfferQuery::new()
            .gte("num_gpus", 1)
            .lte("num_gpus", 2)
            .to_body();
        assert_eq!(body["num_gpus"], json!({"gte": 1, "lte": 2}));
    }

    #[test]
    fn including_rented_drops_the_default_filters() {
        let body = OfferQuery::including_rented().eq("machine_id", 7).to_body();
        assert!(body.get("rented").is_none());
        assert!(body.get("rentable").is_none());
        assert_eq!(body["machine_id"]["eq"], 7);
    }

    #[test]
    fn on_demand_is_the_default_kind() {
        assert_eq!(OfferQuery::new().to_body()["type"], "on-demand");
    }

    #[test]
    fn real_offers_deserialise_and_keep_unknown_fields() {
        let raw = include_str!("../tests/fixtures/bundles.json");
        let parsed: OffersResponse = serde_json::from_str(raw).unwrap();
        assert!(!parsed.offers.is_empty());
        for offer in &parsed.offers {
            assert!(offer.id > 0);
            assert!(offer.num_gpus >= 1);
            assert!(offer.gpu_frac.is_some_and(|f| f > 0.0 && f <= 1.0));
            assert!(offer.min_bid > 0.0);
            assert!(offer.min_bid_per_gpu() <= offer.min_bid);
            assert!(offer.extra.contains_key("gpu_arch"));
        }
    }

    #[test]
    fn per_gpu_prices_divide_by_the_gpu_count() {
        let mut offer: Offer = serde_json::from_value(json!({
            "id": 1, "machine_id": 2, "gpu_name": "L4", "num_gpus": 4,
            "dph_total": 2.0, "min_bid": 1.2, "storage_cost": 0.144,
        }))
        .unwrap();
        assert!((offer.dph_per_gpu() - 0.5).abs() < 1e-9);
        assert!((offer.min_bid_per_gpu() - 0.3).abs() < 1e-9);
        // 0.144 $/GB/month × 50 GB over a 720-hour month.
        assert!((offer.storage_dph(50.0) - 0.01).abs() < 1e-9);
        offer.num_gpus = 0;
        assert!((offer.dph_per_gpu() - 2.0).abs() < 1e-9);
    }
}
