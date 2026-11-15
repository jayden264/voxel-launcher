//! Problems that can be seen before the game starts, from the mod files alone:
//! the same mod twice, and a mod whose required prerequisite is not there.
//!
//! Only Fabric and Quilt metadata is checked for prerequisites: it names
//! exactly what is required, so a report is a fact. Forge-style dependency
//! lists carry version ranges and side flags that are easy to misread, and a
//! false alarm before launch is worse than none.

use crate::config::Dirs;
use crate::mods::{list_installed, ContentKind};
use crate::Result;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek};

/// Provided by the loader or the game itself, never by a mod file. Fabric Loader
/// ships MixinExtras inside itself, so mods like Lithium list it as a
/// requirement that no file in `mods` will ever satisfy.
const BUILTIN: [&str; 7] = ["minecraft", "java", "fabricloader", "fabric-loader", "quilt_loader", "quilt-loader", "mixinextras"];
/// Nested jars are read into memory: skip absurd ones and absurd nesting.
const MAX_NESTED_BYTES: u64 = 32 * 1024 * 1024;
const MAX_DEPTH: u32 = 3;

#[derive(Debug, Clone, Serialize)]
pub struct Problem {
    /// `duplicate-mods` or `missing-dependency`.
    pub code: &'static str,
    pub title: String,
    pub detail: String,
    /// What the UI can offer to do about it: `disable-duplicates` or `mods` (go and install it).
    pub fix: Option<&'static str>,
}

struct Meta {
    id: String,
    name: String,
    provides: Vec<String>,
    depends: Vec<String>,
    nested: Vec<String>,
}

fn read_entry<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str, cap: u64) -> Option<Vec<u8>> {
    let f = zip.by_name(name).ok()?;
    if f.size() > cap {
        return None;
    }
    let mut buf = Vec::new();
    f.take(cap).read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn strings(v: Option<&serde_json::Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|e| e.as_str().or_else(|| e.get("id").and_then(|i| i.as_str())).map(str::to_string)).collect())
        .unwrap_or_default()
}

fn meta_of<R: Read + Seek>(zip: &mut zip::ZipArchive<R>) -> Option<Meta> {
    if let Some(v) = read_entry(zip, "fabric.mod.json", 1 << 20).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) {
        let id = v.get("id")?.as_str()?.to_string();
        return Some(Meta {
            name: v.get("name").and_then(|n| n.as_str()).unwrap_or(&id).to_string(),
            provides: strings(v.get("provides")),
            depends: v.get("depends").and_then(|d| d.as_object()).map(|o| o.keys().cloned().collect()).unwrap_or_default(),
            nested: v.get("jars").and_then(|j| j.as_array()).map(|a| a.iter().filter_map(|e| e.get("file").and_then(|f| f.as_str()).map(str::to_string)).collect()).unwrap_or_default(),
            id,
        });
    }
    let v = read_entry(zip, "quilt.mod.json", 1 << 20).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())?;
    let q = v.get("quilt_loader")?;
    let id = q.get("id")?.as_str()?.to_string();
    // Quilt entries are `"id"` or `{ "id": .., "optional": true }`; optional ones and `unless` ones are not required.
    let depends = q
        .get("depends")
        .and_then(|d| d.as_array())
        .map(|a| {
            a.iter()
                .filter(|e| e.get("optional").and_then(|o| o.as_bool()) != Some(true) && e.get("unless").is_none())
                .filter_map(|e| e.as_str().or_else(|| e.get("id").and_then(|i| i.as_str())).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Some(Meta {
        name: q.pointer("/metadata/name").and_then(|n| n.as_str()).unwrap_or(&id).to_string(),
        provides: strings(q.get("provides")),
        depends,
        nested: strings(q.get("jars")),
        id,
    })
}

/// Every mod id a jar makes available: its own, its aliases and those of the jars bundled inside it.
fn provided_by<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, out: &mut BTreeSet<String>, depth: u32) {
    let Some(meta) = meta_of(zip) else { return };
    out.insert(meta.id.clone());
    out.extend(meta.provides.iter().cloned());
    if depth >= MAX_DEPTH {
        return;
    }
    for file in &meta.nested {
        if let Some(bytes) = read_entry(zip, file, MAX_NESTED_BYTES) {
            if let Ok(mut inner) = zip::ZipArchive::new(Cursor::new(bytes)) {
                provided_by(&mut inner, out, depth + 1);
            }
        }
    }
}

/// What is wrong with the instance's mods, worst first. Never touches the network.
pub fn precheck(dirs: &Dirs, instance_id: &str) -> Result<Vec<Problem>> {
    let installed = list_installed(dirs, instance_id)?;
    let dir = dirs.content_dir(instance_id, ContentKind::Mod)?;
    let mut problems = Vec::new();

    let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for m in installed.iter().filter(|m| m.duplicate) {
        if let Some(id) = &m.mod_id {
            groups.entry(id).or_default().push(&m.filename);
        }
    }
    for (id, files) in groups {
        problems.push(Problem {
            code: "duplicate-mods",
            title: format!("模组「{id}」装了 {} 份", files.len()),
            detail: format!("{}。同一个模组不能同时加载，游戏会直接报错退出。", files.join("、")),
            fix: Some("disable-duplicates"),
        });
    }

    let mut provided = BTreeSet::new();
    let mut metas = Vec::new();
    for m in installed.iter().filter(|m| m.enabled) {
        let Ok(file) = std::fs::File::open(dir.join(&m.filename)) else { continue };
        let Ok(mut zip) = zip::ZipArchive::new(file) else { continue };
        provided_by(&mut zip, &mut provided, 0);
        if let Some(meta) = meta_of(&mut zip) {
            metas.push(meta);
        }
    }
    let mut missing: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for meta in &metas {
        for dep in &meta.depends {
            if !provided.contains(dep) && !BUILTIN.contains(&dep.as_str()) {
                missing.entry(dep.clone()).or_default().insert(meta.name.clone());
            }
        }
    }
    for (dep, users) in missing {
        problems.push(Problem {
            code: "missing-dependency",
            title: format!("缺少前置模组 {dep}"),
            detail: format!("{} 需要它，没有它游戏无法启动。", users.into_iter().collect::<Vec<_>>().join("、")),
            fix: Some("mods"),
        });
    }
    Ok(problems)
}
