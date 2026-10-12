import { useCallback, useEffect, useState } from "react";
import {
  catalogsOf,
  coreOnly,
  expand,
  idOf,
  isCore,
  legacyIds,
  loadoutEquip,
  loadoutIds,
  loadoutMigrate,
  loadoutPlug,
  loadoutUnequip,
  loadoutProject,
  loadoutPrompts,
  loadoutStatus,
  missing,
  missingFixes,
  nameOf,
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
  type LoadoutUnit,
  type ProviderUse,
} from "../lib/loadout";
import { useLoadout } from "../lib/useLoadout";
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

function PluginCard({ plugin }: { plugin: LoadoutPlugin }) {
  const t = useT();
  return (
    <section className={`lo-plugin ${plugin.equipped ? "lo-plugin--equipped" : ""}`}>
      <header className="lo-plugin__head">
        <h3 className="lo-plugin__name">{plugin.name}</h3>
        <em className="lo-badge lo-badge--unit" title={t("loadout.unitHint")}>
          {unitLabel(plugin.unit)}
        </em>
        {!plugin.access ? (
          <em className="lo-badge lo-badge--muted">🔒 {t("loadout.locked")}</em>
        ) : plugin.equipped ? (
          <em className="lo-badge">{t("loadout.equipped")}</em>
        ) : (
          <em className="lo-badge lo-badge--muted">{t("loadout.available")}</em>
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

/** A unit out of reach is never listed, so a tile has no lock; one found on GitHub but not added offers "plug in". */
function UnitTile({
  block,
  base,
  plugging,
  onPlug,
}: {
  block: UnitBlock;
  base?: boolean;
  plugging?: string | null;
  onPlug?: (unit: LoadoutUnit) => void;
}) {
  const t = useT();
  const plug = block.info?.discovered && !block.info.added ? block.info : null;
  return (
    <section className={`lo-tier ${base ? "lo-tier--base" : ""}`} title={t("loadout.unitOpen")}>
      <header className="lo-tier__head">
        <span className="lo-tier__name" title={block.label}>
          ✓ {block.unit}
        </span>
        <span className="lo-tier__who">{base ? t("loadout.tierCore") : t("loadout.tierUnit")}</span>
        {plug && onPlug && <PlugButton unit={plug} busy={plugging === plug.unit} onPlug={onPlug} />}
      </header>
      <ul className="lo-tier__plugins">
        {block.plugins.map((plugin) => (
          <li key={idOf(plugin)}>
            <strong>{plugin.name}</strong>
            {plugin.base && <em className="lo-badge lo-badge--base">{t("loadout.base")}</em>}
            {plugin.description && <span className="lo-tier__desc"> — {plugin.description}</span>}
            {plugin.skills && plugin.skills.length > 0 && (
              <span className="lo-tier__skills">
                {plugin.skills.map((skill) => (
                  <span key={skill.name} className="lo-chip" title={skill.description}>
                    {skill.name}
                  </span>
                ))}
              </span>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

/** What each unit holds, as a pyramid: the core everyone stands on, the private units on top. */
function UnitsPyramid({
  status,
  plugging,
  onPlug,
}: {
  status: LoadoutStatus;
  plugging: string | null;
  onPlug: (unit: LoadoutUnit) => void;
}) {
  const t = useT();
  const { core, units } = pyramidOf(status);
  return (
    <details className="lo-fold" open>
      <summary>
        <span className="lo-fold__title">{t("loadout.pyramid")}</span>
        <span className="lo-fold__sub">{t("loadout.pyramidHint")}</span>
      </summary>
      <div className="lo-pyramid">
        {units.length > 0 && (
          <div className="lo-pyramid__units" style={{ gridTemplateColumns: units.map((u) => `${Math.max(1, u.plugins.length)}fr`).join(" ") }}>
            {units.map((block) => (
              <UnitTile key={block.unit} block={block} plugging={plugging} onPlug={onPlug} />
            ))}
          </div>
        )}
        {core && <UnitTile block={core} base />}
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
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [extra, setExtra] = useState("");
  const [equipping, setEquipping] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  /** `null` until known, and for an engine without prompts: no panel either way. */
  const [prompts, setPrompts] = useState<LoadoutPrompt[] | null>(null);

  const refresh = useCallback(() => {
    setChecking(true);
    setError(null);
    loadoutProject(cwd)
      .then(setProjectIds)
      .catch(() => undefined);
    loadoutStatus(cwd)
      .then((next) => {
        lastStatus.set(cwd, next);
        setStatus(next);
      })
      .catch((err) => setError(String(err)))
      .finally(() => setChecking(false));
  }, [cwd]);

  // Mounted once per workspace (keyed on it by the caller), so a check still
  // running for the previous one cannot land here.
  useEffect(() => {
    if (installed) refresh();
  }, [installed, refresh]);

  useEffect(() => {
    if (!installed) return;
    loadoutPrompts()
      .then((found) => setPrompts(found?.prompts ?? null))
      .catch(() => setPrompts(null));
  }, [installed]);

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

  async function plug(unit: LoadoutUnit) {
    setPlugging(unit.unit);
    setError(null);
    try {
      await loadoutPlug(unit.repo);
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
  const mine = plugins.filter((p) => equippedIds.includes(idOf(p, status)));
  const others = plugins.filter((p) => !equippedIds.includes(idOf(p, status)));
  // Units found on GitHub with no clone here are not in the pyramid: they are offered on their own.
  const toPlug = status ? pluggable(status).filter((u) => !u.local) : [];
  // Readiness of what this project uses; of the whole catalog until it uses something.
  const scope = mine.length ? mine : plugins;
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
            <span className="loadout__hint">{checking ? t("loadout.checking") : ""}</span>
            <button className="btn btn--ghost btn--tiny" onClick={refresh} disabled={checking}>
              {t("loadout.refresh")}
            </button>
          </header>

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

        {!status && !error && <p className="loadout__none">{t("loadout.firstCheck")}</p>}

        {status && <UnitsPyramid status={status} plugging={plugging} onPlug={plug} />}

        {status && (
          <section className={`lo-card lo-machine ${fixes.length ? "lo-machine--warn" : ""}`}>
            <header className="lo-card__head">
              <h3>{t("loadout.machine", { ready: readyCount, total: capabilities.length })}</h3>
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
              <PluginCard key={idOf(plugin, status)} plugin={plugin} />
            ))}
          </div>
        )}

        {others.length > 0 && (
          <details className="lo-fold">
            <summary>
              <span className="lo-fold__title">
                {t("loadout.otherPlugins", { n: others.length })}
              </span>
            </summary>
            <div className="loadout__plugins lo-fold__body">
              {others.map((plugin) => (
                <PluginCard key={idOf(plugin, status)} plugin={plugin} />
              ))}
            </div>
          </details>
        )}

        {prompts && prompts.length > 0 && <PromptsPanel prompts={prompts} />}
      </div>
    </div>
  );
}
