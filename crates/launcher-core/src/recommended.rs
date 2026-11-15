//! Mods worth offering when an instance is created, so a new Fabric instance
//! is usable without a trip to the mod page.

use crate::loader::LoaderKind;
use crate::mods::{plan_install, target_for, InstallPlan, ModrinthClient, Target};
use crate::Result;
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Recommended {
    /// Modrinth slug.
    pub slug: &'static str,
    pub title: &'static str,
    pub why: &'static str,
    /// Pre-ticked in the create dialog.
    pub default_on: bool,
}

const fn rec(slug: &'static str, title: &'static str, why: &'static str, default_on: bool) -> Recommended {
    Recommended { slug, title, why, default_on }
}

const FABRIC_API: Recommended = rec("fabric-api", "Fabric API", "绝大多数 Fabric 模组都需要它", true);
const SODIUM: Recommended = rec("sodium", "Sodium", "大幅提升帧数", false);
const LITHIUM: Recommended = rec("lithium", "Lithium", "优化游戏逻辑，减少卡顿", false);
const MOD_MENU: Recommended = rec("modmenu", "Mod Menu", "在游戏里查看和管理已装模组", false);
const IRIS: Recommended = rec("iris", "Iris Shaders", "使用光影包（会自动带上 Sodium）", false);

const FABRIC_LIKE: [Recommended; 5] = [FABRIC_API, SODIUM, LITHIUM, MOD_MENU, IRIS];
const NEOFORGE: [Recommended; 3] = [SODIUM, LITHIUM, IRIS];

pub fn recommended_for(kind: LoaderKind) -> &'static [Recommended] {
    match kind {
        LoaderKind::Fabric | LoaderKind::Quilt => &FABRIC_LIKE,
        LoaderKind::NeoForge => &NEOFORGE,
        LoaderKind::Forge => &[],
    }
}

/// The recommendations that have a build for `mc` on this loader. A request
/// that fails just drops its entry: the list is a convenience, never an error.
pub async fn available(client: &ModrinthClient, kind: LoaderKind, mc: &str) -> Vec<Recommended> {
    let t = target_for(mc, kind);
    let checks = recommended_for(kind).iter().map(|r| async {
        match client.project_versions(r.slug, &t).await {
            Ok(v) if !v.is_empty() => Some(*r),
            _ => None,
        }
    });
    futures::future::join_all(checks).await.into_iter().flatten().collect()
}

/// One plan for several chosen mods. Shared dependencies are planned once and
/// already installed projects are skipped; every chosen mod ends up marked as
/// chosen, even when it was first pulled in as another mod's dependency.
pub async fn plan_many(client: &ModrinthClient, t: &Target, slugs: &[String], installed: &HashSet<String>) -> Result<InstallPlan> {
    let mut plan = InstallPlan::default();
    let mut known: HashSet<String> = installed.clone();
    for slug in slugs {
        match plan_install(client, t, slug, &known).await {
            Ok(sub) => {
                for f in sub.files {
                    known.insert(f.project_id.clone());
                    plan.total_bytes += f.size;
                    plan.files.push(f);
                }
                plan.warnings.extend(sub.warnings);
            }
            // Already planned as a dependency of an earlier choice (or installed): nothing to add,
            // but a planned one is now explicitly chosen.
            Err(e) if e.to_string().ends_with("已安装") => {
                let id = client.project_id(slug).await?;
                if let Some(f) = plan.files.iter_mut().find(|f| f.project_id == id) {
                    f.user_selected = true;
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(plan)
}
