//! The consoles the Retro shelf knows (design canvas: Games · Retro), and which emulator runs each.

use super::emu::Emu;

pub struct System {
    pub id: &'static str,
    pub name: &'static str,
    pub short: &'static str,
    pub color: &'static str,
    pub emu: Emu,
    /// RetroArch core, without `_libretro.dll`.
    pub core: Option<&'static str>,
    /// Game files, lower case without the dot.
    pub exts: &'static [&'static str],
    /// Folder names (lower case, letters and digits only) that hold this console's games, for
    /// spotting the console folders inside a "roms" folder.
    pub aliases: &'static [&'static str],
    /// The console's folder on thumbnails.libretro.com.
    pub thumbs: &'static str,
    pub bios: Bios,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Bios {
    None,
    /// Dumped from a PlayStation: the core looks for it in RetroArch's system folder.
    Ps1,
    /// Dumped from a PlayStation 2, in PCSX2's bios folder.
    Ps2,
}

pub const SYSTEMS: &[System] = &[
    System {
        id: "n64",
        name: "Nintendo 64",
        short: "N64",
        color: "#2F9E44",
        emu: Emu::Retroarch,
        core: Some("mupen64plus_next"),
        exts: &["z64", "n64", "v64", "zip", "7z"],
        aliases: &["n64", "nintendo64"],
        thumbs: "Nintendo - Nintendo 64",
        bios: Bios::None,
    },
    System {
        id: "ps1",
        name: "PlayStation",
        short: "PS1",
        color: "#7C8696",
        emu: Emu::Retroarch,
        core: Some("swanstation"),
        // A .bin is listed through its .cue, and discs through their .m3u (scan.rs).
        exts: &["chd", "cue", "m3u", "pbp"],
        aliases: &["ps1", "psx", "playstation", "playstation1", "psone"],
        thumbs: "Sony - PlayStation",
        bios: Bios::Ps1,
    },
    System {
        id: "ps2",
        name: "PlayStation 2",
        short: "PS2",
        color: "#1C7ED6",
        emu: Emu::Pcsx2,
        core: None,
        exts: &["iso", "chd", "cso", "zso"],
        aliases: &["ps2", "playstation2"],
        thumbs: "Sony - PlayStation 2",
        bios: Bios::Ps2,
    },
    System {
        id: "gc",
        name: "GameCube",
        short: "GC",
        color: "#7048E8",
        emu: Emu::Dolphin,
        core: None,
        exts: &["rvz", "iso", "gcm", "gcz", "ciso"],
        aliases: &["gc", "gamecube", "ngc", "gcn"],
        thumbs: "Nintendo - GameCube",
        bios: Bios::None,
    },
    System {
        id: "md",
        name: "Mega Drive",
        short: "MD",
        color: "#E03131",
        emu: Emu::Retroarch,
        core: Some("genesis_plus_gx"),
        exts: &["md", "gen", "smd", "bin", "zip", "7z"],
        aliases: &["md", "megadrive", "genesis", "segagenesis"],
        thumbs: "Sega - Mega Drive - Genesis",
        bios: Bios::None,
    },
    System {
        id: "snes",
        name: "Super Nintendo",
        short: "SNES",
        color: "#F08C00",
        emu: Emu::Retroarch,
        core: Some("snes9x"),
        exts: &["sfc", "smc", "zip", "7z"],
        aliases: &["snes", "supernintendo", "sfc", "superfamicom"],
        thumbs: "Nintendo - Super Nintendo Entertainment System",
        bios: Bios::None,
    },
    System {
        id: "gba",
        name: "Game Boy Advance",
        short: "GBA",
        color: "#0CA678",
        emu: Emu::Retroarch,
        core: Some("mgba"),
        exts: &["gba", "zip", "7z"],
        aliases: &["gba", "gameboyadvance"],
        thumbs: "Nintendo - Game Boy Advance",
        bios: Bios::None,
    },
];

pub fn get(id: &str) -> Option<&'static System> {
    SYSTEMS.iter().find(|s| s.id == id)
}

/// The console a folder named `name` holds, by its usual names ("PSX", "Mega Drive", "n64").
pub fn by_folder_name(name: &str) -> Option<&'static System> {
    let key: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    SYSTEMS.iter().find(|s| s.aliases.contains(&key.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names() {
        assert_eq!(by_folder_name("Mega Drive").unwrap().id, "md");
        assert_eq!(by_folder_name("PSX").unwrap().id, "ps1");
        assert_eq!(by_folder_name("ps2").unwrap().id, "ps2");
        assert_eq!(by_folder_name("Nintendo 64").unwrap().id, "n64");
        assert!(by_folder_name("ferramentas").is_none());
    }

    #[test]
    fn retroarch_consoles_have_cores() {
        for s in SYSTEMS {
            assert_eq!(s.core.is_some(), s.emu == Emu::Retroarch, "{}", s.id);
        }
    }
}
