import { useEffect, useState } from "react";
import { useStore } from "../store";
import { loadoutLocate, loadoutPresets, type Preset } from "./loadout";

/**
 * Asked once per window: whether the engine is installed, and the catalog's
 * presets. Neither changes while the app runs, and the tab, the sidebar, the
 * new-workspace dialog and the offer to equip all read them.
 */
let located: Promise<boolean> | null = null;
let presetsOnce: Promise<Record<string, Preset>> | null = null;

const locate = () =>
  (located ??= loadoutLocate()
    .then(Boolean)
    .catch(() => false));

const presetsOf = () =>
  (presetsOnce ??= loadoutPresets().catch(() => ({}) as Record<string, Preset>));

export interface LoadoutAddon {
  /** The add-on is switched on in the settings. */
  enabled: boolean;
  /** The engine was found; `null` until known, and while the add-on is off. */
  installed: boolean | null;
  /** Empty until known, or when the engine is missing. */
  presets: Record<string, Preset>;
  /** Enabled and installed: what every Loadout feature waits for. */
  ready: boolean;
}

export function useLoadout(): LoadoutAddon {
  const { state } = useStore();
  const enabled = state.settings.loadoutEnabled;
  const [installed, setInstalled] = useState<boolean | null>(null);
  const [presets, setPresets] = useState<Record<string, Preset>>({});

  useEffect(() => {
    if (!enabled) return;
    let live = true;
    locate().then((found) => {
      if (!live) return;
      setInstalled(found);
      if (found) presetsOf().then((p) => live && setPresets(p));
    });
    return () => {
      live = false;
    };
  }, [enabled]);

  return {
    enabled,
    installed: enabled ? installed : null,
    presets,
    ready: enabled && installed === true,
  };
}
