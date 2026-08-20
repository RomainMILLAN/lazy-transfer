//! Streaming check: a large transfer must not buffer the file in RAM.
//! Run with: cargo test --release --test webdav_mem -- --ignored --nocapture
//! (needs the docker server from tests/webdav_live.rs)
use lazy_transfer::transfer::backend::RemoteBackend;
use lazy_transfer::transfer::types::{WebDavAuth, WebDavConfig};
use lazy_transfer::transfer::webdav_backend::WebDavBackend;

fn rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        .unwrap_or(0)
}

fn drain(h: lazy_transfer::transfer::stream::StreamHandle) {
    for line in h.rx.iter() {
        if line.done {
            assert!(line.err.is_none(), "{:?}", line.err);
            break;
        }
    }
}

#[test]
#[ignore]
fn large_transfers_do_not_buffer_in_ram() {
    let cfg = WebDavConfig {
        url: "http://localhost:18080/".to_string(),
        auth: WebDavAuth::Basic {
            user: "alice".to_string(),
            password: "s3cret".to_string(),
        },
        insecure_tls: false,
    };
    let b = WebDavBackend::connect(&cfg).expect("connect");

    const SIZE: usize = 500 * 1024 * 1024;
    let tmp = tempfile::tempdir().unwrap();
    let big = tmp.path().join("big.bin");
    {
        use std::io::Write;
        let mut f = std::io::BufWriter::new(std::fs::File::create(&big).unwrap());
        let chunk = vec![7u8; 1024 * 1024];
        for _ in 0..(SIZE / chunk.len()) {
            f.write_all(&chunk).unwrap();
        }
    }

    let baseline = rss_kb();
    println!("RSS baseline: {baseline} kB");

    drain(b.upload(big.to_str().unwrap(), "/big.bin").unwrap());
    let after_up = rss_kb();
    println!(
        "RSS after 500 MiB upload: {after_up} kB (+{} kB)",
        after_up.saturating_sub(baseline)
    );

    let back = tmp.path().join("back.bin");
    drain(b.download("/big.bin", back.to_str().unwrap()).unwrap());
    let after_down = rss_kb();
    println!(
        "RSS after 500 MiB download: {after_down} kB (+{} kB)",
        after_down.saturating_sub(baseline)
    );

    assert_eq!(std::fs::metadata(&back).unwrap().len() as usize, SIZE);
    // Generous ceiling: buffering the file whole would add ~500 MiB, streaming adds
    // only the 64 KiB working buffers plus allocator slack.
    let growth = after_down.saturating_sub(baseline);
    assert!(
        growth < 64 * 1024,
        "RSS grew by {growth} kB — the transfer is buffering, not streaming"
    );

    b.delete("/big.bin").unwrap();
}
