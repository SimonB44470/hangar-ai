import { useEffect, useState } from "react";
import { useStore } from "../store";
import { loadoutLocate, loadoutPresets, type Preset } from "./loadout";

/**
 * Shared by every window part that reads them (the tab, the sidebar, the
 * new-workspace dialog, the offer to equip): whether the engine is installed,
 * asked once, and the catalogs' presets, asked once and again whenever the tab
 * re-reads the catalogs (`reloadPresets`) — a catalog can gain a project type
 * while the app runs.
 */
let located: Promise<boolean> | null = null;
let presetsOnce: Promise<Record<string, Preset>> | null = null;
const listeners = new Set<(presets: Record<string, Preset>) => void>();

const locate = () =>
  (located ??= loadoutLocate()
    .then(Boolean)
    .catch(() => false));

const presetsOf = () =>
  (presetsOnce ??= loadoutPresets().catch(() => ({}) as Record<string, Preset>));

/** Asks the engine for the presets again and hands them to every reader. Keeps the old ones if it fails. */
export async function reloadPresets(): Promise<Record<string, Preset>> {
  const previous = presetsOnce;
  const next = loadoutPresets();
  try {
    const presets = await next;
    presetsOnce = next;
    for (const listener of listeners) listener(presets);
    return presets;
  } catch {
    return previous ? previous : {};
  }
}

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
    const update = (next: Record<string, Preset>) => live && setPresets(next);
    listeners.add(update);
    locate().then((found) => {
      if (!live) return;
      setInstalled(found);
      if (found) presetsOf().then(update);
    });
    return () => {
      live = false;
      listeners.delete(update);
    };
  }, [enabled]);

  return {
    enabled,
    installed: enabled ? installed : null,
    presets,
    ready: enabled && installed === true,
  };
}
