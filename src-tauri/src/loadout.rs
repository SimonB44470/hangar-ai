//! Loadout: the plugin catalog a team equips its Claude Code projects from.
//!
//! Hangar does not reimplement any of it. The catalog ships its own engine,
//! `loadout.mjs`, which already knows how to check every capability on this
//! machine and how to write a project's `.claude/settings.json`; the tab only
//! runs it with `--json` and draws what comes back. A catalog that changes its
//! checks therefore needs no Hangar release.
//!
//! The engine is found where Claude Code installs the marketplace. When it is
//! not there, `loadout_locate` says so as data and the tab stays hidden: most
//! people running Hangar have never heard of Loadout, and should not have to.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use crate::git::off_thread;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Points at a catalog clone instead of the installed marketplace — the same
/// thing `loadout.mjs --catalog` does, for whoever works on the catalog itself.
const CATALOG_ENV: &str = "LOADOUT_CATALOG";

fn script_in(catalog: &Path) -> PathBuf {
    catalog
        .join("plugins")
        .join("loadout")
        .join("scripts")
        .join("loadout.mjs")
}

fn locate() -> Option<PathBuf> {
    let catalog = match std::env::var_os(CATALOG_ENV) {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => dirs::home_dir()?
            .join(".claude")
            .join("plugins")
            .join("marketplaces")
            .join("loadout"),
    };
    let script = script_in(&catalog);
    script.is_file().then_some(script)
}

fn script() -> Result<PathBuf, String> {
    locate().ok_or_else(|| "Loadout is not installed".to_string())
}

/// The line of a failed run worth showing: the engine throws plain `Error`s
/// whose message is written for people, and node prints it under a stack.
fn diagnostic(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines
        .iter()
        .find_map(|l| l.strip_prefix("Error: "))
        .or_else(|| lines.last().copied())
        .unwrap_or("loadout.mjs failed")
        .to_string()
}

fn run(args: &[&str]) -> Result<String, String> {
    let script = script()?;
    // Without it a bundled macOS app looks at the bare `launchd` PATH, where a
    // Homebrew node does not exist. See path_env.rs.
    crate::path_env::ensure();

    let mut command = Command::new("node");
    command.arg(&script).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command.output().map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => "Node.js was not found on PATH".to_string(),
        _ => format!("could not run node: {err}"),
    })?;
    if !output.status.success() {
        return Err(diagnostic(&output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn run_json(args: &[&str]) -> Result<Value, String> {
    let out = run(args)?;
    serde_json::from_str(&out).map_err(|err| format!("loadout.mjs printed no JSON: {err}"))
}

/// Gives each provider entry the catalog's "how to enable it" line when the
/// engine did not include it (engines older than the `how` field). Entries
/// that already carry one are left alone.
fn fill_how(status: &mut Value, providers: &Value) {
    let how = |id: &str| providers.pointer(&format!("/providers/{id}/how")).cloned();
    let Some(plugins) = status.get_mut("plugins").and_then(Value::as_array_mut) else {
        return;
    };
    for capability in plugins
        .iter_mut()
        .filter_map(|p| p.get_mut("capabilities").and_then(Value::as_array_mut))
        .flatten()
    {
        for key in ["requires", "chain"] {
            let Some(entries) = capability.get_mut(key).and_then(Value::as_array_mut) else {
                continue;
            };
            for entry in entries.iter_mut().filter_map(Value::as_object_mut) {
                if entry.contains_key("how") {
                    continue;
                }
                let found = entry.get("provider").and_then(Value::as_str).and_then(&how);
                if let Some(found) = found {
                    entry.insert("how".into(), found);
                }
            }
        }
    }
}

/// Where the engine is, or `None` — in which case the tab is not shown.
#[tauri::command]
pub fn loadout_locate() -> Option<String> {
    locate().map(|p| p.to_string_lossy().into_owned())
}

/// Every capability of the catalog, checked on this machine, with what the
/// workspace has equipped. Asking GitHub which units this account can read is
/// most of its time (about two seconds): `offline` skips that, so the tab can
/// draw what is on this machine first and fill in the discovered units after.
#[tauri::command]
pub async fn loadout_status(cwd: String, offline: Option<bool>) -> Result<Value, String> {
    off_thread(move || {
        let mut args = vec!["status", "--project", cwd.as_str(), "--json"];
        if offline == Some(true) {
            args.push("--offline");
        }
        let mut status = run_json(&args)?;
        let providers = script()?
            .parent()
            .and_then(Path::parent)
            .map(|dir| dir.join("providers.json"));
        if let Some(providers) = providers
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        {
            fill_how(&mut status, &providers);
        }
        Ok(status)
    })
    .await
}

/// Project types and the plugins each one equips.
#[tauri::command]
pub async fn loadout_presets() -> Result<Value, String> {
    off_thread(|| run_json(&["presets", "--json"])).await
}

/// Ready-to-copy prompts for keeping a setup lean (the catalog's `prompts`),
/// or `None` from an engine that predates them — the panel is then hidden.
#[tauri::command]
pub async fn loadout_prompts() -> Result<Option<Value>, String> {
    off_thread(|| match run_json(&["prompts", "--json"]) {
        Ok(prompts) => Ok(Some(prompts)),
        Err(err) if is_unknown_command(&err) => Ok(None),
        Err(err) => Err(err),
    })
    .await
}

/// How an engine says it has no such command: `loadout: unknown command "x"`.
fn is_unknown_command(err: &str) -> bool {
    err.contains("unknown command")
}

/// Equips the workspace with presets or plugins: the engine writes its
/// `.claude/settings.json` and reports `{ file, plugins, written }`.
#[tauri::command]
pub async fn loadout_equip(cwd: String, names: Vec<String>) -> Result<Value, String> {
    off_thread(move || {
        if names.is_empty() {
            return Err("nothing to equip".to_string());
        }
        let mut args = vec!["equip", cwd.as_str()];
        args.extend(names.iter().map(String::as_str));
        let report = run_json(&[args.as_slice(), &["--json"]].concat())?;
        // Engines older than `written` treated `--json` as a preview and wrote
        // nothing: the plain command is the one that writes there.
        if report.get("written").and_then(Value::as_bool) != Some(true) {
            run(&args)?;
        }
        Ok(report)
    })
    .await
}

/// Removes presets or plugins from the workspace's `.claude/settings.json`.
/// The engine refuses the core, which stays active everywhere.
#[tauri::command]
pub async fn loadout_unequip(cwd: String, names: Vec<String>) -> Result<Value, String> {
    off_thread(move || {
        if names.is_empty() {
            return Err("nothing to unequip".to_string());
        }
        let mut args = vec!["unequip", cwd.as_str()];
        args.extend(names.iter().map(String::as_str));
        args.push("--json");
        run_json(&args)
    })
    .await
}

/// Every plugin id (`<name>@<catalog>`) a project's `.claude/settings.json`
/// enables. Each unit is its own catalog now, so the window, which knows the
/// catalogs from the engine, keeps the Loadout ones; plugins from other
/// marketplaces pass through and are ignored there.
fn enabled_in(settings: &Value) -> Vec<String> {
    let mut ids: Vec<String> = settings
        .get("enabledPlugins")
        .and_then(Value::as_object)
        .map(|enabled| {
            enabled
                .iter()
                .filter(|(key, on)| on.as_bool() == Some(true) && key.contains('@'))
                .map(|(key, _)| key.clone())
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids
}

/// What a project has equipped, read straight from its settings file: no
/// engine run, so the offer to equip can be decided the moment a workspace
/// opens.
#[tauri::command]
pub fn loadout_project(cwd: String) -> Result<Vec<String>, String> {
    script()?;
    let file = Path::new(&cwd).join(".claude").join("settings.json");
    let settings = std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or(Value::Null);
    Ok(enabled_in(&settings))
}

/// Moves a project's old `<plugin>@loadout` entries to their unit's catalog:
/// the engine's `equip` with no names only migrates, and reports
/// `{ migrated, unresolved, written }`.
#[tauri::command]
pub async fn loadout_migrate(cwd: String) -> Result<Value, String> {
    off_thread(move || run_json(&["equip", &cwd, "--json"])).await
}

/// A GitHub `owner/name`, and nothing else: the one argument `loadout_plug`
/// hands to the CLI comes from the engine's discovery, and only this shape is
/// let through (no URL, no path, no flag, nothing a shell would read).
fn is_github_repo(repo: &str) -> bool {
    let part = |s: &str| {
        !s.is_empty()
            && !s.starts_with(['-', '.'])
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    matches!(repo.split_once('/'), Some((owner, name)) if part(owner) && part(name) && !name.contains('/'))
}

/// Exactly `claude plugin marketplace add <owner/name>`.
fn plug_args(repo: &str) -> Result<[&str; 4], String> {
    if !is_github_repo(repo) {
        return Err(format!("not a GitHub repository: {repo}"));
    }
    Ok(["plugin", "marketplace", "add", repo])
}

/// A marketplace name as Claude Code keys it in `extraKnownMarketplaces`.
fn is_marketplace_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['-', '.'])
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Claude Code's user settings: a marketplace's `autoUpdate` there applies to
/// every project, those equipped later included.
fn user_settings() -> Result<PathBuf, String> {
    dirs::home_dir()
        .map(|home| home.join(".claude").join("settings.json"))
        .ok_or_else(|| "no home folder".to_string())
}

/// The settings file as JSON, `{}` when there is none yet. A file that does not
/// parse is an error, never an empty object: writing that back would wipe it.
fn read_settings(path: &Path) -> Result<Value, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(json!({})),
        Err(err) => return Err(format!("could not read {}: {err}", path.display())),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(value) if value.is_object() => Ok(value),
        _ => Err(format!(
            "{} is not a JSON object: fix it before changing auto-update",
            path.display()
        )),
    }
}

/// Writes next to the file then renames over it, so Claude Code never reads
/// half a settings file. Keys keep their order (serde_json `preserve_order`).
fn write_settings(path: &Path, settings: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())? + "\n";
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.hangar-tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("could not replace {}: {e}", path.display())
    })
}

/// Each marketplace's `autoUpdate`, where it is set. Only that key is read:
/// sources and everything else in the settings stay out of the window.
fn auto_updates_in(settings: &Value) -> std::collections::BTreeMap<String, bool> {
    settings
        .get("extraKnownMarketplaces")
        .and_then(Value::as_object)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|(name, entry)| {
                    Some((name.clone(), entry.get("autoUpdate")?.as_bool()?))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Sets a marketplace's `autoUpdate`, creating its entry with `source` when it
/// has none. With `keep_explicit`, a value already set (an explicit `false`
/// above all) is left alone. Touches nothing else; says whether it changed.
fn set_auto_update(
    settings: &mut Value,
    name: &str,
    source: Option<Value>,
    on: bool,
    keep_explicit: bool,
) -> Result<bool, String> {
    let root = settings
        .as_object_mut()
        .ok_or("settings are not an object")?;
    let entries = root
        .entry("extraKnownMarketplaces")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("extraKnownMarketplaces is not an object")?;
    if !entries.contains_key(name) {
        let source = source.ok_or_else(|| format!("no source known for marketplace {name}"))?;
        entries.insert(name.to_string(), json!({ "source": source }));
    }
    let entry = entries
        .get_mut(name)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("extraKnownMarketplaces.{name} is not an object"))?;
    let current = entry.get("autoUpdate").and_then(Value::as_bool);
    if current == Some(on) || (keep_explicit && current.is_some()) {
        return Ok(false);
    }
    entry.insert("autoUpdate".into(), Value::Bool(on));
    Ok(true)
}

/// The core catalog's marketplace name and source (`metadata.loadout.source`
/// of its marketplace.json), for an entry the user settings do not have yet.
fn core_marketplace() -> Option<(String, Value)> {
    let root = script().ok()?.ancestors().nth(4)?.to_path_buf();
    let text =
        std::fs::read_to_string(root.join(".claude-plugin").join("marketplace.json")).ok()?;
    let market: Value = serde_json::from_str(&text).ok()?;
    let name = market.get("name")?.as_str()?.to_string();
    let source = market.pointer("/metadata/loadout/source")?.clone();
    Some((name, source))
}

/// Where a marketplace comes from: the core's own metadata, or a unit's GitHub
/// repository as the engine reported it.
fn source_for(name: &str, repo: Option<&str>) -> Result<Option<Value>, String> {
    if !is_marketplace_name(name) {
        return Err(format!("not a marketplace name: {name}"));
    }
    if let Some((core, source)) = core_marketplace() {
        if core == name {
            return Ok(Some(source));
        }
    }
    match repo {
        Some(repo) if is_github_repo(repo) => Ok(Some(json!({ "source": "github", "repo": repo }))),
        Some(repo) => Err(format!("not a GitHub repository: {repo}")),
        None => Ok(None),
    }
}

/// Every Loadout-relevant marketplace's `autoUpdate` from the user settings.
#[tauri::command]
pub fn loadout_auto_updates() -> Result<std::collections::BTreeMap<String, bool>, String> {
    Ok(auto_updates_in(&read_settings(&user_settings()?)?))
}

/// Turns a catalog's auto-update on or off for every project, in the user
/// settings. `repo` is the unit's `owner/name`, used only when the settings
/// have no entry for it yet.
#[tauri::command]
pub async fn loadout_set_auto_update(
    marketplace: String,
    repo: Option<String>,
    on: bool,
) -> Result<(), String> {
    off_thread(move || {
        let source = source_for(&marketplace, repo.as_deref())?;
        let path = user_settings()?;
        let mut settings = read_settings(&path)?;
        if set_auto_update(&mut settings, &marketplace, source, on, false)? {
            write_settings(&path, &settings)?;
        }
        Ok(())
    })
    .await
}

/// After a unit is plugged in: auto-update on for it and for the core, unless
/// the person already chose (an explicit `false` stays).
fn auto_update_by_default(marketplace: &str, repo: &str) -> Result<(), String> {
    let path = user_settings()?;
    let mut settings = read_settings(&path)?;
    let mut changed = set_auto_update(
        &mut settings,
        marketplace,
        source_for(marketplace, Some(repo))?,
        true,
        true,
    )?;
    if let Some((core, source)) = core_marketplace() {
        changed |= set_auto_update(&mut settings, &core, Some(source), true, true)?;
    }
    if changed {
        write_settings(&path, &settings)?;
    }
    Ok(())
}

/// Plugs a unit discovered on GitHub into Claude Code: adds its repository as
/// a marketplace, after which the engine lists its plugins, and turns its
/// auto-update on (see `auto_update_by_default`).
#[tauri::command]
pub async fn loadout_plug(repo: String, marketplace: String) -> Result<(), String> {
    off_thread(move || {
        let args = plug_args(&repo)?;
        if !is_marketplace_name(&marketplace) {
            return Err(format!("not a marketplace name: {marketplace}"));
        }
        claude(&args)?;
        auto_update_by_default(&marketplace, &repo)
            .map_err(|err| format!("plugged in, but auto-update is not set: {err}"))
    })
    .await
}

/// Runs the Claude Code CLI with arguments its callers have already checked
/// (each builds them from a whitelisted shape).
fn claude(args: &[&str]) -> Result<(), String> {
    crate::path_env::ensure();
    let spawn = |program: &str, prefix: &[&str]| {
        let mut command = Command::new(program);
        command.args(prefix).args(args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        command.output()
    };
    let output = match spawn("claude", &[]) {
        // An npm install is `claude.cmd`, which only cmd.exe resolves; every
        // argument was checked by its caller, so nothing reaches cmd as syntax.
        #[cfg(windows)]
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => spawn("cmd", &["/C", "claude"]),
        other => other,
    }
    .map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => "Claude Code was not found on PATH".to_string(),
        _ => format!("could not run claude: {err}"),
    })?;
    if !output.status.success() {
        let mut text = output.stderr;
        text.extend_from_slice(&output.stdout);
        return Err(diagnostic(&text));
    }
    Ok(())
}

/// Exactly `claude plugin marketplace update <name>`.
fn update_args(name: &str) -> Result<[&str; 4], String> {
    if !is_marketplace_name(name) {
        return Err(format!("not a marketplace name: {name}"));
    }
    Ok(["plugin", "marketplace", "update", name])
}

/// Pulls one catalog's marketplace clone, the copy the engine reads: without
/// it, a plugin merged upstream stays invisible until Claude Code refreshes
/// on its own at the next session start. Only the clone moves; running
/// sessions keep what they loaded until /reload-plugins.
#[tauri::command]
pub async fn loadout_update_marketplace(name: String) -> Result<(), String> {
    off_thread(move || claude(&update_args(&name)?)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_error_message_wins_over_the_stack() {
        let stderr = b"file:///x/loadout.mjs:132\n  throw new Error('nope')\n\nError: unknown plugin or preset \"x\" (see: loadout presets)\n    at resolvePlugins (file:///x:1:1)\n\nNode.js v22.0.0\n";
        assert_eq!(
            diagnostic(stderr),
            "unknown plugin or preset \"x\" (see: loadout presets)"
        );
    }

    #[test]
    fn without_an_error_line_the_last_word_is_kept() {
        assert_eq!(diagnostic(b"something\nwent wrong\n"), "went wrong");
        assert_eq!(diagnostic(b""), "loadout.mjs failed");
    }

    #[test]
    fn an_older_engine_is_told_apart_from_a_failure() {
        assert!(is_unknown_command(
            "loadout: unknown command \"prompts\" (status, equip, presets, lint, readme)"
        ));
        assert!(!is_unknown_command("Node.js was not found on PATH"));
    }

    #[test]
    fn every_enabled_id_is_kept_whatever_its_catalog() {
        let settings = json!({ "enabledPlugins": {
            "sketch@studio": true,
            "brush@loadout": true,
            "off@crew": false,
            "other@elsewhere": true,
            "no-catalog": true
        }});
        assert_eq!(
            enabled_in(&settings),
            ["brush@loadout", "other@elsewhere", "sketch@studio"]
        );
        assert!(enabled_in(&Value::Null).is_empty());
    }

    #[test]
    fn only_a_github_owner_and_name_is_plugged() {
        assert_eq!(
            plug_args("acme/loadout-crew").unwrap(),
            ["plugin", "marketplace", "add", "acme/loadout-crew"]
        );
        assert!(is_github_repo("a_b.c/d-e.f"));
        for bad in [
            "",
            "loadout-crew",
            "a/b/c",
            "/b",
            "a/",
            "--help/x",
            "a/-x",
            "../x",
            "a/b&calc",
            "a/b c",
            "https://github.com/a/b",
            "C:\\x/y",
        ] {
            assert!(plug_args(bad).is_err(), "{bad} must be refused");
        }
    }

    /// Against a real engine: `LOADOUT_CATALOG=<core clone>` and
    /// `LOADOUT_SCRATCH=<empty folder>`, then
    /// `cargo test --lib loadout::tests::real_engine -- --ignored --nocapture`.
    /// Writes only the scratch folder's `.claude/settings.json`. The plugins
    /// come from whatever units the engine finds on this machine (two at least).
    #[test]
    #[ignore]
    fn real_engine_migrates_equips_and_unequips_by_id() {
        use tauri::async_runtime::block_on;
        let cwd = std::env::var("LOADOUT_SCRATCH").expect("LOADOUT_SCRATCH");
        let file = Path::new(&cwd).join(".claude").join("settings.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "{}").unwrap();

        let local = block_on(loadout_status(cwd.clone(), Some(true))).unwrap();
        assert_eq!(local["discovery"], "skipped");
        let status = block_on(loadout_status(cwd.clone(), None)).unwrap();
        println!("discovery {}", status["discovery"]);
        assert_eq!(
            local["plugins"], status["plugins"],
            "offline lists the same plugins"
        );
        let plugins = status["plugins"].as_array().unwrap();
        assert!(plugins.iter().any(|p| p["id"] == "loadout@loadout"));
        // Unit plugins, not bases: equipping one must not drag in what is tested.
        let picks: Vec<(&str, &str)> = plugins
            .iter()
            .filter(|p| p["unit"] != "open" && p["base"] != true)
            .filter_map(|p| Some((p["id"].as_str()?, p["name"].as_str()?)))
            .collect();
        let [(old_id, old_name), (new_id, _), ..] = picks[..] else {
            panic!("needs two unit plugins on this machine, found {picks:?}");
        };

        // A project equipped before units had their own catalogs.
        let legacy = format!("{old_name}@loadout");
        let settings = json!({ "enabledPlugins": { legacy.clone(): true } });
        std::fs::write(&file, settings.to_string()).unwrap();
        assert_eq!(loadout_project(cwd.clone()).unwrap(), [legacy.clone()]);

        let migrated = block_on(loadout_migrate(cwd.clone())).unwrap();
        println!("migrate {}", migrated["migrated"]);
        assert_eq!(migrated["written"], true);
        let ids = loadout_project(cwd.clone()).unwrap();
        assert!(ids.iter().any(|id| id == old_id), "{ids:?}");
        assert!(!ids.contains(&legacy), "{ids:?}");

        let equipped = block_on(loadout_equip(cwd.clone(), vec![new_id.into()])).unwrap();
        println!("equip {}", equipped["plugins"]);
        assert!(loadout_project(cwd.clone())
            .unwrap()
            .iter()
            .any(|id| id == new_id));

        let removed = block_on(loadout_unequip(cwd.clone(), vec![new_id.into()])).unwrap();
        println!("unequip {}", removed["removed"]);
        assert!(!loadout_project(cwd).unwrap().iter().any(|id| id == new_id));
    }

    fn github(repo: &str) -> Option<Value> {
        Some(json!({ "source": "github", "repo": repo }))
    }

    #[test]
    fn auto_update_is_read_only_where_set() {
        let settings = json!({ "extraKnownMarketplaces": {
            "studio": { "source": { "source": "github", "repo": "acme/loadout-studio" }, "autoUpdate": true },
            "crew": { "source": { "source": "github", "repo": "acme/loadout-crew" }, "autoUpdate": false },
            "other": { "source": { "source": "github", "repo": "x/y" } }
        }});
        let found = auto_updates_in(&settings);
        assert_eq!(found.get("studio"), Some(&true));
        assert_eq!(found.get("crew"), Some(&false));
        assert!(!found.contains_key("other"));
        assert!(auto_updates_in(&json!({})).is_empty());
    }

    #[test]
    fn auto_update_creates_the_entry_and_touches_nothing_else() {
        let mut settings = json!({ "theme": "dark", "permissions": { "allow": ["Bash(ls)"] } });
        assert!(set_auto_update(
            &mut settings,
            "crew",
            github("acme/loadout-crew"),
            true,
            false
        )
        .unwrap());
        assert_eq!(
            settings,
            json!({
                "theme": "dark",
                "permissions": { "allow": ["Bash(ls)"] },
                "extraKnownMarketplaces": { "crew": {
                    "source": { "source": "github", "repo": "acme/loadout-crew" },
                    "autoUpdate": true
                }}
            })
        );
        // Off again: the entry and its source stay.
        assert!(set_auto_update(&mut settings, "crew", None, false, false).unwrap());
        assert_eq!(
            settings["extraKnownMarketplaces"]["crew"]["autoUpdate"],
            false
        );
        assert_eq!(
            settings["extraKnownMarketplaces"]["crew"]["source"]["repo"],
            "acme/loadout-crew"
        );
        // Unchanged: nothing to write.
        assert!(!set_auto_update(&mut settings, "crew", None, false, false).unwrap());
        // No entry and no source: refused rather than half-written.
        assert!(set_auto_update(&mut settings, "guild", None, true, false).is_err());
    }

    #[test]
    fn the_default_keeps_an_explicit_choice() {
        let mut settings = json!({ "extraKnownMarketplaces": {
            "crew": { "source": { "source": "github", "repo": "acme/loadout-crew" }, "autoUpdate": false }
        }});
        assert!(!set_auto_update(&mut settings, "crew", None, true, true).unwrap());
        assert_eq!(
            settings["extraKnownMarketplaces"]["crew"]["autoUpdate"],
            false
        );
        assert!(set_auto_update(
            &mut settings,
            "studio",
            github("acme/loadout-studio"),
            true,
            true
        )
        .unwrap());
        assert_eq!(
            settings["extraKnownMarketplaces"]["studio"]["autoUpdate"],
            true
        );
    }

    #[test]
    fn settings_are_written_whole_and_in_order() {
        let dir = std::env::temp_dir().join(format!("hangar-loadout-{}", uuid::Uuid::new_v4()));
        let path = dir.join("settings.json");
        assert_eq!(read_settings(&path).unwrap(), json!({}));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "{ \"zeta\": 1, \"alpha\": { \"b\": 2, \"a\": 1 } }").unwrap();
        let mut settings = read_settings(&path).unwrap();
        set_auto_update(
            &mut settings,
            "crew",
            github("acme/loadout-crew"),
            true,
            false,
        )
        .unwrap();
        write_settings(&path, &settings).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.find("zeta").unwrap() < text.find("alpha").unwrap(),
            "{text}"
        );
        assert!(
            text.find("\"b\"").unwrap() < text.find("\"a\"").unwrap(),
            "{text}"
        );
        assert!(text.ends_with("}\n"));
        assert!(!path.with_extension("json.hangar-tmp").exists());

        std::fs::write(&path, "{ not json").unwrap();
        assert!(read_settings(&path).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_a_marketplace_name_is_updated() {
        assert_eq!(
            update_args("crew").unwrap(),
            ["plugin", "marketplace", "update", "crew"]
        );
        for bad in ["", "--all", "a b", "a&calc", "a/b", "../x"] {
            assert!(update_args(bad).is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn only_a_plain_marketplace_name_is_written() {
        assert!(is_marketplace_name("crew"));
        assert!(is_marketplace_name("acme-tools.v2"));
        for bad in ["", "-x", ".x", "a/b", "a b", "a\"b"] {
            assert!(!is_marketplace_name(bad), "{bad}");
            assert!(source_for(bad, None).is_err(), "{bad}");
        }
        assert!(source_for("crew", Some("not a repo")).is_err());
        assert_eq!(source_for("crew", None).unwrap(), None);
    }

    /// Read-only: prints each marketplace's `autoUpdate` from this machine's
    /// user settings, the state the toggles show.
    /// `cargo test --lib loadout::tests::this_machine -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn this_machine_s_auto_updates() {
        println!("{:?}", loadout_auto_updates().unwrap());
    }

    /// Pulls one real marketplace clone through the CLI, as "Check again" does:
    /// `LOADOUT_PULL=<marketplace name>`, then
    /// `cargo test --lib loadout::tests::real_marketplace -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_marketplace_update() {
        let name = std::env::var("LOADOUT_PULL").expect("LOADOUT_PULL");
        // `cargo test` puts every native dependency's folder in front of PATH
        // (18k characters here), past what cmd.exe searches: the npm
        // `claude.cmd` and the `node` it calls are then "not recognized". The
        // app runs with the user's own PATH; give the test the same.
        let path = std::env::var_os("PATH").unwrap_or_default();
        let own: Vec<PathBuf> = std::env::split_paths(&path)
            .filter(|dir| !dir.to_string_lossy().contains("target"))
            .collect();
        std::env::set_var("PATH", std::env::join_paths(own).unwrap());
        let started = std::time::Instant::now();
        tauri::async_runtime::block_on(loadout_update_marketplace(name.clone())).unwrap();
        println!("{name} pulled in {:?}", started.elapsed());
        assert!(
            tauri::async_runtime::block_on(loadout_update_marketplace("--all".into())).is_err()
        );
    }

    #[test]
    fn how_is_filled_only_where_missing() {
        let mut status = json!({ "plugins": [{ "capabilities": [{
            "requires": [{ "provider": "node" }],
            "chain": [{ "provider": "ffmpeg", "how": "kept" }, { "provider": "unknown" }]
        }]}]});
        let providers = json!({ "providers": {
            "node": { "how": "install Node 20" },
            "ffmpeg": { "how": "replaced?" }
        }});
        fill_how(&mut status, &providers);
        let cap = &status["plugins"][0]["capabilities"][0];
        assert_eq!(cap["requires"][0]["how"], "install Node 20");
        assert_eq!(cap["chain"][0]["how"], "kept");
        assert!(cap["chain"][1].get("how").is_none());
    }
}
