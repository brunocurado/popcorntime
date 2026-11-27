//! Compile the Swift subtitle translator (Apple Translation framework) and
//! embed it in the library. Without `swiftc` (non-macOS builds) an empty
//! placeholder is embedded and translation reports itself unavailable.

use std::{env, fs, path::PathBuf, process::Command};

fn main() {
  let src = PathBuf::from("swift/pt-translate.swift");
  println!("cargo:rerun-if-changed={}", src.display());
  let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("pt-translate");
  let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
  let built = target_os == "macos"
    && Command::new("swiftc")
      .args(["-parse-as-library", "-O", "-o"])
      .arg(&out)
      .arg(&src)
      .status()
      .map(|s| s.success())
      .unwrap_or(false);
  if !built {
    println!(
      "cargo:warning=pt-translate not built (needs swiftc on macOS); subtitle translation disabled"
    );
    fs::write(&out, b"").unwrap();
  }
}
