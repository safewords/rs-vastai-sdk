//! Read-only: print the account's credit and the cheapest interruptible
//! offers for a few GPU models. Rents nothing.
//!
//! ```sh
//! VAST_API_KEY=... cargo run --example market -- "RTX 4060" "L4"
//! ```

use vastai::{Client, OfferKind, OfferQuery};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let vast = Client::new(std::env::var("VAST_API_KEY")?)?;
    let user = vast.current_user().await?;
    println!("account {} — credit ${:.2}", user.id, user.credit);

    let gpus: Vec<String> = std::env::args().skip(1).collect();
    let mut query = OfferQuery::new()
        .kind(OfferKind::Interruptible)
        .gte("reliability2", 0.97)
        .order_by("dph_total", true)
        .limit(15);
    if !gpus.is_empty() {
        query = query.any_of("gpu_name", gpus);
    }
    for o in vast.search_offers(&query).await? {
        println!(
            "{:>9}  {:2}× {:<14} min bid ${:.3}/gpu/h  on-demand ${:.3}/gpu/h  rel {:.3}  ↓{:>5.0} ↑{:>5.0} Mbit/s  {}",
            o.id,
            o.num_gpus,
            o.gpu_name,
            o.min_bid_per_gpu(),
            o.dph_per_gpu(),
            o.reliability2,
            o.inet_down,
            o.inet_up,
            o.geolocation.as_deref().unwrap_or("?"),
        );
    }
    Ok(())
}
