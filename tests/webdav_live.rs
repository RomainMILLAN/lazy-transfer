//! Live WebDAV integration checks.
//!
//! Ignored by default (they need a server), so `cargo test` stays offline.
//! To run them:
//!
//! ```sh
//! docker run -d --name lt-dav -p 18080:80 \
//!   -e AUTH_TYPE=Basic -e USERNAME=alice -e PASSWORD=s3cret bytemark/webdav
//! cargo test --test webdav_live -- --ignored --test-threads=1
//! ```
//!
//! The image is Apache mod_dav, which is what makes these worth running: it is the
//! server that answers 301 for a collection addressed without its trailing slash.

use lazy_transfer::transfer::backend::RemoteBackend;
use lazy_transfer::transfer::stream::StreamHandle;
use lazy_transfer::transfer::types::{WebDavAuth, WebDavConfig};
use lazy_transfer::transfer::webdav_backend::WebDavBackend;

const URL: &str = "http://localhost:18080/";

fn backend() -> WebDavBackend {
    let cfg = WebDavConfig {
        url: URL.to_string(),
        auth: WebDavAuth::Basic {
            user: "alice".to_string(),
            password: "s3cret".to_string(),
        },
        insecure_tls: false,
    };
    WebDavBackend::connect(&cfg).expect("connect")
}

/// Drains a transfer, returning the percentages seen and the terminal error.
fn drain(h: StreamHandle) -> (Vec<String>, Option<String>) {
    let mut pcts = Vec::new();
    let mut err = None;
    for line in h.rx.iter() {
        if line.done {
            err = line.err;
            break;
        }
        if !line.text.is_empty() {
            pcts.push(line.text);
        }
    }
    (pcts, err)
}

#[test]
#[ignore]
fn wrong_password_fails_at_connect() {
    let cfg = WebDavConfig {
        url: URL.to_string(),
        auth: WebDavAuth::Basic {
            user: "alice".to_string(),
            password: "wrong".to_string(),
        },
        insecure_tls: false,
    };
    let err = match WebDavBackend::connect(&cfg) {
        Ok(_) => panic!("a wrong password must fail at connect, not later"),
        Err(e) => e,
    };
    assert!(
        err.contains("401") || err.to_lowercase().contains("authentification"),
        "unhelpful message: {err}"
    );
}

#[test]
#[ignore]
fn bad_root_is_reported_clearly() {
    let cfg = WebDavConfig {
        url: "http://localhost:18080/definitely/not/dav/".to_string(),
        auth: WebDavAuth::Basic {
            user: "alice".to_string(),
            password: "s3cret".to_string(),
        },
        insecure_tls: false,
    };
    let err = match WebDavBackend::connect(&cfg) {
        Ok(_) => panic!("a non-WebDAV URL must be rejected"),
        Err(e) => e,
    };
    assert!(!err.is_empty(), "{err}");
}

#[test]
#[ignore]
fn full_file_lifecycle() {
    let b = backend();
    assert_eq!(b.home_dir().unwrap(), "/");

    let dir = "/lt-lifecycle";
    let _ = b.delete(dir);
    b.mkdir(dir).expect("mkdir");

    // Names with a space, an accent, a '#' and a '+' — the encoding traps.
    let tmp = tempfile::tempdir().unwrap();
    let local = tmp.path().join("héllo +wörld #1.txt");
    std::fs::write(&local, b"hello webdav").unwrap();

    let remote = format!("{dir}/héllo +wörld #1.txt");
    let (_, err) = drain(b.upload(local.to_str().unwrap(), &remote).unwrap());
    assert!(err.is_none(), "upload: {err:?}");

    let listed = b.list_dir(dir).expect("list");
    let names: Vec<&str> = listed.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&".."), "parent entry missing: {names:?}");
    assert!(
        names.contains(&"héllo +wörld #1.txt"),
        "encoded name round-trip failed: {names:?}"
    );
    let entry = listed
        .iter()
        .find(|e| e.name == "héllo +wörld #1.txt")
        .unwrap();
    assert_eq!(entry.size, 12);
    assert!(!entry.is_dir);
    assert!(!entry.modified.is_empty(), "no date parsed");
    assert_eq!(entry.permissions, "", "WebDAV must not fake POSIX bits");

    // Rename, then download and compare.
    let renamed = format!("{dir}/renamed.txt");
    b.rename(&remote, &renamed).expect("rename file");
    let out = tmp.path().join("back.txt");
    let (_, err) = drain(b.download(&renamed, out.to_str().unwrap()).unwrap());
    assert!(err.is_none(), "download: {err:?}");
    assert_eq!(std::fs::read(&out).unwrap(), b"hello webdav");

    b.delete(&renamed).expect("delete file");
    assert!(!b
        .list_dir(dir)
        .unwrap()
        .iter()
        .any(|e| e.name == "renamed.txt"));

    b.delete(dir).expect("delete dir");
}

/// The regression this exists for: a collection addressed without a trailing slash
/// gets a 301 from mod_dav, and both delete and rename must retry as a collection.
#[test]
#[ignore]
fn delete_and_rename_work_on_directories() {
    let b = backend();
    let dir = "/lt-dirops";
    let _ = b.delete(dir);
    let _ = b.delete("/lt-dirops-renamed");

    b.mkdir(dir).unwrap();
    b.mkdir(&format!("{dir}/inner")).unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("x.txt");
    std::fs::write(&f, b"x").unwrap();
    drain(
        b.upload(f.to_str().unwrap(), &format!("{dir}/inner/x.txt"))
            .unwrap(),
    );

    // Rename a non-empty directory.
    b.rename(dir, "/lt-dirops-renamed")
        .expect("rename a collection");
    assert!(b.list_dir("/lt-dirops-renamed").is_ok());

    // Delete a non-empty directory: the server recurses.
    b.delete("/lt-dirops-renamed")
        .expect("delete a non-empty collection");
    assert!(!b
        .list_dir("/")
        .unwrap()
        .iter()
        .any(|e| e.name == "lt-dirops-renamed"));
}

#[test]
#[ignore]
fn directory_transfers_report_progress_by_bytes() {
    let b = backend();
    let remote_root = "/lt-tree";
    let _ = b.delete(remote_root);

    // One big file plus many small ones: the case where per-file progress lies.
    let tmp = tempfile::tempdir().unwrap();
    let tree = tmp.path().join("tree");
    std::fs::create_dir_all(tree.join("a/b")).unwrap();
    std::fs::write(tree.join("big.bin"), vec![7u8; 512 * 1024]).unwrap();
    for i in 0..20 {
        std::fs::write(tree.join(format!("a/small{i}.txt")), b"s").unwrap();
    }
    std::fs::write(tree.join("a/b/deep.txt"), b"deep").unwrap();

    let (pcts, err) = drain(b.upload_dir(tree.to_str().unwrap(), "/").unwrap());
    assert!(err.is_none(), "upload_dir: {err:?}");
    // The real property: progress follows BYTES, not file count. The 512 KiB file is
    // ~96% of the payload but only 1 of 21 files, so it must carry the bar most of
    // the way up before the 20 small files finish it. Per-file progress would have
    // shown ~5% here.
    let nums: Vec<u32> = pcts
        .iter()
        .filter_map(|p| p.trim_end_matches('%').parse().ok())
        .collect();
    assert!(!nums.is_empty(), "no progress at all: {pcts:?}");
    assert!(
        nums.windows(2).all(|w| w[0] < w[1]),
        "progress must be monotonic: {pcts:?}"
    );
    assert_eq!(nums.last().copied(), Some(100), "{pcts:?}");
    assert!(
        nums.iter().any(|&n| n >= 90) && nums.len() >= 3,
        "the big file should dominate the bar: {pcts:?}"
    );

    let listed = b.list_dir("/tree").expect("uploaded tree listed");
    let names: Vec<&str> = listed.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"big.bin"), "{names:?}");
    assert!(names.contains(&"a"), "{names:?}");

    // ...and back down, which is where the FTP backend emits nothing at all.
    let dest = tmp.path().join("down");
    std::fs::create_dir_all(&dest).unwrap();
    let (pcts, err) = drain(b.download_dir("/tree", dest.to_str().unwrap()).unwrap());
    assert!(err.is_none(), "download_dir: {err:?}");
    assert!(
        pcts.iter().any(|p| p.ends_with('%')),
        "download_dir emitted no percentage: {pcts:?}"
    );
    assert_eq!(
        std::fs::read(dest.join("tree/big.bin")).unwrap().len(),
        512 * 1024
    );
    assert_eq!(
        std::fs::read(dest.join("tree/a/b/deep.txt")).unwrap(),
        b"deep"
    );

    b.delete("/tree").unwrap();
}

/// An interrupted directory upload must name where it stopped, and a rerun must
/// converge (MKCOL 405 counts as success, PUT overwrites).
#[test]
#[ignore]
fn interrupted_upload_dir_is_resumable() {
    let b = backend();
    let _ = b.delete("/lt-resume");
    let tmp = tempfile::tempdir().unwrap();
    let tree = tmp.path().join("lt-resume");
    std::fs::create_dir_all(tree.join("sub")).unwrap();
    std::fs::write(tree.join("one.txt"), b"1").unwrap();
    std::fs::write(tree.join("sub/two.txt"), b"2").unwrap();

    for _ in 0..2 {
        let (_, err) = drain(b.upload_dir(tree.to_str().unwrap(), "/").unwrap());
        assert!(err.is_none(), "rerun must converge: {err:?}");
    }
    assert_eq!(
        b.list_dir("/lt-resume/sub").unwrap().len(),
        2, // ".." + two.txt
    );
    b.delete("/lt-resume").unwrap();
}

#[test]
#[ignore]
fn tar_mode_is_reported_as_unsupported() {
    let b = backend();
    assert!(b.upload_tar("/tmp/x", "/y").is_err());
    assert!(b.download_tar("/x", "/tmp/y").is_err());
}
