//! Local STT catalog, SHA-256 downloads, and whisper.cpp CLI install.
use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
pub struct FileSpec {
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

#[derive(Clone, Copy)]
pub struct Offer {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: Kind,
    pub files: &'static [FileSpec],
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Canary,
    Parakeet,
    Whisper,
}

pub const CANARY: Offer = Offer {
    id: "canary",
    name: "Canary 180M Flash (INT8)",
    kind: Kind::Canary,
    files: &[
        FileSpec {
            name: "encoder-model.int8.onnx",
            url: "https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/92c2231a4e2b2524277fea759be967d2e6edfc49/encoder-model.int8.onnx",
            sha256: "996d1c89e6cbc891a7c88bf410884c178ffa474f7b13084522ac74a5e144cc81",
            bytes: 0,
        },
        FileSpec {
            name: "decoder-model.int8.onnx",
            url: "https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/92c2231a4e2b2524277fea759be967d2e6edfc49/decoder-model.int8.onnx",
            sha256: "9dd9c447872088c912e916d73751f9621a54085d5bc46788454fe904db51a914",
            bytes: 0,
        },
        FileSpec {
            name: "vocab.txt",
            url: "https://huggingface.co/istupakov/canary-180m-flash-onnx/resolve/92c2231a4e2b2524277fea759be967d2e6edfc49/vocab.txt",
            sha256: "2dae6fc7815f9640645e0c765522b278ee0cef49b482d91f6913e334628d3e77",
            bytes: 0,
        },
        FileSpec {
            name: "nemo128.onnx",
            url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/nemo128.onnx",
            sha256: "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f",
            bytes: 139_764,
        },
    ],
};

const PARAKEET_FILES: &[FileSpec] = &[
    FileSpec {
        name: "encoder-model.int8.onnx",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/encoder-model.int8.onnx",
        sha256: "6139d2fa7e1b086097b277c7149725edbab89cc7c7ae64b23c741be4055aff09",
        bytes: 652_183_999,
    },
    FileSpec {
        name: "decoder_joint-model.int8.onnx",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/decoder_joint-model.int8.onnx",
        sha256: "eea7483ee3d1a30375daedc8ed83e3960c91b098812127a0d99d1c8977667a70",
        bytes: 18_202_004,
    },
    FileSpec {
        name: "nemo128.onnx",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/nemo128.onnx",
        sha256: "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f",
        bytes: 139_764,
    },
    FileSpec {
        name: "vocab.txt",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/vocab.txt",
        sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
        bytes: 93_939,
    },
];

pub const PARAKEET: Offer = Offer {
    id: "parakeet",
    name: "Parakeet TDT 0.6B (INT8)",
    kind: Kind::Parakeet,
    files: PARAKEET_FILES,
};

const WHISPER_TINY: &[FileSpec] = &[FileSpec {
    name: "ggml-tiny-q5_1.bin",
    url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny-q5_1.bin",
    sha256: "818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7",
    bytes: 32_152_673,
}];
const WHISPER_BASE: &[FileSpec] = &[FileSpec {
    name: "ggml-base-q5_1.bin",
    url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin",
    sha256: "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898",
    bytes: 59_707_625,
}];
const WHISPER_SMALL: &[FileSpec] = &[FileSpec {
    name: "ggml-small-q5_1.bin",
    url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
    sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb4d02d4e8e87e213d3d8f20abea",
    bytes: 190_085_487,
}];

pub const OFFERS: &[Offer] = &[
    CANARY,
    PARAKEET,
    Offer {
        id: "whisper-tiny",
        name: "Whisper Tiny Q5",
        kind: Kind::Whisper,
        files: WHISPER_TINY,
    },
    Offer {
        id: "whisper-base",
        name: "Whisper Base Q5",
        kind: Kind::Whisper,
        files: WHISPER_BASE,
    },
    Offer {
        id: "whisper-small",
        name: "Whisper Small Q5",
        kind: Kind::Whisper,
        files: WHISPER_SMALL,
    },
];

pub fn get(id: &str) -> Option<&'static Offer> {
    OFFERS.iter().find(|o| o.id == id)
}

pub fn resolve(value: &str) -> Option<&'static str> {
    let n = value.trim().to_lowercase().replace([' ', '_'], "-");
    match n.as_str() {
        "canary" | "flash" => Some("canary"),
        "parakeet" | "parakeet-0.6b" | "parakeet-tdt" | "parakeet-unified" | "unified" => {
            Some("parakeet")
        }
        "whisper-tiny" | "tiny" => Some("whisper-tiny"),
        "whisper-base" | "base" => Some("whisper-base"),
        "whisper-small" | "small" => Some("whisper-small"),
        other => get(other).map(|o| o.id),
    }
}

pub fn dir(assets: &Path, offer: &Offer) -> PathBuf {
    match offer.kind {
        Kind::Canary => assets.join("models/canary-180m-flash"),
        Kind::Parakeet => assets.join("models/parakeet-tdt-0.6b-v3-int8"),
        Kind::Whisper => assets.join("models/whisper"),
    }
}

pub fn installed(assets: &Path, offer: &Offer) -> bool {
    offer
        .files
        .iter()
        .all(|f| dir(assets, offer).join(f.name).is_file())
        && (offer.kind != Kind::Whisper || whisper_cli(assets).is_some())
}

pub fn missing_bytes(assets: &Path, offer: &Offer) -> u64 {
    let mut n = 0;
    for f in offer.files {
        if !dir(assets, offer).join(f.name).is_file() {
            n += if f.bytes == 0 { 80_000_000 } else { f.bytes };
        }
    }
    if offer.kind == Kind::Whisper && whisper_cli(assets).is_none() {
        n += whisper_cli_archive().map(|a| a.bytes).unwrap_or(9_000_000);
    }
    n
}

pub fn size_label(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else {
        format!("{} MB", (bytes + 500_000) / 1_000_000)
    }
}

pub fn confirm_text(assets: &Path, id: &str) -> Result<String> {
    let offer = get(id).context("Unknown local STT model")?;
    if installed(assets, offer) {
        return Ok(String::new());
    }
    Ok(format!(
        "Download {} (~{})? Type yes to start. Esc or /cancel keeps your current model — nothing is downloaded.",
        offer.name,
        size_label(missing_bytes(assets, offer))
    ))
}

pub fn whisper_cli(assets: &Path) -> Option<PathBuf> {
    let exe = assets.join("runtime").join(if cfg!(windows) {
        "whisper-cli.exe"
    } else {
        "whisper-cli"
    });
    exe.is_file().then_some(exe)
}

struct Archive {
    url: &'static str,
    sha256: &'static str,
    bytes: u64,
}

fn whisper_cli_archive() -> Option<Archive> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some(Archive {
            url: "https://github.com/ggml-org/whisper.cpp/releases/download/b5130/whisper-bin-x64.zip",
            sha256: "f9ec6c52a2e949b62ab51fa21d0d497958f9e41c3010c157c4e42932d5316f3c",
            bytes: 8_573_270,
        })
    } else if cfg!(all(windows, target_arch = "aarch64")) {
        Some(Archive {
            url: "https://github.com/ggml-org/whisper.cpp/releases/download/b5130/whisper-bin-win-cpu-arm64.zip",
            sha256: "799543b926ab5b6c2d60cab269a2092e0ae8d27820e9e15429e59de3699546fc",
            bytes: 4_361_895,
        })
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some(Archive {
            url: "https://github.com/ggml-org/whisper.cpp/releases/download/b5130/whisper-bin-ubuntu-x64.tar.gz",
            sha256: "53e7fd8b5764edad916b8848dd0af6abb1ff1d3b86c899e79c78652412536c32",
            bytes: 9_793_438,
        })
    } else {
        None
    }
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    let mut buf = [0u8; 1024 * 256];
    loop {
        let n = std::io::Read::read(&mut file, &mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

async fn fetch(url: &str, dest: &Path, sha256: &str) -> Result<()> {
    if dest.is_file() && sha256_file(dest)? == sha256 {
        return Ok(());
    }
    dest.parent().map(std::fs::create_dir_all).transpose()?;
    let part = dest.with_extension("part");
    let mut response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3600))
        .build()?
        .get(url)
        .header("User-Agent", "Accessor/0.2")
        .send()
        .await?
        .error_for_status()?;
    let mut out = std::fs::File::create(&part)?;
    let mut hasher = Sha256::new();
    while let Some(chunk) = response.chunk().await? {
        hasher.update(&chunk);
        out.write_all(&chunk)?;
    }
    out.flush()?;
    ensure!(
        format!("{:x}", hasher.finalize()) == sha256,
        "Checksum mismatch for {}",
        dest.display()
    );
    let _ = std::fs::remove_file(dest);
    std::fs::rename(part, dest)?;
    Ok(())
}

fn extract_cli(archive: &Path, assets: &Path) -> Result<PathBuf> {
    let unpack = assets.join("runtime/whisper-unpack");
    let _ = std::fs::remove_dir_all(&unpack);
    std::fs::create_dir_all(&unpack)?;
    let status = std::process::Command::new("tar")
        .args(["-xf"])
        .arg(archive)
        .arg("-C")
        .arg(&unpack)
        .status()
        .context("Could not extract whisper-cli (need tar on PATH)")?;
    ensure!(status.success(), "Extracting whisper-cli failed");
    let want = if cfg!(windows) {
        "whisper-cli.exe"
    } else {
        "whisper-cli"
    };
    let found = find_file(&unpack, want).context("whisper-cli missing from archive")?;
    let dest = assets.join("runtime").join(want);
    std::fs::create_dir_all(dest.parent().unwrap())?;
    std::fs::copy(&found, &dest)?;
    let _ = std::fs::remove_dir_all(&unpack);
    Ok(dest)
}

fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == name) {
                return Some(path);
            }
        }
    }
    None
}

pub async fn download(assets: PathBuf, id: &str) -> Result<String> {
    let offer = get(id).context("Unknown local STT model")?;
    let folder = dir(&assets, offer);
    std::fs::create_dir_all(&folder)?;
    for file in offer.files {
        fetch(file.url, &folder.join(file.name), file.sha256).await?;
    }
    if offer.kind == Kind::Whisper && whisper_cli(&assets).is_none() {
        let archive = whisper_cli_archive().context(
            "No pinned whisper-cli build for this OS/CPU. Parakeet still works as a local upgrade.",
        )?;
        let zip = assets.join("runtime").join("whisper-cli.archive");
        fetch(archive.url, &zip, archive.sha256).await?;
        extract_cli(&zip, &assets)?;
    }
    Ok(format!("Installed {}.", offer.name))
}

pub fn whisper_model_file(assets: &Path, id: &str) -> Result<PathBuf> {
    let offer = get(id).context("Unknown Whisper model")?;
    ensure!(offer.kind == Kind::Whisper, "Not a Whisper model");
    Ok(dir(assets, offer).join(offer.files[0].name))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parakeet_is_the_large_local_upgrade() {
        assert!(missing_bytes(Path::new("/nope"), &PARAKEET) > 600_000_000);
        assert!(confirm_text(Path::new("/nope"), "parakeet")
            .unwrap()
            .contains("yes"));
        assert!(get("whisper-tiny").is_some());
        assert_eq!(resolve("Parakeet unified"), Some("parakeet"));
        assert_eq!(resolve("whisper tiny"), Some("whisper-tiny"));
        assert!(get("nope").is_none());
    }
}
