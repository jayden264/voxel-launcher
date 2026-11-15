//! Keeping launches from failing, and explaining the ones that still do.
//!
//! * `safe_memory_mb` keeps the requested heap inside what the machine can give.
//! * `diagnose` reads a game log / crash report and names the likely cause in
//!   plain language, with something the player can do about it.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnosis {
    /// Stable id, useful for the UI and tests.
    pub code: &'static str,
    pub title: String,
    pub advice: String,
    /// The log line that gave it away (trimmed), if there is one.
    pub detail: Option<String>,
}

/// Heap to actually use: the request, but never more than the memory that is
/// free right now minus headroom for the OS and the JVM's own overhead.
/// `available_mb == 0` means "unknown": the request is kept as is.
pub fn safe_memory_mb(requested_mb: u32, available_mb: u32, max_mb: u32) -> u32 {
    const HEADROOM: u32 = 1024;
    const FLOOR: u32 = 1024;
    let mut mb = requested_mb.min(max_mb.max(FLOOR));
    if available_mb > 0 {
        mb = mb.min(available_mb.saturating_sub(HEADROOM).max(FLOOR));
    }
    mb.max(FLOOR)
}

/// Free physical memory in MiB (0 if it cannot be read).
pub fn available_memory_mb() -> u32 {
    match sysinfo::System::new() {
        Ok(mut sys) => {
            sys.refresh_memory();
            (sys.available_memory() / 1024 / 1024).min(u32::MAX as u64) as u32
        }
        Err(_) => 0,
    }
}

/// The programs using the most memory right now as `(name, MiB)`, biggest first.
/// Processes of the same program are added together (a browser is dozens).
pub fn top_memory_programs(n: usize) -> Vec<(String, u32)> {
    use sysinfo::{ProcessesToUpdate, System};
    let Ok(mut sys) = System::new() else { return vec![] };
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let mut by_name: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for p in sys.processes().values() {
        *by_name.entry(p.name().to_string_lossy().into_owned()).or_default() += p.memory();
    }
    let mut v: Vec<(String, u32)> = by_name.into_iter().map(|(k, b)| (k, (b / 1024 / 1024).min(u32::MAX as u64) as u32)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(n);
    v
}

/// Free space (MiB) on the drive holding `path`; `None` if unknown.
pub fn free_disk_mb(path: &std::path::Path) -> Option<u64> {
    use sysinfo::Disks;
    let target = std::fs::canonicalize(path).ok()?;
    let disks = Disks::new_with_refreshed_list().ok()?;
    disks
        .iter()
        .filter(|d| target.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space() / 1024 / 1024)
}

fn find_line<'a>(text: &'a str, needles: &[&str]) -> Option<&'a str> {
    text.lines().find(|l| needles.iter().any(|n| l.contains(n)))
}

fn clip(line: &str) -> String {
    let t = line.trim();
    if t.chars().count() > 220 {
        let s: String = t.chars().take(220).collect();
        format!("{s}…")
    } else {
        t.to_string()
    }
}

fn d(code: &'static str, title: &str, advice: &str, detail: Option<&str>) -> Diagnosis {
    Diagnosis { code, title: title.into(), advice: advice.into(), detail: detail.map(clip) }
}

/// Most specific causes first; the first rule that matches wins.
pub fn diagnose(text: &str) -> Option<Diagnosis> {
    let has = |s: &str| text.contains(s);

    if has("UnsupportedClassVersionError") || has("has been compiled by a more recent version of the Java Runtime") {
        return Some(d(
            "java-too-old",
            "Java 版本太低",
            "这个版本或模组需要更新的 Java。在实例设置里把「Java 路径」清空，让启动器自动下载匹配的运行时。",
            find_line(text, &["UnsupportedClassVersionError", "more recent version"]),
        ));
    }
    if has("Could not reserve enough space") || has("Invalid maximum heap size") || has("Could not create the Java Virtual Machine") {
        return Some(d(
            "jvm-start",
            "Java 无法启动（内存设置过大）",
            "把实例的最大内存调小一些（不要超过电脑可用内存），或检查「额外 JVM 参数」里有没有写错的参数。",
            find_line(text, &["Could not reserve", "Invalid maximum heap", "Could not create the Java"]),
        ));
    }
    if has("java.lang.OutOfMemoryError") {
        return Some(d(
            "out-of-memory",
            "游戏内存不足",
            "在实例设置里调大最大内存（模组多时建议 4–6 GB），同时关掉其他占内存的程序。",
            find_line(text, &["OutOfMemoryError"]),
        ));
    }
    if has("zip END header not found") || has("invalid LOC header") || has("ZipException") || has("invalid CEN header") || has("Not in GZIP format") {
        return Some(d(
            "corrupt-file",
            "有文件下载损坏",
            "某个游戏文件或模组没下完整。到「内容」页删掉最近装的模组重新安装，或重新启动让启动器补全游戏文件。",
            find_line(text, &["ZipException", "zip END header", "invalid LOC", "invalid CEN", "GZIP"]),
        ));
    }
    if has("Found duplicate mods") || has("Duplicate mod") || has("duplicate mod") {
        return Some(d(
            "duplicate-mods",
            "装了重复的模组",
            "同一个模组出现了两份（通常是不同版本）。打开实例的 mods 文件夹，删掉旧版本只留一个。",
            find_line(text, &["uplicate mod"]),
        ));
    }
    if has("Mod resolution failed") || has("Incompatible mod set") || has("HARD_DEP_NO_CANDIDATE") || has("Mod dependency") && has("requires") {
        return Some(d(
            "mod-dependency",
            "模组缺少前置或版本不兼容",
            "日志里会写明缺哪个前置模组、需要什么版本。去「内容」页搜索并安装它（如 Fabric API、前置库），或更新/删除不兼容的模组。",
            find_line(text, &["Immediate reason", "Unmet dependency", "depends on", "requires"]),
        ));
    }
    if has("Missing or unsupported mandatory dependencies") {
        return Some(d(
            "mod-dependency",
            "模组缺少前置",
            "日志里列出了缺失的前置模组，安装它们后再启动。",
            find_line(text, &["Missing or unsupported", "Mod ID:"]),
        ));
    }
    if has("IMixinConfigPlugin") && has("ClassCastException") {
        return Some(d(
            "loader-twice",
            "模组加载器被重复加载",
            "通常是游戏文件所在路径被系统重定向或做了链接。请把启动器数据放在普通本地文件夹，或用安装版启动器重试。",
            find_line(text, &["ClassCastException"]),
        ));
    }
    if has("InvalidMixinException") || has("MixinApplyError") || has("Mixin apply failed") || has("MixinTransformerError") {
        return Some(d(
            "mixin-conflict",
            "模组之间冲突",
            "有模组修改游戏的方式互相冲突，或不适配当前游戏版本。查看日志里提到的模组名，更新或移除它。",
            find_line(text, &["InvalidMixinException", "MixinApplyError", "Mixin apply failed"]),
        ));
    }
    if has("NoSuchMethodError") || has("NoSuchFieldError") || has("NoClassDefFoundError") || has("AbstractMethodError") {
        return Some(d(
            "mod-version-mismatch",
            "模组版本和游戏或其他模组不匹配",
            "多半是某个模组版本过旧/过新。检查模组都是为当前游戏版本和加载器下载的，并更新它们的前置库。",
            find_line(text, &["NoSuchMethodError", "NoSuchFieldError", "NoClassDefFoundError", "AbstractMethodError"]),
        ));
    }
    if has("GLFW error") || has("Pixel format not accepted") || has("WGL:") || has("Failed to create window") || has("OpenGL") && has("not supported") {
        return Some(d(
            "graphics",
            "显卡或驱动不支持",
            "更新显卡驱动；笔记本请在系统设置里让 Java 使用独立显卡。光影或渲染类模组可能需要更新的显卡。",
            find_line(text, &["GLFW error", "Pixel format", "WGL:", "Failed to create window", "OpenGL"]),
        ));
    }
    if has("Address already in use") || has("BindException") {
        return Some(d(
            "port-in-use",
            "端口被占用",
            "可能已经有一个游戏或服务器在运行。关闭它们后再试。",
            find_line(text, &["Address already in use", "BindException"]),
        ));
    }
    if has("A mod crashed on startup") || has("Caused by:") && has("Exception") {
        return Some(d(
            "mod-crash",
            "有模组在启动时出错",
            "展开日志，看「Caused by」附近提到的模组名；先把它移出 mods 文件夹，确认是不是它的问题。",
            find_line(text, &["A mod crashed on startup", "Caused by:"]),
        ));
    }
    None
}

/// Diagnose from several sources at once (crash report, game log, stdio log);
/// the first source that yields a cause wins, in the order given.
pub fn diagnose_all(sources: &[&str]) -> Option<Diagnosis> {
    sources.iter().find_map(|s| diagnose(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_is_capped_by_what_is_free() {
        assert_eq!(safe_memory_mb(8192, 5000, 12000), 3976);
        assert_eq!(safe_memory_mb(4096, 16000, 12000), 4096);
        assert_eq!(safe_memory_mb(4096, 0, 12000), 4096, "unknown free memory keeps the request");
        assert_eq!(safe_memory_mb(4096, 1200, 12000), 1024, "never below the floor");
        assert_eq!(safe_memory_mb(20000, 0, 6000), 6000, "never above the slider maximum");
    }

    #[test]
    fn top_memory_programs_is_limited_and_sorted() {
        let top = top_memory_programs(3);
        assert!(top.len() <= 3);
        assert!(top.windows(2).all(|w| w[0].1 >= w[1].1));
    }

    #[test]
    fn names_each_known_cause() {
        let cases = [
            ("java.lang.UnsupportedClassVersionError: x", "java-too-old"),
            ("Error occurred during initialization of VM\nCould not reserve enough space for 9437184KB object heap", "jvm-start"),
            ("Exception in thread \"main\" java.lang.OutOfMemoryError: Java heap space", "out-of-memory"),
            ("Caused by: java.util.zip.ZipException: zip END header not found", "corrupt-file"),
            ("Found duplicate mods:\n - sodium", "duplicate-mods"),
            ("Mod resolution failed\nImmediate reason: [HARD_DEP_NO_CANDIDATE lithium]", "mod-dependency"),
            ("Missing or unsupported mandatory dependencies:\n\tMod ID: 'x'", "mod-dependency"),
            ("ClassCastException: class A cannot be cast to class org.spongepowered.asm.mixin.extensibility.IMixinConfigPlugin", "loader-twice"),
            ("org.spongepowered.asm.mixin.transformer.throwables.InvalidMixinException: boom", "mixin-conflict"),
            ("java.lang.NoSuchMethodError: foo", "mod-version-mismatch"),
            ("GLFW error 65543: WGL: The driver does not appear to support OpenGL", "graphics"),
            ("java.net.BindException: Address already in use", "port-in-use"),
            ("A mod crashed on startup!", "mod-crash"),
        ];
        for (text, code) in cases {
            assert_eq!(diagnose(text).map(|x| x.code), Some(code), "{text}");
        }
        assert_eq!(diagnose("everything is fine"), None);
    }

    #[test]
    fn the_most_specific_cause_wins_over_a_generic_crash() {
        let log = "A mod crashed on startup!\nCaused by: java.lang.OutOfMemoryError: Java heap space";
        assert_eq!(diagnose(log).unwrap().code, "out-of-memory");
    }

    #[test]
    fn detail_is_the_offending_line_and_is_clipped() {
        let long = format!("Mod resolution failed\nImmediate reason: {}", "x".repeat(400));
        let got = diagnose(&long).unwrap();
        let detail = got.detail.unwrap();
        assert!(detail.starts_with("Immediate reason"));
        assert!(detail.chars().count() <= 221);
    }

    #[test]
    fn diagnose_all_prefers_earlier_sources() {
        let got = diagnose_all(&["nothing here", "java.lang.OutOfMemoryError", "NoSuchMethodError"]).unwrap();
        assert_eq!(got.code, "out-of-memory");
        assert!(diagnose_all(&["a", "b"]).is_none());
    }
}
