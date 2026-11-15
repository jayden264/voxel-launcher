//! Parsing `install_profile.json` and resolving its placeholders, using the real
//! profiles of the official installers.

use launcher_core::forge::{parse_profile, DataEntry, DataValue, InstallProfile, Resolver};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const FORGE_PROFILE: &str = include_str!("fixtures/forge-1.21.1-52.1.0.install_profile.json");
const NEO_PROFILE: &str = include_str!("fixtures/neoforge-21.1.172.install_profile.json");
const FORGE_1122: &str = include_str!("fixtures/forge-1.12.2-14.23.5.2860.install_profile.json");
const FORGE_1710: &str = include_str!("fixtures/forge-1.7.10-10.13.4.1614.install_profile.json");

fn unix(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn resolver() -> Resolver {
    Resolver::new(
        Path::new("/r"),
        Path::new("/r/installers/forge-installer.jar"),
        Path::new("/r/versions/1.21.1/1.21.1.jar"),
        "1.21.1",
        Path::new("/r/libraries"),
        Path::new("/r/tmp/extract"),
    )
}

fn entry(client: &str) -> DataEntry {
    DataEntry { client: client.into(), server: "unused".into() }
}

#[test]
fn modern_forge_profile_parses_processors_and_libraries() {
    let InstallProfile::Modern { minecraft, libraries, data, processors, version_json_entry } =
        parse_profile(FORGE_PROFILE.as_bytes()).unwrap()
    else {
        panic!("expected a modern profile")
    };
    assert_eq!(minecraft, "1.21.1");
    assert_eq!(libraries.len(), 21);
    assert_eq!(processors.len(), 7);
    assert_eq!(version_json_entry, "version.json");
    assert!(data.contains_key("BINPATCH") && data.contains_key("PATCHED"));
    let lzma = libraries.iter().find(|l| l.name == "com.github.jponge:lzma-java:1.3").unwrap();
    assert_eq!(lzma.path, "com/github/jponge/lzma-java/1.3/lzma-java-1.3.jar");
    assert!(lzma.url.starts_with("https://maven.minecraftforge.net/") && lzma.sha1.is_some());
    let last = processors.last().unwrap();
    assert_eq!(last.jar, "net.minecraftforge:binarypatcher:1.2.0");
    assert_eq!(last.outputs.get("{PATCHED}").map(String::as_str), Some("{PATCHED_SHA}"));
}

#[test]
fn neoforge_profile_parses_ten_processors() {
    let InstallProfile::Modern { libraries, processors, .. } = parse_profile(NEO_PROFILE.as_bytes()).unwrap() else {
        panic!("expected a modern profile")
    };
    assert_eq!(libraries.len(), 70);
    assert_eq!(processors.len(), 10);
}

#[test]
fn forge_1_12_2_is_modern_with_no_processors_and_an_embedded_library() {
    let InstallProfile::Modern { libraries, processors, data, .. } = parse_profile(FORGE_1122.as_bytes()).unwrap() else {
        panic!("expected a modern profile")
    };
    assert!(processors.is_empty() && data.is_empty());
    let embedded: Vec<_> = libraries.iter().filter(|l| l.url.is_empty()).map(|l| l.name.as_str()).collect();
    assert_eq!(embedded, vec!["net.minecraftforge:forge:1.12.2-14.23.5.2860"]);
}

#[test]
fn forge_1_7_10_is_legacy() {
    let InstallProfile::Legacy { version_info, universal_entry, universal_coord, minecraft } =
        parse_profile(FORGE_1710.as_bytes()).unwrap()
    else {
        panic!("expected a legacy profile")
    };
    assert_eq!(minecraft, "1.7.10");
    assert_eq!(universal_entry, "forge-1.7.10-10.13.4.1614-1.7.10-universal.jar");
    assert_eq!(universal_coord, "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10");
    assert_eq!(version_info["inheritsFrom"], "1.7.10");
    assert_eq!(version_info["mainClass"], "net.minecraft.launchwrapper.Launch");
}

#[test]
fn only_client_processors_run() {
    use launcher_core::forge::runs_on_client;
    let count = |profile: &str| {
        let InstallProfile::Modern { processors, .. } = parse_profile(profile.as_bytes()).unwrap() else { panic!() };
        processors.iter().filter(|p| runs_on_client(p)).count()
    };
    // Forge: server-only steps are skipped; 3 of 7 run for the client.
    assert_eq!(count(FORGE_PROFILE), 3);
    assert_eq!(count(NEO_PROFILE), 6);
}

#[test]
fn data_values_have_three_forms() {
    let r = resolver();
    match r.data_value(&entry("[net.minecraft:client:1.21.1:mappings@tsrg]")).unwrap() {
        DataValue::Path(p) => assert_eq!(unix(&p), "/r/libraries/net/minecraft/client/1.21.1/client-1.21.1-mappings.tsrg"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(r.data_value(&entry("'2a8064cf'")).unwrap(), DataValue::Literal(s) if s == "2a8064cf"));
    assert!(matches!(r.data_value(&entry("/data/client.lzma")).unwrap(), DataValue::InstallerFile(e) if e == "data/client.lzma"));
}

#[test]
fn coordinates_resolve_into_the_library_directory() {
    let r = resolver();
    assert_eq!(
        unix(&r.coord_path("net.minecraftforge:binarypatcher:1.2.0").unwrap()),
        "/r/libraries/net/minecraftforge/binarypatcher/1.2.0/binarypatcher-1.2.0.jar"
    );
    assert_eq!(
        unix(&r.coord_path("net.neoforged.installertools:binarypatcher:2.1.2:fatjar").unwrap()),
        "/r/libraries/net/neoforged/installertools/binarypatcher/2.1.2/binarypatcher-2.1.2-fatjar.jar"
    );
    assert!(r.coord_path("not-a-coordinate").is_err());
}

#[test]
fn arguments_expand_builtins_data_and_coordinates() {
    let InstallProfile::Modern { data, .. } = parse_profile(FORGE_PROFILE.as_bytes()).unwrap() else { panic!() };
    let r = resolver();
    let resolved = r.variables(&data).unwrap();
    let x = |a: &str| unix(Path::new(&r.expand_arg(a, &resolved.vars).unwrap()));
    assert_eq!(x("{MINECRAFT_JAR}"), "/r/versions/1.21.1/1.21.1.jar");
    assert_eq!(x("{ROOT}/README.txt"), "/r/README.txt");
    assert_eq!(x("{SIDE}"), "client");
    assert_eq!(x("{MINECRAFT_VERSION}"), "1.21.1");
    assert_eq!(x("{BINPATCH}"), "/r/tmp/extract/data/client.lzma");
    assert_eq!(x("{PATCHED}"), "/r/libraries/net/minecraftforge/forge/1.21.1-52.1.0/forge-1.21.1-52.1.0-client.jar");
    assert_eq!(x("{PATCHED_SHA}"), "2d2537d306a05aeac6ed5fa9bf176ffacb78cb4f");
    assert_eq!(x("[net.minecraftforge:forge:1.21.1-52.1.0:client]"), "/r/libraries/net/minecraftforge/forge/1.21.1-52.1.0/forge-1.21.1-52.1.0-client.jar");
    assert_eq!(x("--task"), "--task", "plain arguments pass through");
    assert_eq!(resolved.installer_entries, vec!["data/client.lzma"], "only the client-side installer file is needed");
}

#[test]
fn unknown_placeholders_are_errors_naming_the_variable() {
    let r = resolver();
    let err = r.expand_arg("{NOPE}/x", &BTreeMap::new()).unwrap_err().to_string();
    assert!(err.contains("NOPE"), "{err}");
}

#[test]
fn unsafe_paths_from_the_profile_or_data_are_rejected() {
    let evil = FORGE_PROFILE.replace("com/github/jponge/lzma-java/1.3/lzma-java-1.3.jar", "../evil.jar");
    assert_ne!(evil, FORGE_PROFILE, "fixture shape changed");
    assert!(parse_profile(evil.as_bytes()).is_err());

    let r = resolver();
    assert!(r.data_value(&entry("/../evil")).is_err());
    assert!(r.data_value(&entry("/C:/evil")).is_err());
    let _ = PathBuf::new();
}
