import { describe, expect, it } from "vitest";
import { translate } from "../i18n";
import {
  autoUpdateTarget,
  catalogsOf,
  catalogsToPull,
  keepDiscovered,
  coreOnly,
  defaultLabel,
  expand,
  heldBase,
  idOf,
  idsOf,
  isCore,
  legacyIds,
  loadoutIds,
  missing,
  missingFixes,
  newSkills,
  shownUnit,
  skillCount,
  skillKeys,
  offerable,
  pick,
  pluggable,
  presetIsEquipped,
  pyramidOf,
  reachable,
  reachOf,
  snoozeKey,
  unitsOf,
  type LoadoutPlugin,
  type LoadoutStatus,
  type LoadoutUnit,
  type Preset,
  type ProviderUse,
} from "./loadout";

const use = (provider: string, state: ProviderUse["state"], how?: string): ProviderUse => ({
  provider,
  label: provider.toUpperCase(),
  state,
  detail: `${provider} detail`,
  how,
});

/** A plugin as the units-as-catalogs engine lists it. */
const plugin = (name: string, catalog: string, extra: Partial<LoadoutPlugin> = {}): LoadoutPlugin => ({
  id: `${name}@${catalog}`,
  name,
  catalog,
  unit: catalog === "loadout" ? "open" : catalog,
  equipped: null,
  access: true,
  capabilities: [],
  ...extra,
});

const unit = (name: string, extra: Partial<LoadoutUnit> = {}): LoadoutUnit => ({
  unit: name,
  label: name.toUpperCase(),
  base: `${name}-base`,
  repo: `ORG/loadout-${name}`,
  catalog: name,
  added: true,
  local: false,
  discovered: true,
  ...extra,
});

/** The engine's status: the core, studio added, crew cloned only, guild found on GitHub only. */
const engine: LoadoutStatus = {
  catalog: "loadout",
  project: "C:/game",
  discovery: "github",
  units: [unit("studio", { base: "brush" }), unit("crew", { added: false, local: true, discovered: false }), unit("guild", { added: false })],
  plugins: [
    plugin("loadout", "loadout", { equipped: true }),
    plugin("sketch", "studio", { equipped: true }),
    plugin("brush", "studio", { base: true, equipped: true }),
    plugin("tavern", "crew", { base: true, equipped: false }),
  ],
};

const PRESETS: Record<string, Preset> = {
  blank: { label: "Any project", plugins: ["loadout"], catalog: "loadout" },
  sketch: { label: "Studio project", plugins: ["brush", "sketch"], catalog: "studio" },
  brush: { label: "Any game", plugins: ["brush"], catalog: "studio" },
  crew: { label: "Crew", plugins: ["tavern"], catalog: "crew" },
};

describe("missingFixes", () => {
  const status: LoadoutStatus = {
    catalog: "loadout",
    project: "C:/game",
    plugins: [
      {
        name: "brush",
        unit: "open",
        equipped: true,
        access: true,
        capabilities: [
          {
            id: "models-3d",
            label: "3D",
            ready: false,
            uses: null,
            requires: [use("ffmpeg", "missing", "install FFmpeg")],
            chain: [use("assets-mcp", "session"), use("node", "ok")],
          },
          {
            id: "rig",
            label: "Rig",
            ready: false,
            uses: null,
            requires: [use("ffmpeg", "missing", "install FFmpeg"), use("python", "missing")],
            chain: [],
          },
        ],
      },
      {
        name: "vault",
        unit: "vault",
        equipped: false,
        access: false,
        capabilities: [{ id: "x", label: "X", ready: false, uses: null, requires: [use("java", "missing")], chain: [] }],
      },
    ],
  };

  it("lists each missing provider once, with its fix", () => {
    expect(missingFixes(status)).toEqual([
      { provider: "ffmpeg", label: "FFMPEG", how: "install FFmpeg" },
      // An engine without `how` still says what failed.
      { provider: "python", label: "PYTHON", how: "python detail" },
    ]);
  });

  it("leaves out plugins whose repository is out of reach", () => {
    expect(missingFixes(status).some((f) => f.provider === "java")).toBe(false);
  });
});

describe("pick", () => {
  it("uses the app's language, English when the catalog has none", () => {
    expect(pick({ fr: "Élaguer", en: "Prune" }, "fr")).toBe("Élaguer");
    expect(pick({ fr: "", en: "Prune" }, "fr")).toBe("Prune");
  });
});

describe("plugin ids", () => {
  it("are <name>@<catalog>, from the engine or rebuilt for a single-catalog one", () => {
    expect(idOf(engine.plugins[1])).toBe("sketch@studio");
    expect(idOf({ name: "brush", unit: "open", equipped: null, access: true, capabilities: [] }, { ...engine, catalog: "loadout" })).toBe(
      "brush@loadout",
    );
  });

  it("resolve presets in their own catalog, and bare plugin names when one plugin has it", () => {
    expect(idsOf("sketch", PRESETS, null)).toEqual(["brush@studio", "sketch@studio"]);
    expect(idsOf("tavern", PRESETS, engine)).toEqual(["tavern@crew"]);
    expect(idsOf("sketch@studio", {}, null)).toEqual(["sketch@studio"]);
    // A preset from a single-catalog engine has no catalog: the core's.
    expect(idsOf("old", { old: { label: "", plugins: ["brush"] } }, null)).toEqual(["brush@loadout"]);
  });

  it("expand a folder default into ids, with the core always in", () => {
    expect(expand(["sketch"], PRESETS)).toEqual(["brush@studio", "loadout@loadout", "sketch@studio"]);
  });

  it("key what a project lacks and has", () => {
    expect(missing(expand(["sketch"], PRESETS), ["loadout@loadout", "brush@studio"])).toEqual(["sketch@studio"]);
    expect(presetIsEquipped(PRESETS.sketch, engine)).toBe(true);
    expect(presetIsEquipped(PRESETS.crew, engine)).toBe(false);
  });

  it("keep only the Loadout catalogs of a project's settings", () => {
    const ids = ["code-simplifier@claude-plugins-official", "tavern@crew", "sketch@studio", "loadout@loadout"];
    expect(loadoutIds(ids, catalogsOf(engine, {}))).toEqual(["tavern@crew", "sketch@studio", "loadout@loadout"]);
    // Before the status is in, the presets name the catalogs.
    expect(loadoutIds(ids, catalogsOf(null, PRESETS))).toEqual(["tavern@crew", "sketch@studio", "loadout@loadout"]);
  });
});

describe("the migration offer", () => {
  const old = ["loadout@loadout", "brush@loadout", "sketch@loadout", "tavern@crew"];

  it("flags old <plugin>@loadout entries once the engine has unit catalogs", () => {
    expect(legacyIds(old, engine, {})).toEqual(["brush@loadout", "sketch@loadout"]);
    expect(legacyIds(old, null, PRESETS)).toEqual(["brush@loadout", "sketch@loadout"]);
  });

  it("flags nothing for a single-catalog engine, where those ids are current", () => {
    const single: LoadoutStatus = {
      catalog: "loadout",
      project: null,
      plugins: [{ name: "brush", unit: "studio", equipped: true, access: true, capabilities: [] }],
    };
    expect(legacyIds(old, single, { sketch: { label: "", plugins: ["brush"] } })).toEqual([]);
    expect(legacyIds(old, null, {})).toEqual([]);
  });

  it("never flags the core", () => {
    expect(legacyIds(["loadout@loadout"], engine, PRESETS)).toEqual([]);
  });
});

describe("hidden units", () => {
  it("shows the core and the units added or cloned here, never one out of reach", () => {
    expect(unitsOf(engine)).toEqual([
      { unit: "core", label: "core" },
      { unit: "crew", label: "CREW" },
      { unit: "studio", label: "STUDIO" },
    ]);
  });

  it("builds the pyramid from units[], a visible unit even before its plugins are listed", () => {
    const { core, units } = pyramidOf(engine);
    expect(core?.plugins.map((p) => p.name)).toEqual(["loadout"]);
    expect(units.map((u) => [u.unit, u.plugins.map((p) => p.name)])).toEqual([
      ["crew", ["tavern"]],
      // The base first in its unit.
      ["studio", ["brush", "sketch"]],
    ]);
  });

  it("drops plugins of units not shown and plugins out of reach", () => {
    const status: LoadoutStatus = {
      ...engine,
      plugins: [...engine.plugins, plugin("stray", "guild"), plugin("closed", "studio", { access: false })],
    };
    expect(reachable(status).plugins.map((p) => p.name)).toEqual(["loadout", "sketch", "brush", "tavern"]);
  });

  it("reads units off the plugins for a single-catalog engine, without locked ones", () => {
    const single: LoadoutStatus = {
      catalog: "loadout",
      project: null,
      plugins: [
        { name: "loadout", unit: "open", equipped: null, access: true, capabilities: [] },
        { name: "brush", unit: "studio", equipped: null, access: true, capabilities: [] },
        { name: "tavern", unit: "crew", equipped: null, access: false, capabilities: [] },
      ],
    };
    expect(unitsOf(single).map((u) => u.unit)).toEqual(["core", "studio"]);
    expect(pyramidOf(single).units.map((u) => u.unit)).toEqual(["studio"]);
  });
});

describe("the plug-in action", () => {
  it("is offered for a unit found on GitHub that Claude Code has not added", () => {
    expect(pluggable(engine).map((u) => [u.unit, u.repo])).toEqual([["guild", "ORG/loadout-guild"]]);
  });

  it("is not offered when discovery is unavailable or the unit is added", () => {
    expect(pluggable({ ...engine, discovery: "unavailable", units: [unit("crew", { discovered: false, added: false, local: true })] })).toEqual([]);
    expect(pluggable({ ...engine, units: [unit("studio")] })).toEqual([]);
  });

  it("puts the button on the tile of a cloned unit, and leaves a GitHub-only one out of the pyramid", () => {
    const status: LoadoutStatus = { ...engine, units: [unit("crew", { added: false, local: true, discovered: true }), unit("guild", { added: false })] };
    const { units } = pyramidOf(status);
    expect(units.map((u) => [u.unit, u.info?.discovered && !u.info.added])).toEqual([["crew", true]]);
  });
});

describe("folder defaults", () => {
  it("keys a snooze on the default, whatever its order", () => {
    expect(snoozeKey(["sketch", "brush"])).toBe(snoozeKey(["brush", "sketch"]));
  });

  it("labels presets by name, plugins as they are", () => {
    expect(defaultLabel(["sketch", "extra"], PRESETS)).toBe("Studio project + extra");
  });

  it("tells which units a preset reaches and whether it is locked", () => {
    expect(reachOf("sketch", PRESETS, engine)).toEqual({ units: ["studio"], locked: false });
    expect(reachOf("blank", PRESETS, engine)).toEqual({ units: ["core"], locked: false });
    expect(reachOf("sketch", PRESETS, null)).toEqual({ units: [], locked: false });
    const closed = { ...engine, plugins: [plugin("tavern", "crew", { access: false })] };
    expect(reachOf("crew", PRESETS, closed)).toEqual({ units: ["crew"], locked: true });
  });
});

describe("the core", () => {
  it("is the open unit, and loadout@loadout before the status is in", () => {
    expect(isCore("loadout@loadout", engine)).toBe(true);
    expect(isCore("brush@studio", engine)).toBe(false);
    expect(isCore("loadout@loadout", null)).toBe(true);
    expect(isCore("brush@studio", null)).toBe(false);
  });

  it("is never offered, even though no project's settings list it", () => {
    const status = { ...engine, plugins: engine.plugins.map((p) => ({ ...p, equipped: false })) };
    expect(offerable(status).map((p) => p.name)).toEqual(["sketch", "brush", "tavern"]);
  });

  it("makes a preset of only the core pointless to offer", () => {
    expect(coreOnly(PRESETS.blank, engine)).toBe(true);
    expect(coreOnly(PRESETS.brush, engine)).toBe(false);
  });
});

describe("unit bases", () => {
  it("holds a base while another plugin of its unit is equipped", () => {
    const equipped = ["brush@studio", "sketch@studio"];
    expect(heldBase("brush@studio", equipped, engine)).toBe("studio");
    expect(heldBase("brush@studio", ["brush@studio"], engine)).toBe(null);
    expect(heldBase("sketch@studio", equipped, engine)).toBe(null);
  });
});

describe("the machine's readiness line", () => {
  const line = (locale: "en" | "fr", ready: number, total: number) =>
    translate(locale, "loadout.machine", { n: ready, ready, total });

  it("agrees with the number of ready capabilities, in both languages", () => {
    expect(line("en", 1, 1)).toBe("This machine: 1 capability ready out of 1");
    expect(line("en", 0, 3)).toBe("This machine: 0 capabilities ready out of 3");
    expect(line("fr", 1, 1)).toBe("Cette machine : 1 capacité prête sur 1");
    expect(line("fr", 0, 2)).toBe("Cette machine : 0 capacité prête sur 2");
    expect(line("fr", 2, 3)).toBe("Cette machine : 2 capacités prêtes sur 3");
  });
});

describe("the auto-update toggle", () => {
  it("writes the core's marketplace on the core tile, and an added unit's with its repository", () => {
    const { core, units } = pyramidOf(engine);
    expect(autoUpdateTarget(core!, engine, true)).toEqual({ marketplace: "loadout", repo: null });
    const studio = units.find((u) => u.unit === "studio")!;
    expect(autoUpdateTarget(studio, engine, false)).toEqual({ marketplace: "studio", repo: "ORG/loadout-studio" });
  });

  it("is absent for a unit Claude Code has not added, and for single-catalog engines", () => {
    const crew = pyramidOf(engine).units.find((u) => u.unit === "crew")!;
    expect(autoUpdateTarget(crew, engine, false)).toBe(null);
    expect(autoUpdateTarget({ unit: "studio", plugins: [] }, engine, false)).toBe(null);
  });
});

describe("re-checking the catalogs", () => {
  it("pulls only the Loadout catalogs set to auto-update", () => {
    const autos = { loadout: true, studio: true, crew: false, elsewhere: true };
    expect(catalogsToPull(autos, engine, {})).toEqual(["loadout", "studio"]);
    expect(catalogsToPull(null, engine, {})).toEqual([]);
    // Before any status, the presets name the catalogs.
    expect(catalogsToPull(autos, null, PRESETS)).toEqual(["loadout", "studio"]);
  });

  it("keeps the units GitHub found while the offline status draws first", () => {
    const local: LoadoutStatus = { ...engine, discovery: "skipped", units: engine.units!.map((u) => ({ ...u, discovered: false })).filter((u) => u.unit !== "guild") };
    const kept = keepDiscovered(local, engine);
    expect(kept.discovery).toBe("github");
    expect(kept.units!.map((u) => [u.unit, u.discovered])).toEqual([
      ["studio", true],
      ["crew", false],
      ["guild", true],
    ]);
    expect(keepDiscovered(local, null)).toBe(local);
    expect(keepDiscovered(local, { ...engine, discovery: "unavailable" })).toBe(local);
  });
});

describe("the unit list", () => {
  const withSkills: LoadoutStatus = {
    ...engine,
    plugins: [
      plugin("loadout", "loadout", { skills: [{ name: "setup", description: "Checks this machine" }] }),
      plugin("brush", "studio", { base: true, skills: [{ name: "paint", description: "Paints textures" }, { name: "mix", description: "Mixes colours" }] }),
      plugin("sketch", "studio", { description: "Quick drafts", skills: [{ name: "outline", description: "Draws outlines" }] }),
      plugin("tavern", "crew", { base: true, skills: [] }),
    ],
  };
  const { core, units } = pyramidOf(withSkills);
  const studio = units.find((u) => u.unit === "studio")!;
  const everything = () => true;

  it("counts and keys each unit's skills", () => {
    expect(skillCount(studio)).toBe(3);
    expect(skillKeys(studio)).toEqual(["brush@studio/paint", "brush@studio/mix", "sketch@studio/outline"]);
  });

  it("says which skills are new, and none the first time a unit shows", () => {
    expect(newSkills(studio, undefined).size).toBe(0);
    expect([...newSkills(studio, ["brush@studio/paint", "brush@studio/mix"])]).toEqual(["sketch@studio/outline"]);
  });

  it("searches skill names and descriptions, any case, keeping only what matches", () => {
    const found = shownUnit(studio, "COLOUR", false, everything)!;
    expect(found.searching).toBe(true);
    expect(found.plugins.map((p) => [p.plugin.name, p.skills.map((s) => s.name)])).toEqual([["brush", ["mix"]]]);
    expect(shownUnit(core!, "colour", false, everything)).toBe(null);
  });

  it("shows a whole plugin or unit when its own name or description matches", () => {
    expect(shownUnit(studio, "drafts", false, everything)!.plugins.map((p) => [p.plugin.name, p.skills.length])).toEqual([["sketch", 1]]);
    expect(shownUnit(studio, "studio", false, everything)!.plugins.length).toBe(2);
  });

  it("keeps only the plugins equipped in this project, and drops a unit with none", () => {
    const equipped = (p: LoadoutPlugin) => p.name === "sketch";
    expect(shownUnit(studio, "", true, equipped)!.plugins.map((p) => p.plugin.name)).toEqual(["sketch"]);
    expect(shownUnit(core!, "", true, equipped)).toBe(null);
    expect(shownUnit(studio, "", false, equipped)!.searching).toBe(false);
  });
});
