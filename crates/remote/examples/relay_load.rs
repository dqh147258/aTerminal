//! Bounded relay load probe. Reports measured throughput, not an application benchmark.
use ai_terminal_protocol::local::{Reply, Request};
use ai_terminal_remote::{Channel, create_pair};
use anyhow::{Context, Result};
use prost::Message;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    let url = args
        .get(1)
        .context("usage: relay_load SERVER ADMIN_TOKEN_FILE [PAIRS] [SECONDS]")?;
    let token = std::fs::read_to_string(args.get(2).context("token file required")?)?;
    let count = args.get(3).map_or(Ok(20), |s| s.parse::<usize>())?.min(64);
    let seconds = args.get(4).map_or(Ok(30), |s| s.parse::<u64>())?.min(1800);
    let bytes = Arc::new(AtomicU64::new(0));
    let mut jobs = Vec::new();
    let mut rooms = Vec::new();
    let mut desktops = Vec::new();
    for _ in 0..count {
        let (mut pair, mut invite) = create_pair(url, token.trim(), false).await?;
        pair.ice_servers.clear();
        invite.ice_servers.clear();
        rooms.push(pair.room.clone());
        desktops.push(tokio::spawn(async move {
            let mut channel = Channel::accept(&pair).await?;
            channel.disable_direct();
            while let Ok(request) = channel.receive().await {
                let size = request.len() as u64;
                channel
                    .send(
                        &Reply {
                            accepted_input_seq: size,
                            ..Reply::default()
                        }
                        .encode_to_vec(),
                    )
                    .await?;
            }
            Ok::<_, anyhow::Error>(())
        }));
        let mut channel = Channel::connect(&invite).await?;
        channel.disable_direct();
        let total = bytes.clone();
        jobs.push(tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            for _ in 0..seconds {
                tick.tick().await;
                let reply = channel
                    .request(Request {
                        input: vec![b'x'; 65536],
                        ..Request::default()
                    })
                    .await?;
                total.fetch_add(reply.accepted_input_seq, Ordering::Relaxed);
            }
            Ok::<_, anyhow::Error>(())
        }));
    }
    let mut failure = None;
    for job in jobs {
        if let Err(error) = job.await? {
            failure = Some(error)
        }
    }
    for desktop in desktops {
        desktop.abort();
    }
    let http = reqwest::Client::new();
    for room in rooms {
        http.delete(format!("{url}/v1/pairs/{room}"))
            .bearer_auth(token.trim())
            .send()
            .await?
            .error_for_status()?;
    }
    if let Some(error) = failure {
        return Err(error);
    }
    println!(
        "pairs={count} seconds={seconds} payload_bytes={} workload=64KiB_per_pair_per_second",
        bytes.load(Ordering::Relaxed)
    );
    Ok(())
}
