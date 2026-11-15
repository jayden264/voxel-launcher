//! Download source handling: official Mojang hosts vs the BMCLAPI mirror.
//!
//! Mapping table verified against bmclapi2.bangbang93.com with HEAD requests.
//! A mirror URL is always followed by the original URL as a fallback.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    #[default]
    Mirror,
    Official,
}

const MIRROR: &str = "https://bmclapi2.bangbang93.com";

/// (official prefix, mirror prefix)
const MAP: &[(&str, &str)] = &[
    ("https://launchermeta.mojang.com", MIRROR),
    ("https://piston-meta.mojang.com", MIRROR),
    ("https://piston-data.mojang.com", MIRROR),
    ("https://launcher.mojang.com", MIRROR),
    ("https://libraries.minecraft.net", "https://bmclapi2.bangbang93.com/maven"),
    ("https://resources.download.minecraft.net", "https://bmclapi2.bangbang93.com/assets"),
    ("https://meta.fabricmc.net", "https://bmclapi2.bangbang93.com/fabric-meta"),
    ("https://maven.fabricmc.net", "https://bmclapi2.bangbang93.com/maven"),
    ("https://meta.quiltmc.org", "https://bmclapi2.bangbang93.com/quilt-meta"),
    // maven.quiltmc.org: mirror path unverified, intentionally official-only.
    // Forge / NeoForge maven (checked: installers, .sha1 sidecars, metadata, libraries).
    ("https://maven.minecraftforge.net", "https://bmclapi2.bangbang93.com/maven"),
    ("https://maven.neoforged.net/releases", "https://bmclapi2.bangbang93.com/maven"),
];

/// Rewrite a URL to the mirror if a mapping exists.
pub fn to_mirror(url: &str) -> Option<String> {
    for (official, mirror) in MAP {
        if let Some(rest) = url.strip_prefix(official) {
            if rest.is_empty() || rest.starts_with('/') {
                return Some(format!("{mirror}{rest}"));
            }
        }
    }
    None
}

/// Ordered candidate URLs to try: mirror first (if enabled and mapped), then official.
pub fn candidates(url: &str, source: Source) -> Vec<String> {
    let mut v = Vec::with_capacity(2);
    if source == Source::Mirror {
        if let Some(m) = to_mirror(url) {
            v.push(m);
        }
    }
    v.push(url.to_string());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_hosts_and_subpaths() {
        assert_eq!(
            to_mirror("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json").unwrap(),
            "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json"
        );
        assert_eq!(
            to_mirror("https://libraries.minecraft.net/org/ow2/asm/asm/9.6/asm-9.6.jar").unwrap(),
            "https://bmclapi2.bangbang93.com/maven/org/ow2/asm/asm/9.6/asm-9.6.jar"
        );
        assert_eq!(
            to_mirror("https://resources.download.minecraft.net/bd/bdf4").unwrap(),
            "https://bmclapi2.bangbang93.com/assets/bd/bdf4"
        );
        assert_eq!(
            to_mirror("https://meta.fabricmc.net/v2/versions/loader/1.21.1").unwrap(),
            "https://bmclapi2.bangbang93.com/fabric-meta/v2/versions/loader/1.21.1"
        );
        assert_eq!(
            to_mirror("https://piston-data.mojang.com/v1/objects/abc/client.jar").unwrap(),
            "https://bmclapi2.bangbang93.com/v1/objects/abc/client.jar"
        );
    }

    #[test]
    fn forge_and_neoforge_maven_are_mirrored() {
        assert_eq!(
            to_mirror("https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml").unwrap(),
            "https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge/maven-metadata.xml"
        );
        assert_eq!(
            to_mirror("https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.172/neoforge-21.1.172-installer.jar").unwrap(),
            "https://bmclapi2.bangbang93.com/maven/net/neoforged/neoforge/21.1.172/neoforge-21.1.172-installer.jar"
        );
        assert!(to_mirror("https://maven.minecraftforge.net.evil.com/x").is_none());
    }

    #[test]
    fn unmapped_and_lookalike_hosts_are_left_alone() {
        assert!(to_mirror("https://maven.quiltmc.org/repository/release/x").is_none());
        assert!(to_mirror("https://libraries.minecraft.net.evil.com/x").is_none());
        assert!(to_mirror("https://example.com/").is_none());
    }

    #[test]
    fn candidates_fall_back_to_official() {
        let u = "https://libraries.minecraft.net/a/b.jar";
        let c = candidates(u, Source::Mirror);
        assert_eq!(c.len(), 2);
        assert_eq!(c[1], u);
        assert_eq!(candidates(u, Source::Official), vec![u.to_string()]);
        assert_eq!(candidates("https://example.com/x", Source::Mirror).len(), 1);
    }
}
