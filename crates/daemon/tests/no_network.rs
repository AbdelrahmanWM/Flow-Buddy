//! The daemon reads keys, so it makes no network connections: no network
//! types in its code, and no HTTP client among its dependencies. The AI
//! features live in the CLI, which you run yourself.

use std::path::{Path, PathBuf};

const NETWORK: &[&str] = &[
    "std::net",
    "TcpStream",
    "TcpListener",
    "UdpSocket",
    "ToSocketAddrs",
    "tokio::net::Tcp",
    "tokio::net::Udp",
];

const CLIENTS: &[&str] = &[
    "ureq",
    "reqwest",
    "hyper",
    "curl",
    "isahc",
    "surf",
    "attohttpc",
];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_network_code() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    // The daemon and the crates it's built from.
    for dir in ["src", "../core/src", "../store/src"] {
        sources(&root.join(dir), &mut files);
    }
    assert!(files.len() > 10);
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        for word in NETWORK {
            assert!(!text.contains(word), "{} uses {word}", f.display());
        }
    }
}

#[test]
fn no_http_client() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for manifest in ["Cargo.toml", "../core/Cargo.toml", "../store/Cargo.toml"] {
        let text = std::fs::read_to_string(root.join(manifest)).unwrap();
        for client in CLIENTS {
            let dep = format!("\n{client} =");
            let workspace = format!("\n{client}.workspace");
            assert!(
                !text.contains(&dep) && !text.contains(&workspace),
                "{manifest} depends on {client}"
            );
        }
    }
}
