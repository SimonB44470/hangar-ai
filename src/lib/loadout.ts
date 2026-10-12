import { invoke } from "@tauri-apps/api/core";

/**
 * Loadout: the plugin catalogs a team equips its Claude Code projects from.
 *
 * Everything here is computed by the core catalog's own engine (`loadout.mjs`),
 * which the backend runs with `--json`; the shapes below are its output. The
 * window only draws them, so a catalog that changes its checks needs no
 * Hangar release.
 *
 * The core is one catalog and each unit is its own private catalog, so a
 * plugin is known by its id, `<name>@<catalog>` (`sketch@studio`); the
 * core is `loadout@loadout`. Engines older than units-as-catalogs had a single
 * catalog and no ids: there every plugin is `<name>@<that catalog>`.
 */

/** `session`: can only be checked from inside a Claude session (an MCP server). */
export type ProviderState = "ok" | "missing" | "session";

export interface ProviderUse {
  provider: string;
  label: string;
  state: ProviderState;
  detail: string;
  /** What to do to enable it, written for people. */
  how?: string;
  /** What using it costs, when the catalog says. */
  cost?: string;
  /** Paid: the skill asks before spending. */
  ask_before?: boolean;
}

export interface Capability {
  id: string;
  label: string;
  ready: boolean;
  /** The provider the capability would use right now, first of its chain. */
  uses: string | null;
  /** All of them are needed. */
  requires: ProviderUse[];
  /** The first available one is used, in order. */
  chain: ProviderUse[];
}

export interface SkillInfo {
  name: string;
  description: string;
}

export interface LoadoutPlugin {
  /** `<name>@<catalog>`; absent from single-catalog engines (see `idOf`). */
  id?: string;
  name: string;
  /** The catalog it comes from: the core's, or its unit's. */
  catalog?: string;
  unit: string;
  /** What the plugin is for; absent from engines older than the units pyramid. */
  description?: string;
  /** Its skills, when a copy of the plugin is readable on this machine. */
  skills?: SkillInfo[];
  /** Its unit's base: equipped along with any other plugin of the unit. */
  base?: boolean;
  /** `null` when no project was asked about. */
  equipped: boolean | null;
  /** False when its repository is private and out of reach for this account. */
  access: boolean;
  capabilities: Capability[];
}

/** A unit the engine knows of: on this machine, on GitHub, or both. */
export interface LoadoutUnit {
  unit: string;
  label: string;
  /** Its base plugin's name. */
  base: string;
  /** `owner/name` on GitHub. */
  repo: string;
  catalog: string;
  /** Its marketplace is known to Claude Code here: its plugins install. */
  added: boolean;
  /** A working clone sits in the core clone's `repos/`. */
  local: boolean;
  /** Seen on GitHub: this account can read it. */
  discovered: boolean;
}

/** `unavailable`: GitHub could not be asked (gh missing or signed out); `skipped`: offline on purpose. */
export type Discovery = "github" | "unavailable" | "skipped";

export interface LoadoutStatus {
  /** The core catalog's name. */
  catalog: string;
  project: string | null;
  /** Absent from single-catalog engines, like `units`. */
  discovery?: Discovery;
  units?: LoadoutUnit[];
  plugins: LoadoutPlugin[];
}

export interface Preset {
  label: string;
  /** Plugin names inside the preset's catalog. */
  plugins: string[];
  /** Absent from single-catalog engines: the core's then. */
  catalog?: string;
}

export interface EquipReport {
  file: string;
  plugins: string[];
  /** Old `<plugin>@loadout` entries moved to their unit's catalog. */
  migrated?: { from: string; to: string }[];
  /** Old entries whose unit is not on this machine: they stay as they were. */
  unresolved?: string[];
}

/** Path to the engine, or `null` when Loadout is not installed. */
export const loadoutLocate = () => invoke<string | null>("loadout_locate");

export const loadoutStatus = (cwd: string) => invoke<LoadoutStatus>("loadout_status", { cwd });

/** Keyed by preset name, or `<preset>@<catalog>` when two catalogs share one. */
export const loadoutPresets = () => invoke<Record<string, Preset>>("loadout_presets");

/** Equips the project with presets or plugins, by preset name or plugin id. */
export const loadoutEquip = (cwd: string, names: string[]) =>
  invoke<EquipReport>("loadout_equip", { cwd, names });

/** Moves the project's old `<plugin>@loadout` entries to their unit's catalog (`equip` with no names). */
export const loadoutMigrate = (cwd: string) => invoke<EquipReport>("loadout_migrate", { cwd });

/** Adds a unit's repository (`owner/name`) to Claude Code's marketplaces. */
export const loadoutPlug = (repo: string) => invoke<void>("loadout_plug", { repo });

export interface UnequipReport {
  file: string;
  removed: string[];
  /** Asked for but not equipped: nothing to do. */
  absent?: string[];
  kept: string[];
  /** E.g. "sketch still needs brush", when a plugin that stays depends on a removed one. */
  warnings?: string[];
}

/** Removes presets or plugins from the project; the core cannot be removed. */
export const loadoutUnequip = (cwd: string, names: string[]) =>
  invoke<UnequipReport>("loadout_unequip", { cwd, names });

/** Every plugin id the project's settings enable, from any marketplace — a file read, instant. */
export const loadoutProject = (cwd: string) => invoke<string[]>("loadout_project", { cwd });

/** The core plugin, installed for the person and active in every project. */
export const CORE_ID = "loadout@loadout";

const coreCatalog = (status: LoadoutStatus | null): string => status?.catalog ?? "loadout";

export const nameOf = (id: string): string => id.split("@")[0];
export const catalogOf = (id: string): string | undefined => id.split("@")[1];

export const idOf = (plugin: LoadoutPlugin, status: LoadoutStatus | null = null): string =>
  plugin.id ?? `${plugin.name}@${plugin.catalog ?? coreCatalog(status)}`;

export const pluginById = (id: string, status: LoadoutStatus | null): LoadoutPlugin | undefined =>
  status?.plugins.find((p) => idOf(p, status) === id);

/**
 * The core is installed for the person, not per project: active everywhere,
 * never offered nor removed. Known from the status; `loadout@loadout` until it is in.
 */
export const isCore = (id: string, status: LoadoutStatus | null): boolean => {
  const plugin = pluginById(id, status);
  return plugin ? plugin.unit === "open" : id === CORE_ID;
};

/** A preset's plugins, as ids in the preset's own catalog. */
export const presetIds = (preset: Preset, status: LoadoutStatus | null): string[] =>
  preset.plugins.map((name) => (name.includes("@") ? name : `${name}@${preset.catalog ?? coreCatalog(status)}`));

/**
 * The ids a preset name or a plugin stands for. A bare plugin name (folder
 * defaults saved by older versions) resolves when one plugin has it.
 */
export function idsOf(name: string, presets: Record<string, Preset>, status: LoadoutStatus | null): string[] {
  if (presets[name]) return presetIds(presets[name], status);
  if (name.includes("@")) return [name];
  const found = status?.plugins.filter((p) => p.name === name) ?? [];
  return [found.length === 1 ? idOf(found[0], status) : `${name}@${coreCatalog(status)}`];
}

/** A preset made only of the core adds nothing a project does not already have. */
export const coreOnly = (preset: Preset, status: LoadoutStatus | null): boolean =>
  presetIds(preset, status).every((id) => isCore(id, status));

/** The Loadout catalogs this window knows of: the core, the units, and whatever the presets come from. */
export function catalogsOf(status: LoadoutStatus | null, presets: Record<string, Preset>): Set<string> {
  const out = new Set([coreCatalog(status)]);
  for (const unit of status?.units ?? []) out.add(unit.catalog);
  for (const plugin of status?.plugins ?? []) out.add(catalogOf(idOf(plugin, status))!);
  for (const preset of Object.values(presets)) if (preset.catalog) out.add(preset.catalog);
  return out;
}

/** The ids of a project's settings that belong to a Loadout catalog. */
export const loadoutIds = (ids: string[], catalogs: Set<string>): string[] =>
  ids.filter((id) => catalogs.has(catalogOf(id) ?? ""));

/**
 * Entries written before units had their own catalogs (`sketch@loadout`):
 * they no longer resolve, and `loadoutMigrate` moves them. Only an engine with
 * unit catalogs makes them stale — its status has `units`, its presets a
 * `catalog` — so nothing is flagged until one of those is known.
 */
export function legacyIds(ids: string[], status: LoadoutStatus | null, presets: Record<string, Preset>): string[] {
  const unitCatalogs = Boolean(status?.units) || Object.values(presets).some((p) => p.catalog);
  if (!unitCatalogs) return [];
  const core = coreCatalog(status);
  return ids.filter((id) => catalogOf(id) === core && !isCore(id, status) && !pluginById(id, status));
}

export interface Fix {
  provider: string;
  label: string;
  how: string;
}

/**
 * What is missing on this machine and how to enable it — once per provider,
 * however many capabilities cite it. Plugins out of reach are left out: no
 * install fixes a repository nobody gave access to.
 */
export function missingFixes(status: LoadoutStatus): Fix[] {
  const seen = new Map<string, Fix>();
  for (const plugin of status.plugins) {
    if (!plugin.access) continue;
    for (const capability of plugin.capabilities) {
      for (const use of [...capability.requires, ...capability.chain]) {
        if (use.state !== "missing" || seen.has(use.provider)) continue;
        seen.set(use.provider, {
          provider: use.provider,
          label: use.label,
          how: use.how ?? use.detail,
        });
      }
    }
  }
  return [...seen.values()];
}

/** Whether the project already has every plugin of the preset. */
export function presetIsEquipped(preset: Preset, status: LoadoutStatus): boolean {
  const equipped = new Set(status.plugins.filter((p) => p.equipped).map((p) => idOf(p, status)));
  return presetIds(preset, status).every((id) => equipped.has(id));
}

/** A text the catalog gives in both of the app's languages. */
export type Localized = { fr: string; en: string };

export interface LoadoutPrompt {
  id: string;
  /** Instructions for the person to follow, not a prompt to paste. */
  tutorial: boolean;
  /** Announced, not usable yet. */
  soon: boolean;
  title: Localized;
  when: Localized;
  /** Line breaks as `\n`; may hold `{skill}`, which the person replaces. */
  text: Localized;
}

/** The catalog's ready-to-copy prompts, or `null` from an engine that has none. */
export const loadoutPrompts = () =>
  invoke<{ prompts: LoadoutPrompt[] } | null>("loadout_prompts");

/** The app's language, falling back to English when the catalog lacks it. */
export const pick = (text: Localized, locale: "en" | "fr"): string => text[locale] || text.en;

/**
 * The plugin ids a list of presets and plugins stands for. The core is always
 * there: the engine adds it to every equip.
 */
export function expand(names: string[], presets: Record<string, Preset>, status: LoadoutStatus | null = null): string[] {
  const out = new Set([CORE_ID]);
  for (const name of names) for (const id of idsOf(name, presets, status)) out.add(id);
  return [...out].sort();
}

/** What a project lacks of its folder's default. */
export const missing = (wanted: string[], equipped: string[]): string[] =>
  wanted.filter((plugin) => !equipped.includes(plugin));

/** Identifies a folder default, so "later" holds until that default changes. */
export const snoozeKey = (names: string[]): string => [...names].sort().join("+");

/** A default's label: the preset's own when it is one, the names otherwise. */
export function defaultLabel(names: string[], presets: Record<string, Preset>): string {
  return names.map((name) => presets[name]?.label ?? name).join(" + ");
}

/** The catalog calls its core unit "open"; people know it as the core. */
export const unitLabel = (unit: string): string => (unit === "open" ? "core" : unit);

/**
 * The units this machine shows: added to Claude Code or cloned locally. A unit
 * this account cannot read is never listed by the engine, so it never appears
 * at all — no lock, no name. Single-catalog engines have no `units`: their
 * units are read off the plugins, without the ones out of reach.
 */
export function visibleUnits(status: LoadoutStatus): string[] {
  if (status.units) return status.units.filter((u) => u.added || u.local).map((u) => u.unit);
  return [...new Set(status.plugins.filter((p) => p.access && p.unit !== "open").map((p) => p.unit))];
}

/** Units found on GitHub that Claude Code does not know yet: each can be plugged in. */
export const pluggable = (status: LoadoutStatus): LoadoutUnit[] =>
  (status.units ?? []).filter((u) => u.discovered && !u.added);

export interface UnitAccess {
  unit: string;
  /** The catalog's name for it; the unit's id when the engine gives none. */
  label: string;
}

/** The core first, then every visible unit by name. */
export function unitsOf(status: LoadoutStatus): UnitAccess[] {
  const labels = new Map((status.units ?? []).map((u) => [u.unit, u.label]));
  return [
    { unit: "core", label: "core" },
    ...visibleUnits(status)
      .sort((a, b) => a.localeCompare(b))
      .map((unit) => ({ unit, label: labels.get(unit) ?? unit })),
  ];
}

/**
 * The units a preset or plugin name reaches, and whether any of its plugins is out of reach. Unknown until the
 * status is in: then nothing is locked.
 */
export function reachOf(
  name: string,
  presets: Record<string, Preset>,
  status: LoadoutStatus | null,
): { units: string[]; locked: boolean } {
  const found = idsOf(name, presets, status)
    .map((id) => pluginById(id, status))
    .filter((p): p is LoadoutPlugin => Boolean(p));
  return {
    units: [...new Set(found.map((p) => unitLabel(p.unit)))],
    locked: found.some((p) => !p.access),
  };
}

/** What this account can install: the core and the visible units' plugins, readable ones only. */
export function reachable(status: LoadoutStatus): LoadoutStatus {
  const shown = new Set(visibleUnits(status));
  return {
    ...status,
    plugins: status.plugins.filter((p) => p.access && (p.unit === "open" || shown.has(p.unit))),
  };
}

/**
 * What a project can be equipped with: everything this account can install but
 * the core, which is installed for the person and already active everywhere —
 * it is never in a project's settings, so it would otherwise look not equipped.
 */
export const offerable = (status: LoadoutStatus): LoadoutPlugin[] =>
  reachable(status).plugins.filter((p) => !isCore(idOf(p, status), status));

/** A unit's base stays while another plugin of its unit is equipped: the engine refuses to remove it then. */
export function heldBase(id: string, equipped: string[], status: LoadoutStatus | null): string | null {
  const plugin = pluginById(id, status);
  if (!plugin?.base) return null;
  const sibling = status!.plugins.some(
    (p) => p.unit === plugin.unit && idOf(p, status) !== id && equipped.includes(idOf(p, status)),
  );
  return sibling ? unitLabel(plugin.unit) : null;
}

export interface UnitBlock {
  unit: string;
  /** The catalog's name for the unit, when it gives one. */
  label?: string;
  plugins: LoadoutPlugin[];
  /** What the engine says about the unit; absent for the core and single-catalog engines. */
  info?: LoadoutUnit;
}

/** The pyramid: the core underneath everything, then each visible unit, sorted by name. */
export function pyramidOf(status: LoadoutStatus): { core: UnitBlock | null; units: UnitBlock[] } {
  const info = new Map((status.units ?? []).map((u) => [u.unit, u]));
  const blocks = new Map<string, UnitBlock>();
  // A visible unit is drawn even before its plugins are listed.
  for (const unit of visibleUnits(status)) {
    blocks.set(unit, { unit, label: info.get(unit)?.label, plugins: [], info: info.get(unit) });
  }
  for (const plugin of reachable(status).plugins) {
    const unit = unitLabel(plugin.unit);
    const block = blocks.get(unit) ?? { unit, plugins: [] };
    block.plugins.push(plugin);
    blocks.set(unit, block);
  }
  // The base first in its unit: everything else in the unit stands on it.
  for (const block of blocks.values()) block.plugins.sort((x, y) => Number(Boolean(y.base)) - Number(Boolean(x.base)));
  const core = blocks.get("core") ?? null;
  blocks.delete("core");
  return { core, units: [...blocks.values()].sort((x, y) => x.unit.localeCompare(y.unit)) };
}
