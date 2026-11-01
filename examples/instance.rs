//! Rent, inspect, re-bid and destroy an instance. **`rent` spends money.**
//!
//! ```sh
//! export VAST_API_KEY=...
//! # Rent offer 123 interruptibly at $0.06/GPU/h with ssh, 30 GB of disk:
//! cargo run --example instance -- rent 123 0.06 30 rust:1.90-trixie ~/.ssh/id_ed25519.pub
//! cargo run --example instance -- show 456        # the instance as Vast reports it
//! cargo run --example instance -- bid 456 0.07    # change the bid, per GPU
//! cargo run --example instance -- logs 456
//! cargo run --example instance -- destroy 456
//! ```
//!
//! Pass `-` as the bid to rent on demand.

use std::time::Duration;

use vastai::{Client, CreateInstance, RunType};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let vast = Client::new(std::env::var("VAST_API_KEY")?)?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| -> Result<&str, Box<dyn std::error::Error>> {
        args.get(i)
            .map(String::as_str)
            .ok_or_else(|| format!("missing argument {i}; see the example's docs").into())
    };

    match arg(0)? {
        "rent" => {
            let offer: u64 = arg(1)?.parse()?;
            let bid = match arg(2)? {
                "-" => None,
                b => Some(b.parse::<f64>()?),
            };
            let disk_gb: f64 = arg(3)?.parse()?;
            let image = arg(4)?.to_string();
            let key = std::fs::read_to_string(arg(5)?)?;
            let created = vast
                .create_instance(
                    offer,
                    &CreateInstance {
                        image,
                        disk_gb,
                        label: Some("rs-vastai-sdk-example".into()),
                        bid_per_gpu: bid,
                        runtype: RunType::Ssh,
                        env: vec![(
                            "NVIDIA_DRIVER_CAPABILITIES".into(),
                            "compute,utility,video".into(),
                        )],
                        cancel_unavailable: true,
                        ..Default::default()
                    },
                )
                .await?;
            println!("rented instance {}", created.id);
            vast.attach_ssh_key(created.id, &key).await?;
            println!("ssh key attached");
        }
        "show" => {
            let id: u64 = arg(1)?.parse()?;
            match vast.get_instance(id).await? {
                Some(i) => {
                    println!("status: {:?}", i.status());
                    println!("{}", serde_json::to_string_pretty(&i)?);
                }
                None => println!("instance {id} does not exist"),
            }
        }
        "bid" => {
            let id: u64 = arg(1)?.parse()?;
            vast.change_bid(id, arg(2)?.parse()?).await?;
            println!("bid changed");
        }
        "logs" => {
            let id: u64 = arg(1)?.parse()?;
            print!(
                "{}",
                vast.logs(id, Some(200), Duration::from_secs(20)).await?
            );
        }
        "destroy" => {
            let id: u64 = arg(1)?.parse()?;
            vast.destroy_instance(id).await?;
            println!("destroyed {id}");
        }
        other => return Err(format!("unknown command {other}").into()),
    }
    Ok(())
}
