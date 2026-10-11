//! Deterministic device-free SVG catalog for browser/native asset adapters.
use std::{error::Error, fs, path::PathBuf};
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let first = args.next().ok_or("output file or --check file required")?;
    let check = first == "--check";
    let path = PathBuf::from(if check {
        args.next().ok_or("check file required")?
    } else {
        first
    });
    if args.next().is_some() {
        return Err("unexpected arguments".into());
    }
    let symbols = fpl_gfx::symbols::NAMES
        .iter()
        .map(|&name| (name, fpl_gfx::symbols::svg(name).expect("catalog outline")))
        .collect::<std::collections::BTreeMap<_, _>>();
    let bytes = serde_json::to_vec_pretty(
        &serde_json::json!({"schema":"fpl.gfx.symbols.svg.v1","symbols":symbols}),
    )?;
    if check {
        if fs::read(&path)? != bytes {
            return Err(format!("GFX symbol drift: {}", path.display()).into());
        }
    } else {
        fs::write(path, bytes)?;
    }
    Ok(())
}
