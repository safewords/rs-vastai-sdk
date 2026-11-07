# rs-vastai-sdk

An unofficial, third-party async Rust client for the [vast.ai](https://vast.ai)
REST API. Not affiliated with or endorsed by Vast.ai.

It covers what renting GPUs from code needs: search the market, rent an offer
on demand or as an interruptible bid, watch, re-bid, stop and destroy
instances, and read their logs.

```rust
use vastai::{Client, CreateInstance, OfferKind, OfferQuery};

let vast = Client::new(std::env::var("VAST_API_KEY")?)?;

let offers = vast
    .search_offers(
        &OfferQuery::new()
            .kind(OfferKind::Interruptible)
            .any_of("gpu_name", ["RTX 4060", "L4"])
            .gte("reliability2", 0.98)
            .order_by("dph_total", true)
            .limit(10),
    )
    .await?;

let offer = &offers[0];
let created = vast
    .create_instance(
        offer.id,
        &CreateInstance {
            image: "nvidia/cuda:12.4.1-base-ubuntu22.04".into(),
            disk_gb: 16.0,
            label: Some("my-batch".into()),
            // Interruptible, bidding 20% over the floor. Bids are per GPU.
            bid_per_gpu: Some(offer.min_bid_per_gpu() * 1.2),
            args: vec!["nvidia-smi".into()],
            ..Default::default()
        },
    )
    .await?;

vast.destroy_instance(created.id).await?;
```

## Prices and units

Every price is US dollars per hour.

- In a **search result**, an offer's `dph_total`, `dph_base` and `min_bid` are
  for the whole offer — all `num_gpus` of it.
- The **bid you send** to rent interruptibly, or to change a bid, is **per GPU**.

`Offer::min_bid_per_gpu()` and `Offer::dph_per_gpu()` convert, and
`CreateInstance::bid_per_gpu` is named for its unit.

## Interruptible instances

An interruptible instance runs while its bid is the highest on the machine.
When it is outbid, or the machine is rented on demand, it is **stopped**, not
destroyed: it keeps its disk, storage is still billed, and it resumes if its
bid wins again. `Instance::status()` reports that as `InstanceStatus::Outbid`,
so a controller can raise the bid (`Client::change_bid`) or give up on the
machine (`Client::destroy_instance`).

Two things Vast does that are easy to get wrong:

- A preempted instance has `intended_status: "stopped"`, exactly like one its
  owner stopped. So any stopped interruptible instance reads as `Outbid`; if
  you stop your own, track that yourself.
- The instance's `min_bid` is not updated when it is outbid. What the machine
  now costs is on its offer: search with `machine_id` to find out.

## Retries

Reads, searches and idempotent writes (destroy, change bid, stop/start, logs)
are retried with backoff on 429 and 5xx. `create_instance` is never retried: a
retry after a lost answer would rent twice.

## Example

```sh
VAST_API_KEY=... cargo run --example market -- "RTX 4060" "L4"
```

Read-only: prints your credit and the cheapest matching interruptible offers.

## Feature docs

How each part works, verified against the code with `path:line` references,
and the known defects: [`docs/features/README.md`](docs/features/README.md).

## License

MIT
