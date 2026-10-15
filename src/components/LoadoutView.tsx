import { useCallback, useEffect, useRef, useState } from "react";
import {
  autoUpdateTarget,
  catalogsOf,
  catalogsToPull,
  coreOnly,
  expand,
  idOf,
  isCore,
  keepDiscovered,
  newSkills,
  shownUnit,
  skillCount,
  skillKey,
  skillKeys,
  legacyIds,
  loadoutEquip,
  loadoutIds,
  loadoutAutoUpdates,
  loadoutMigrate,
  loadoutPlug,
  loadoutSetAutoUpdate,
  loadoutUpdateMarketplace,
  loadoutUnequip,
  loadoutProject,
  loadoutPrompts,
  loadoutStatus,
  missing,
  missingFixes,
  nameOf,
  offerable,
  pick,
  heldBase,
  pluggable,
  presetIds,
  pyramidOf,
  reachable,
  reachOf,
  unitLabel,
  unitsOf,
  type LoadoutPrompt,
  type UnitBlock,
  type LoadoutPlugin,
  type LoadoutStatus,
  type AutoUpdateTarget,
  type LoadoutUnit,
  type ShownPlugin,
  type ShownUnit,
  type ProviderUse,
} from "../lib/loadout";
import { reloadPresets, useLoadout } from "../lib/useLoadout";
import { useStore } from "../store";
import { useLocale, useT } from "../i18n";
import type { Workspace } from "../types";

interface Props {
  /** Active workspace: the project whose `.claude/settings.json` is read and
      equipped. Key the view on it: its state belongs to one workspace. */
  workspace: Workspace;
}

/**
 * Last status per workspace, so coming back to the tab draws at once while the
 * engine checks again — a check asks GitHub about private plugins and takes
 * seconds.
 */
const lastStatus = new Map<string, LoadoutStatus>();

/** Back on the window after this long, the tab checks again (locally: no pull, no GitHub). */
const FOCUS_RECHECK_MS = 60_000;
/** While the tab stays open, the auto-updated catalogs are pulled this often. */
const PERIODIC_PULL_MS = 30 * 60_000;

const STATE_GLYPH: Record<ProviderUse["state"], string> = {
  ok: "✓",
  missing: "✕",
  session: "○",
};

function ProviderLine({ use, required }: { use: ProviderUse; required: boolean }) {
  const t = useT();
  return (
    <li className={`lo-use lo-use--${use.state}`}>
      <span className="lo-use__glyph" aria-hidden>
        {STATE_GLYPH[use.state]}
      </span>
      <span className="lo-use__label">
        {required ? t("loadout.needs", { label: use.label }) : use.label}
      </span>
      {use.ask_before && <em className="lo-badge lo-badge--paid">{t("loadout.asksFirst")}</em>}
      {use.cost && <span className="lo-use__cost">{use.cost}</span>}
      {use.state === "missing" && <span className="lo-use__detail">{use.detail}</span>}
      {use.state === "session" && <span className="lo-use__detail">{t("loadout.session")}</span>}
    </li>
  );
}

/** `core`: installed for the person, so always on whatever the project's settings say. */
function PluginCard({ plugin, core }: { plugin: LoadoutPlugin; core?: boolean }) {
  const t = useT();
  return (
    <section className={`lo-plugin ${plugin.equipped || core ? "lo-plugin--equipped" : ""}`}>
      <header className="lo-plugin__head">
        <h3 className="lo-plugin__name">{plugin.name}</h3>
        <em className="lo-badge lo-badge--unit" title={t("loadout.unitHint")}>
          {unitLabel(plugin.unit)}
        </em>
        {!plugin.access ? (
          <em className="lo-badge lo-badge--muted">🔒 {t("loadout.locked")}</em>
        ) : core ? (
          <em className="lo-badge">{t("loadout.alwaysOn")}</em>
        ) : (
          <em className="lo-badge">{t("loadout.equipped")}</em>
        )}
      </header>
      {plugin.access &&
        plugin.capabilities.map((capability) => (
          <div key={capability.id} className="lo-cap">
            <p className={`lo-cap__label ${capability.ready ? "is-ready" : "is-blocked"}`}>
              <span className="lo-cap__dot" aria-hidden />
              {capability.label}
            </p>
            <ul className="lo-cap__uses">
              {capability.requires.map((use) => (
                <ProviderLine key={`r-${use.provider}`} use={use} required />
              ))}
              {capability.chain.map((use) => (
                <ProviderLine key={`c-${use.provider}`} use={use} required={false} />
              ))}
            </ul>
          </div>
        ))}
      {!plugin.access && <p className="lo-plugin__note">{t("loadout.lockedHint")}</p>}
    </section>
  );
}

function PromptCard({ prompt }: { prompt: LoadoutPrompt }) {
  const t = useT();
  const locale = useLocale();
  const [copied, setCopied] = useState(false);
  const text = pick(prompt.text, locale);

  function copy() {
    navigator.clipboard
      .writeText(text)
      .then(() => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      })
      .catch(() => undefined);
  }

  return (
    <article className={`lo-prompt ${prompt.soon ? "lo-prompt--soon" : ""}`}>
      <header className="lo-prompt__head">
        <h4 className="lo-prompt__title">{pick(prompt.title, locale)}</h4>
        {prompt.tutorial && <em className="lo-badge">{t("loadout.tutorial")}</em>}
        {prompt.soon && <em className="lo-badge lo-badge--muted">{t("loadout.soon")}</em>}
        {/* A tutorial is read, not pasted; a prompt not usable yet is not offered. */}
        {!prompt.tutorial && !prompt.soon && (
          <button className="btn btn--ghost lo-prompt__copy" onClick={copy}>
            {copied ? t("loadout.copied") : t("loadout.copy")}
          </button>
        )}
      </header>
      <p className="lo-prompt__when">{pick(prompt.when, locale)}</p>
      <pre className="lo-prompt__text">{text}</pre>
      {!prompt.tutorial && !prompt.soon && text.includes("{skill}") && (
        <p className="lo-prompt__hint">{t("loadout.skillPlaceholder")}</p>
      )}
    </article>
  );
}

/** "Plug in" for a unit found on GitHub that Claude Code does not know yet. */
function PlugButton({ unit, busy, onPlug }: { unit: LoadoutUnit; busy: boolean; onPlug: (unit: LoadoutUnit) => void }) {
  const t = useT();
  return (
    <button
      className="btn btn--primary btn--tiny"
      onClick={() => onPlug(unit)}
      disabled={busy}
      title={t("loadout.plugHint", { repo: unit.repo })}
    >
      {busy ? t("loadout.plugging") : t("loadout.plugIn")}
    </button>
  );
}

/** The list's auto-update state: `null` when the user settings could not be read, and then no toggle shows. */
interface AutoUpdates {
  state: Record<string, boolean> | null;
  /** The marketplace being written. */
  busy: string | null;
  onToggle: (target: AutoUpdateTarget, on: boolean) => void;
}

/** Per unit, the skills already seen, so a new one can say so. Per machine, in local storage. */
const SEEN_KEY = "hangar.loadout.seenSkills";

function readSeen(): Record<string, string[]> {
  try {
    const value = JSON.parse(localStorage.getItem(SEEN_KEY) ?? "{}");
    return value && typeof value === "object" ? value : {};
  } catch {
    return {};
  }
}

function writeSeen(seen: Record<string, string[]>) {
  try {
    localStorage.setItem(SEEN_KEY, JSON.stringify(seen));
  } catch {
    // Storage off: "new" badges just do not persist.
  }
}

function PluginRow({
  shown,
  equipped,
  fresh,
  open,
  onToggle,
}: {
  shown: ShownPlugin;
  equipped: boolean;
  /** Keys of the skills new to this person. */
  fresh: Set<string>;
  open: boolean;
  onToggle: () => void;
}) {
  const t = useT();
  const { plugin, skills } = shown;
  return (
    <li className="lo-unit-plugin">
      <div className="lo-unit-plugin__head">
        <strong>{plugin.name}</strong>
        {plugin.base && <em className="lo-badge lo-badge--base">{t("loadout.base")}</em>}
        {equipped && <em className="lo-badge">{t("loadout.equippedHereBadge")}</em>}
        {plugin.description && (
          <span className="lo-unit-plugin__desc" title={plugin.description}>
            {plugin.description}
          </span>
        )}
        {skills.length > 0 && (
          <button className="lo-unit-plugin__skills" onClick={onToggle} aria-expanded={open}>
            {t("loadout.skillCount", { n: skills.length })} {open ? "▾" : "▸"}
          </button>
        )}
      </div>
      {open && skills.length > 0 && (
        <ul className="lo-skill-list">
          {skills.map((skill) => (
            <li key={skill.name}>
              <code>{skill.name}</code>
              {fresh.has(skillKey(plugin, skill)) && <em className="lo-badge lo-badge--new">{t("loadout.newBadge")}</em>}
              <span className="lo-skill-list__desc" title={skill.description}>
                {skill.description}
              </span>
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}

/**
 * What this person can use, one compact row per unit (the core first): name,
 * how many plugins and skills, what is new since the last look, auto-update.
 * A row opens on its plugins, one line each; a plugin opens on its skills. A
 * search across every skill and an "equipped in this project" filter keep it
 * readable with many units.
 */
function UnitsList({
  status,
  busy,
  plugging,
  onPlug,
  auto,
  isEquipped,
}: {
  status: LoadoutStatus;
  /** A check is running: the list dims until it lands. */
  busy: boolean;
  plugging: string | null;
  onPlug: (unit: LoadoutUnit) => void;
  auto: AutoUpdates;
  isEquipped: (plugin: LoadoutPlugin) => boolean;
}) {
  const t = useT();
  const { core, units } = pyramidOf(status);
  const blocks = core ? [core, ...units] : units;
  const [query, setQuery] = useState("");
  const [equippedOnly, setEquippedOnly] = useState(false);
  const [openUnits, setOpenUnits] = useState<Set<string>>(() => new Set());
  const [openPlugins, setOpenPlugins] = useState<Set<string>>(() => new Set());
  const [seen, setSeen] = useState(readSeen);
  /** What was new when a unit was opened: its skills keep their badge while it stays open. */
  const [fresh, setFresh] = useState<Record<string, Set<string>>>({});

  // A unit seen for the first time: all of it counts as seen.
  const unseen = blocks.filter((block) => !seen[block.unit]);
  useEffect(() => {
    if (!unseen.length) return;
    const next = { ...seen };
    for (const block of unseen) next[block.unit] = skillKeys(block);
    setSeen(next);
    writeSeen(next);
  }, [unseen.map((b) => b.unit).join("+")]); // eslint-disable-line react-hooks/exhaustive-deps

  const flip = (set: Set<string>, key: string) => {
    const next = new Set(set);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    return next;
  };

  function toggleUnit(block: UnitBlock) {
    const opening = !openUnits.has(block.unit);
    setOpenUnits((open) => flip(open, block.unit));
    if (!opening) return;
    const found = newSkills(block, seen[block.unit]);
    if (!found.size) return;
    setFresh((all) => ({ ...all, [block.unit]: found }));
    // Shown now: no longer new next time.
    const next = { ...seen, [block.unit]: skillKeys(block) };
    setSeen(next);
    writeSeen(next);
  }

  const shown = blocks
    .map((block) => shownUnit(block, query, equippedOnly, isEquipped))
    .filter((unit): unit is ShownUnit => Boolean(unit));

  return (
    <details className={`lo-fold ${busy ? "is-busy" : ""}`} open>
      <summary>
        <span className="lo-fold__title">{t("loadout.pyramid")}</span>
        <span className="lo-fold__sub">{t("loadout.pyramidHint")}</span>
      </summary>
      <div className="lo-units">
        <div className="lo-units__tools">
          <input
            type="search"
            className="lo-units__search"
            placeholder={t("loadout.searchSkills")}
            aria-label={t("loadout.searchSkills")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <label className="lo-units__filter">
            <input type="checkbox" checked={equippedOnly} onChange={(e) => setEquippedOnly(e.target.checked)} />
            {t("loadout.filterEquipped")}
          </label>
        </div>
        {shown.length === 0 && (
          <p className="lo-row__hint">
            {query.trim() ? t("loadout.noMatch", { query: query.trim() }) : t("loadout.nothingEquipped")}
          </p>
        )}
        <ul className="lo-units__list">
          {shown.map(({ block, plugins, searching }) => {
            const isCoreBlock = block === core;
            const open = searching || openUnits.has(block.unit);
            const news = newSkills(block, seen[block.unit]).size;
            const target = autoUpdateTarget(block, status, isCoreBlock);
            const plug = block.info?.discovered && !block.info.added ? block.info : null;
            return (
              <li key={block.unit} className={`lo-unit ${open ? "is-open" : ""} ${isCoreBlock ? "lo-unit--core" : ""}`}>
                <div className="lo-unit__row">
                  <button
                    className="lo-unit__toggle"
                    onClick={() => toggleUnit(block)}
                    aria-expanded={open}
                    title={isCoreBlock ? t("loadout.tierCore") : block.label}
                  >
                    <span className="lo-unit__caret" aria-hidden>
                      {open ? "▾" : "▸"}
                    </span>
                    <span className="lo-unit__name">{block.unit}</span>
                    <span className="lo-unit__counts">
                      {t("loadout.pluginCount", { n: block.plugins.length })} ·{" "}
                      {t("loadout.skillCount", { n: skillCount(block) })}
                    </span>
                    {news > 0 && <em className="lo-badge lo-badge--new">{t("loadout.newSkills", { n: news })}</em>}
                  </button>
                  {plug && <PlugButton unit={plug} busy={plugging === plug.unit} onPlug={onPlug} />}
                  {target && auto.state && (
                    <label className="lo-unit__auto" title={t("loadout.autoUpdateHint")}>
                      <input
                        type="checkbox"
                        checked={auto.state[target.marketplace] === true}
                        disabled={auto.busy === target.marketplace}
                        onChange={(e) => auto.onToggle(target, e.target.checked)}
                      />
                      {t("loadout.autoUpdate")}
                    </label>
                  )}
                </div>
                {open && (
                  <ul className="lo-unit__plugins">
                    {plugins.map((item) => {
                      const id = idOf(item.plugin);
                      return (
                        <PluginRow
                          key={id}
                          shown={item}
                          equipped={isEquipped(item.plugin)}
                          fresh={fresh[block.unit] ?? new Set()}
                          open={searching || openPlugins.has(id)}
                          onToggle={() => setOpenPlugins((all) => flip(all, id))}
                        />
                      );
                    })}
                  </ul>
                )}
              </li>
            );
          })}
        </ul>
      </div>
    </details>
  );
}

/** Collapsed by default: it is read once in a while, the status every time. */
function PromptsPanel({ prompts }: { prompts: LoadoutPrompt[] }) {
  const t = useT();
  return (
    <details className="lo-fold">
      <summary>
        <span className="lo-fold__title">{t("loadout.prompts")}</span>
        <span className="lo-fold__sub">{t("loadout.promptsHint")}</span>
      </summary>
      <div className="lo-prompts__list">
        {prompts.map((prompt) => (
          <PromptCard key={prompt.id} prompt={prompt} />
        ))}
      </div>
    </details>
  );
}

export function LoadoutView({ workspace }: Props) {
  const t = useT();
  const { state, updateFolder } = useStore();
  const { installed, presets } = useLoadout();
  const cwd = workspace.cwd;
  const folder = state.folders.find((f) => f.id === workspace.folderId) ?? null;
  const [status, setStatus] = useState<LoadoutStatus | null>(() => lastStatus.get(cwd) ?? null);
  /** Every id the settings file enables: known at once, while the full check takes seconds. */
  const [projectIds, setProjectIds] = useState<string[]>([]);
  /** The unit being plugged in. */
  const [plugging, setPlugging] = useState<string | null>(null);
  /** Each marketplace's auto-update, from the user settings; `null` when unreadable. */
  const [autoUpdates, setAutoUpdates] = useState<Record<string, boolean> | null>(null);
  const [toggling, setToggling] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  /** Catalogs being pulled right now, and the ones that failed, with why. */
  const [pulling, setPulling] = useState<string[]>([]);
  const [pullErrors, setPullErrors] = useState<Record<string, string>>({});
  /** The local status is in; GitHub is still being asked which units this account can read. */
  const [discovering, setDiscovering] = useState(false);
  /** Each check supersedes the one before: a slower old answer must not land over a newer one. */
  const generation = useRef(0);
  const lastCheck = useRef(0);
  const presetsRef = useRef(presets);
  presetsRef.current = presets;
  const [error, setError] = useState<string | null>(null);
  const [extra, setExtra] = useState("");
  const [equipping, setEquipping] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  /** `null` until known, and for an engine without prompts: no panel either way. */
  const [prompts, setPrompts] = useState<LoadoutPrompt[] | null>(null);

  /**
   * Reads everything again. `pull` first pulls the catalogs set to auto-update
   * (in parallel, each reporting on its own): the engine reads Claude Code's
   * marketplace clones, which otherwise only move when a new session starts.
   * Then the local status draws at once and the GitHub one fills in after.
   */
  const refresh = useCallback(
    async (pull = false) => {
      const run = ++generation.current;
      const live = () => run === generation.current;
      lastCheck.current = Date.now();
      setChecking(true);
      setError(null);
      loadoutProject(cwd)
        .then((ids) => live() && setProjectIds(ids))
        .catch(() => undefined);
      const autos = await loadoutAutoUpdates().catch(() => null);
      if (!live()) return;
      setAutoUpdates(autos);
      if (pull) {
        const names = catalogsToPull(autos, lastStatus.get(cwd) ?? null, presetsRef.current);
        setPullErrors({});
        setPulling(names);
        await Promise.all(
          names.map((name) =>
            loadoutUpdateMarketplace(name)
              .catch((err) => live() && setPullErrors((all) => ({ ...all, [name]: String(err) })))
              .finally(() => live() && setPulling((left) => left.filter((n) => n !== name))),
          ),
        );
        if (!live()) return;
      }
      // A pulled catalog can bring project types and prompts too.
      void reloadPresets();
      loadoutPrompts()
        .then((found) => live() && setPrompts(found?.prompts ?? null))
        .catch(() => live() && setPrompts(null));
      try {
        const local = keepDiscovered(await loadoutStatus(cwd, true), lastStatus.get(cwd) ?? null);
        if (!live()) return;
        lastStatus.set(cwd, local);
        setStatus(local);
        setDiscovering(true);
        const full = await loadoutStatus(cwd);
        if (!live()) return;
        lastStatus.set(cwd, full);
        setStatus(full);
      } catch (err) {
        if (live()) setError(String(err));
      } finally {
        if (live()) {
          setChecking(false);
          setDiscovering(false);
          setPulling([]);
        }
      }
    },
    [cwd],
  );

  // Mounted once per workspace (keyed on it by the caller), so a check still
  // running for the previous one cannot land here.
  useEffect(() => {
    if (installed) void refresh();
  }, [installed, refresh]);

  // Coming back to the window after a while: a quick local check. While the
  // tab stays open: the auto-updated catalogs are pulled now and then.
  useEffect(() => {
    if (!installed) return;
    const onFocus = () => {
      if (Date.now() - lastCheck.current > FOCUS_RECHECK_MS) void refresh();
    };
    window.addEventListener("focus", onFocus);
    const timer = window.setInterval(() => void refresh(true), PERIODIC_PULL_MS);
    return () => {
      window.removeEventListener("focus", onFocus);
      window.clearInterval(timer);
    };
  }, [installed, refresh]);

  async function unequip(name: string) {
    setEquipping(true);
    setError(null);
    try {
      const report = await loadoutUnequip(cwd, [name]);
      const warnings = report.warnings?.length ? ` ⚠ ${report.warnings.join(" · ")}` : "";
      setNotice(t("loadout.unequippedNotice", { plugins: report.removed.map(nameOf).join(", ") || nameOf(name) }) + warnings);
      refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setEquipping(false);
    }
  }

  async function equip(names: string[]) {
    if (!names.length) return;
    setEquipping(true);
    setError(null);
    try {
      const report = await loadoutEquip(cwd, names);
      setNotice(t("loadout.equippedNotice", { plugins: report.plugins.map(nameOf).join(", ") }));
      setExtra("");
      refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setEquipping(false);
    }
  }

  async function migrate() {
    setEquipping(true);
    setError(null);
    try {
      const report = await loadoutMigrate(cwd);
      const moved = (report.migrated ?? []).map((m) => nameOf(m.to));
      const left = report.unresolved ?? [];
      setNotice(
        [
          moved.length ? t("loadout.migratedNotice", { plugins: moved.join(", ") }) : "",
          left.length ? t("loadout.unresolved", { plugins: left.join(", ") }) : "",
        ]
          .filter(Boolean)
          .join(" "),
      );
      refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setEquipping(false);
    }
  }

  async function toggleAutoUpdate(target: AutoUpdateTarget, on: boolean) {
    setToggling(target.marketplace);
    setError(null);
    try {
      await loadoutSetAutoUpdate(target.marketplace, target.repo, on);
      setAutoUpdates(await loadoutAutoUpdates());
    } catch (err) {
      setError(String(err));
    } finally {
      setToggling(null);
    }
  }

  async function plug(unit: LoadoutUnit) {
    setPlugging(unit.unit);
    setError(null);
    try {
      await loadoutPlug(unit.repo, unit.catalog);
      setNotice(t("loadout.pluggedNotice", { unit: unit.label || unit.unit }));
      refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setPlugging(null);
    }
  }

  if (installed === false) {
    return (
      <div className="loadout">
        <div className="loadout__body">
          <section className="lo-card lo-missing">
            <h3>{t("loadout.notInstalled")}</h3>
            <p>{t("loadout.notInstalledBody")}</p>
          </section>
        </div>
      </div>
    );
  }

  const folderDefault = folder?.loadout ?? [];
  const legacy = legacyIds(projectIds, status, presets);
  const equippedIds = loadoutIds(projectIds, catalogsOf(status, presets)).filter((id) => !legacy.includes(id));
  const wanted = folderDefault.length ? expand(folderDefault, presets, status) : [];
  const lacking = missing(wanted, equippedIds).filter((id) => !isCore(id, status));
  const plugins = status ? reachable(status).plugins : [];
  // The core is always on, so it counts as this project's own and is never offered.
  const mine = plugins.filter((p) => isCore(idOf(p, status), status) || equippedIds.includes(idOf(p, status)));
  const others = status ? offerable(status).filter((p) => !equippedIds.includes(idOf(p, status))) : [];
  // Units found on GitHub with no clone here are not in the pyramid: they are offered on their own.
  const toPlug = status ? pluggable(status).filter((u) => !u.local) : [];
  // Readiness of what this project uses; of the whole catalog until it uses something.
  const scope = mine.some((p) => !isCore(idOf(p, status), status)) ? mine : plugins;
  const capabilities = scope.flatMap((p) => (p.access ? p.capabilities : []));
  const readyCount = capabilities.filter((c) => c.ready).length;
  const fixes = status ? missingFixes({ ...status, plugins: scope }) : [];
  const units = status ? unitsOf(status) : [];
  /** "Studio project — brush, sketch · studio", locked when this account lacks access. */
  const optionOf = (name: string, label: string) => {
    const reach = reachOf(name, presets, status);
    const where = reach.units.length ? ` · ${reach.units.join(", ")}` : "";
    return { label: `${reach.locked ? "🔒 " : ""}${label}${where}`, locked: reach.locked };
  };
  const addable = [
    ...Object.entries(presets)
      .filter(([id, p]) => !coreOnly(p, status) && !reachOf(id, presets, status).locked)
      .filter(([, p]) => missing(presetIds(p, status), equippedIds).length > 0)
      .map(([id, p]) => ({ id, ...optionOf(id, `${p.label} — ${p.plugins.join(", ")}`) })),
    ...others
      .filter((p) => p.access && !presets[p.name])
      .map((p) => ({ id: idOf(p, status), ...optionOf(idOf(p, status), p.name) })),
  ];

  return (
    <div className="loadout">
      <div className="loadout__body">
        <section className="lo-card lo-project">
          <header className="lo-card__head">
            <h3>{t("loadout.thisProject")}</h3>
            <span className="loadout__hint" aria-live="polite">
              {checking && <span className="lo-spinner" aria-hidden />}
              {pulling.length
                ? t("loadout.updatingCatalogs", { names: pulling.join(", ") })
                : discovering
                  ? t("loadout.discovering")
                  : checking
                    ? t("loadout.checking")
                    : ""}
            </span>
            <button
              className="btn btn--ghost btn--tiny"
              onClick={() => void refresh(true)}
              disabled={checking}
              title={t("loadout.refreshHint")}
            >
              {t("loadout.refresh")}
            </button>
          </header>

          {Object.entries(pullErrors).map(([name, why]) => (
            <p key={name} className="loadout__error">
              {t("loadout.updateFailed", { name, error: why })}
            </p>
          ))}

          {units.length > 0 && (
            <div className="lo-row">
              <span className="lo-row__label">{t("loadout.myUnits")}</span>
              <span className="lo-chips">
                {units.map((u) => (
                  <span key={u.unit} className="lo-chip lo-chip--unit" title={u.label}>
                    ✓ {u.unit}
                  </span>
                ))}
              </span>
            </div>
          )}

          {toPlug.length > 0 && (
            <div className="lo-row">
              <span className="lo-row__label">{t("loadout.toPlug")}</span>
              {toPlug.map((u) => (
                <span key={u.unit} className="lo-chips">
                  <span className="lo-chip" title={u.repo}>
                    {u.label || u.unit} · {u.repo}
                  </span>
                  <PlugButton unit={u} busy={plugging === u.unit} onPlug={plug} />
                </span>
              ))}
            </div>
          )}

          {status?.discovery === "unavailable" && <p className="lo-row__hint">{t("loadout.discoveryOff")}</p>}

          <div className="lo-row">
            <span className="lo-row__label">{t("loadout.folderDefault")}</span>
            {folder ? (
              <>
                <span className="lo-folder">{folder.name}</span>
                <select
                  className="loadout__preset"
                  value={folderDefault[0] ?? ""}
                  onChange={(e) =>
                    updateFolder(folder.id, { loadout: e.target.value ? [e.target.value] : [] })
                  }
                  aria-label={t("loadout.folderDefault")}
                >
                  <option value="">{t("loadout.noDefault")}</option>
                  {Object.entries(presets)
                    .filter(
                      ([id, p]) =>
                        folderDefault[0] === id ||
                        (!coreOnly(p, status) && !reachOf(id, presets, status).locked),
                    )
                    .map(([id, p]) => {
                    const option = optionOf(id, `${p.label} — ${p.plugins.join(", ")}`);
                    return (
                      <option
                        key={id}
                        value={id}
                        disabled={option.locked && folderDefault[0] !== id}
                      >
                        {option.label}
                      </option>
                    );
                  })}
                </select>
              </>
            ) : (
              <span className="lo-row__hint">{t("loadout.noFolder")}</span>
            )}
          </div>

          <div className="lo-row">
            <span className="lo-row__label">{t("loadout.equippedHere")}</span>
            <span className="lo-chips">
              <span className="lo-chip lo-chip--core" title={t("loadout.coreAlways")}>
                loadout · {t("loadout.alwaysOn")}
              </span>
              {equippedIds
                .filter((id) => !isCore(id, status))
                .map((id) => {
                  const name = nameOf(id);
                  const heldBy = heldBase(id, equippedIds, status);
                  return heldBy ? (
                    <span key={id} className="lo-chip lo-chip--core" title={t("loadout.baseHeld", { unit: heldBy })}>
                      {name} · {t("loadout.baseOf", { unit: heldBy })}
                    </span>
                  ) : (
                  <span
                    key={id}
                    className={`lo-chip lo-chip--removable ${wanted.includes(id) ? "lo-chip--inherited" : ""}`}
                    title={`${id} · ${wanted.includes(id) ? t("loadout.fromFolder") : t("loadout.addedHere")}`}
                  >
                    {name}
                    <button
                      className="lo-chip__remove"
                      onClick={() => unequip(id)}
                      disabled={equipping}
                      title={t("loadout.unequip", { plugin: name })}
                      aria-label={t("loadout.unequip", { plugin: name })}
                    >
                      ×
                    </button>
                  </span>
                  );
                })}
            </span>
          </div>

          {legacy.length > 0 && (
            <div className="lo-row lo-row--warn">
              <span className="lo-row__label">{t("loadout.legacy", { plugins: legacy.join(", ") })}</span>
              <button className="btn btn--primary btn--tiny" onClick={migrate} disabled={equipping}>
                {equipping ? t("loadout.migrating") : t("loadout.migrate")}
              </button>
            </div>
          )}

          {lacking.length > 0 && (
            <div className="lo-row lo-row--warn">
              <span className="lo-row__label">{t("loadout.lacking")}</span>
              <span className="lo-chips">
                {lacking.map((id) => (
                  <span key={id} className="lo-chip lo-chip--missing" title={id}>
                    {nameOf(id)}
                  </span>
                ))}
              </span>
              <button
                className="btn btn--primary btn--tiny"
                onClick={() => equip(folderDefault)}
                disabled={equipping}
              >
                {equipping ? t("loadout.equipping") : t("loadout.equipDefault")}
              </button>
            </div>
          )}

          <div className="lo-row">
            <span className="lo-row__label">{t("loadout.addMore")}</span>
            <select
              className="loadout__preset"
              value={extra}
              onChange={(e) => setExtra(e.target.value)}
              aria-label={t("loadout.addMore")}
            >
              <option value="">{t("loadout.choosePreset")}</option>
              {addable.map((option) => (
                <option key={option.id} value={option.id} disabled={option.locked}>
                  {option.label}
                </option>
              ))}
            </select>
            <button
              className="btn btn--tiny"
              onClick={() => equip([extra])}
              disabled={!extra || equipping}
              title={t("loadout.equipHint")}
            >
              {t("loadout.add")}
            </button>
          </div>

          {error && <p className="loadout__error">{error}</p>}
          {notice && <p className="loadout__notice">{notice}</p>}
        </section>

        {!status && !error && (
          <div className="lo-skeleton" aria-busy="true">
            <p className="loadout__none">{t("loadout.firstCheck")}</p>
            <span className="lo-skeleton__bar" />
            <span className="lo-skeleton__bar" />
            <span className="lo-skeleton__bar lo-skeleton__bar--short" />
          </div>
        )}

        {status && (
          <UnitsList
            busy={checking}
            isEquipped={(plugin) => isCore(idOf(plugin, status), status) || equippedIds.includes(idOf(plugin, status))}
            status={status}
            plugging={plugging}
            onPlug={plug}
            auto={{ state: autoUpdates, busy: toggling, onToggle: toggleAutoUpdate }}
          />
        )}

        {status && (
          <section className={`lo-card lo-machine ${fixes.length ? "lo-machine--warn" : ""}`}>
            <header className="lo-card__head">
              <h3>{t("loadout.machine", { n: readyCount, ready: readyCount, total: capabilities.length })}</h3>
            </header>
            {fixes.length > 0 ? (
              <ul className="lo-fixes__list">
                {fixes.map((fix) => (
                  <li key={fix.provider}>
                    <strong>{fix.label}</strong> — {fix.how}
                  </li>
                ))}
              </ul>
            ) : (
              <p className="lo-row__hint">{t("loadout.machineReady")}</p>
            )}
          </section>
        )}

        {mine.length > 0 && (
          <div className="loadout__plugins">
            {mine.map((plugin) => (
              <PluginCard key={idOf(plugin, status)} plugin={plugin} core={isCore(idOf(plugin, status), status)} />
            ))}
          </div>
        )}

        {prompts && prompts.length > 0 && <PromptsPanel prompts={prompts} />}
      </div>
    </div>
  );
}
