import { useEffect, useState } from "react";
import { useStore } from "../store";
import { useT } from "../i18n";
import {
  catalogsOf,
  defaultLabel,
  expand,
  isCore,
  legacyIds,
  loadoutEquip,
  loadoutIds,
  loadoutMigrate,
  loadoutProject,
  missing,
  nameOf,
  snoozeKey,
} from "../lib/loadout";
import { useLoadout } from "../lib/useLoadout";
import type { Workspace } from "../types";

interface Props {
  workspace: Workspace;
  /** Opens the Loadout tab, for whoever wants the details first. */
  onOpen: () => void;
}

/**
 * A one-line offer above the workspace when its folder has a Loadout default
 * the project does not have yet — the moment work starts is the moment it
 * matters. "Later" holds until the folder's default changes.
 *
 * A project equipped before units had their own catalogs lists them as
 * `<plugin>@loadout`, which no longer loads: then the offer is to update those
 * ids first, rather than to equip plugins it seems to lack.
 */
export function LoadoutOffer({ workspace, onOpen }: Props) {
  const { state, updateWorkspace } = useStore();
  const { ready, presets } = useLoadout();
  const t = useT();
  const folder = state.folders.find((f) => f.id === workspace.folderId);
  const wanted = folder?.loadout?.length ? folder.loadout : null;
  /** Every id the project's settings enable. */
  const [ids, setIds] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setIds(null);
    setError(null);
    if (!ready || !wanted) return;
    let live = true;
    loadoutProject(workspace.cwd)
      .then((found) => live && setIds(found))
      .catch(() => live && setIds(null));
    return () => {
      live = false;
    };
  }, [ready, workspace.cwd, wanted?.join("+")]);

  if (!ready || !wanted || !folder || !ids || !Object.keys(presets).length) return null;
  if (workspace.loadoutSnooze === snoozeKey(wanted)) return null;
  const legacy = legacyIds(ids, null, presets);
  const equipped = loadoutIds(ids, catalogsOf(null, presets));
  // The core is installed for the person, never something a project lacks.
  const lacking = missing(expand(wanted, presets), equipped).filter((id) => !isCore(id, null));
  if (!legacy.length && !lacking.length) return null;

  async function act() {
    setBusy(true);
    setError(null);
    try {
      // Equip migrates too, but the old ids come first: they are why the plugins look missing.
      if (legacy.length) await loadoutMigrate(workspace.cwd);
      else await loadoutEquip(workspace.cwd, wanted!);
      setIds(await loadoutProject(workspace.cwd));
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  const text = legacy.length
    ? t("loadout.offerLegacy", { plugins: legacy.join(", ") })
    : t("loadout.offer", {
        folder: folder.name,
        preset: defaultLabel(wanted, presets),
        plugins: lacking.map(nameOf).join(", "),
      });

  return (
    <div className="lo-offer" role="status">
      <span className="lo-offer__mark" aria-hidden>
        ◆
      </span>
      <span className="lo-offer__text">{error ?? text}</span>
      <button className="btn btn--ghost btn--tiny" onClick={onOpen}>
        {t("loadout.offerDetails")}
      </button>
      <button
        className="btn btn--ghost btn--tiny"
        onClick={() => updateWorkspace(workspace.id, { loadoutSnooze: snoozeKey(wanted) })}
      >
        {t("loadout.offerLater")}
      </button>
      <button className="btn btn--primary btn--tiny" onClick={act} disabled={busy}>
        {legacy.length
          ? busy
            ? t("loadout.migrating")
            : t("loadout.migrate")
          : busy
            ? t("loadout.equipping")
            : t("loadout.equip")}
      </button>
    </div>
  );
}
