//! First-launch model download from the sherpa-onnx GitHub release into models_dir.
use anyhow::{Context, Result};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const BASE: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models";

/// Ensure `<models_dir>/<archive>/` exists, downloading + extracting `<archive>.tar.bz2` if not.
/// `progress(done_bytes, total_bytes)` is called while downloading.
pub fn ensure_archive(models_dir: &Path, archive: &str, progress: &dyn Fn(u64, u64)) -> Result<PathBuf> {
    let dir = models_dir.join(archive);
    if dir.join(".complete").exists() {
        return Ok(dir);
    }
    let tmp = models_dir.join(format!("{archive}.tar.bz2.part"));
    fetch(&format!("{BASE}/{archive}.tar.bz2"), &tmp, progress)?;
    let _ = fs::remove_dir_all(&dir);
    tar::Archive::new(bzip2::read::BzDecoder::new(fs::File::open(&tmp)?))
        .unpack(models_dir)
        .context("extract")?;
    fs::remove_file(&tmp).ok();
    fs::write(dir.join(".complete"), "").context("archive did not contain the expected directory")?;
    Ok(dir)
}

/// Ensure a single release file (e.g. silero_vad.onnx) exists.
pub fn ensure_file(models_dir: &Path, name: &str, progress: &dyn Fn(u64, u64)) -> Result<PathBuf> {
    let p = models_dir.join(name);
    if !p.exists() {
        let tmp = models_dir.join(format!("{name}.part"));
        fetch(&format!("{BASE}/{name}"), &tmp, progress)?;
        fs::rename(&tmp, &p)?;
    }
    Ok(p)
}

fn fetch(url: &str, dest: &Path, progress: &dyn Fn(u64, u64)) -> Result<()> {
    let mut resp = ureq::get(url).call().with_context(|| format!("GET {url}"))?;
    let total = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok()?.parse().ok())
        .unwrap_or(0);
    let mut reader = resp.body_mut().as_reader();
    let mut file = fs::File::create(dest)?;
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        progress(done, total);
    }
    Ok(())
}
