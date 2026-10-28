//! BIOS files, found by name and size; never downloaded. The PlayStation's goes in RetroArch's
//! system folder (swanstation finds it there), the PlayStation 2's in PCSX2's bios folder.

use std::fs;
use std::path::Path;

/// The PlayStation BIOS names cores look for, the North American and Brazilian (SCPH-5501)
/// one first.
const PS1_NAMES: &[&str] = &[
    "scph5501.bin",
    "scph5500.bin",
    "scph5502.bin",
    "scph1001.bin",
    "scph7001.bin",
    "scph7502.bin",
    "scph101.bin",
    "scph1000.bin",
    "scph7003.bin",
];
const PS1_SIZE: u64 = 512 * 1024;
/// A PlayStation 2 BIOS dump (SCPH-xxxxx …BIN) is a 4 MiB ROM.
const PS2_SIZE: u64 = 4 * 1024 * 1024;

fn files(dir: &Path) -> Vec<(String, u64)> {
    fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .map(|e| (e.file_name().to_string_lossy().into_owned(), e.metadata().map_or(0, |m| m.len())))
                .collect()
        })
        .unwrap_or_default()
}

fn is_bin(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".bin") || n.ends_with(".rom0") || !n.contains('.')
}

/// The PlayStation BIOS in `dir`, by its known name or, failing that, its size.
pub fn ps1_in(dir: &Path) -> Option<String> {
    let all = files(dir);
    let sized = |n: &(String, u64)| n.1 == PS1_SIZE;
    PS1_NAMES
        .iter()
        .find_map(|want| all.iter().filter(|f| sized(f)).find(|(n, _)| n.eq_ignore_ascii_case(want)))
        .or_else(|| all.iter().filter(|f| sized(f)).find(|(n, _)| n.to_ascii_lowercase().ends_with(".bin")))
        .map(|(n, _)| n.clone())
}

/// A PlayStation 2 BIOS in `dir`.
pub fn ps2_in(dir: &Path) -> Option<String> {
    files(dir).into_iter().find(|(n, size)| *size == PS2_SIZE && is_bin(n)).map(|(n, _)| n)
}

/// Every BIOS file in `dir`, named for the picker: "scph5501.bin (PlayStation)".
pub fn files_in(dir: &Path) -> Vec<String> {
    files(dir)
        .into_iter()
        .filter_map(|(n, size)| match size {
            PS1_SIZE if is_bin(&n) => Some(format!("{n} (PlayStation)")),
            PS2_SIZE if is_bin(&n) => Some(format!("{n} (PlayStation 2)")),
            _ => None,
        })
        .collect()
}

/// Copies the BIOS files `from` has into the emulators' folders that lack them.
pub fn copy_missing(from: &Path, ps1_to: Option<&Path>, ps2_to: Option<&Path>) {
    let copy = |name: String, to: &Path| {
        let _ = fs::create_dir_all(to);
        if let Err(e) = fs::copy(from.join(&name), to.join(&name)) {
            eprintln!("copying BIOS {name} failed: {e}");
        }
    };
    if let Some(to) = ps1_to.filter(|t| ps1_in(t).is_none()) {
        // Copy under the name cores look for first.
        if let Some(n) = ps1_in(from) {
            copy(n, to);
        }
    }
    if let Some(to) = ps2_to.filter(|t| ps2_in(t).is_none()) {
        for (n, size) in files(from) {
            if size == PS2_SIZE && is_bin(&n) {
                copy(n, to);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_by_name_and_size() {
        let dir = std::env::temp_dir().join(format!("tvl-bios-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SCPH5501.BIN"), vec![0u8; PS1_SIZE as usize]).unwrap();
        fs::write(dir.join("scph5500.bin"), b"too small").unwrap();
        fs::write(dir.join("SCPH-70012_BIOS_V12_USA_200.BIN"), vec![0u8; PS2_SIZE as usize]).unwrap();
        assert_eq!(ps1_in(&dir).as_deref(), Some("SCPH5501.BIN"));
        assert_eq!(ps2_in(&dir).as_deref(), Some("SCPH-70012_BIOS_V12_USA_200.BIN"));
        assert_eq!(files_in(&dir).len(), 2);
        let to = dir.join("pcsx2-bios");
        copy_missing(&dir, None, Some(&to));
        assert!(ps2_in(&to).is_some());
        let _ = fs::remove_dir_all(&dir);
    }
}
