//! An unofficial async client for the [vast.ai](https://vast.ai) REST API.
//!
//! Covers what renting GPUs from code needs, and nothing it does not: search
//! the market, rent an offer (on demand or interruptible), watch, re-bid, stop
//! and destroy instances, and read their logs.
//!
//! ```no_run
//! # async fn demo() -> Result<(), vastai::Error> {
//! use vastai::{Client, OfferQuery, OfferKind};
//!
//! let vast = Client::new(std::env::var("VAST_API_KEY").unwrap())?;
//! let offers = vast
//!     .search_offers(
//!         &OfferQuery::new()
//!             .kind(OfferKind::Interruptible)
//!             .any_of("gpu_name", ["RTX 4060", "L4"])
//!             .gte("reliability2", 0.98)
//!             .order_by("dph_total", true)
//!             .limit(20),
//!     )
//!     .await?;
//! # Ok(()) }
//! ```
//!
//! # Prices
//!
//! Every price is US dollars per hour. In a search result, an offer's
//! `dph_total`, `dph_base` and `min_bid` are all for the **whole offer** — all
//! `num_gpus` of it; `min_bid` equals `dph_base` for the slice. The `price` you
//! *send* to rent interruptibly or to re-bid is **per GPU** (`vastai change
//! bid`: "Per gpu bid price changed"). [`Offer::min_bid_per_gpu`] and
//! [`CreateInstance::bid_per_gpu`] keep the two units apart so callers do not
//! have to remember which is which.

mod client;
mod error;
mod instances;
mod offers;

pub use client::{Client, DEFAULT_BASE_URL};
pub use error::Error;
pub use instances::{CreateInstance, CreatedInstance, Instance, InstanceStatus, RunType};
pub use offers::{Offer, OfferKind, OfferQuery};

/// The account the API key belongs to.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct User {
    pub id: u64,
    #[serde(default)]
    pub username: Option<String>,
    /// Prepaid credit left, in dollars.
    #[serde(default)]
    pub credit: f64,
    #[serde(default)]
    pub balance: f64,
    #[serde(default)]
    pub can_pay: bool,
}
