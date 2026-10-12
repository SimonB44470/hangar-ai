import type { Translator } from "../../i18n";
import { useLoadout } from "../../lib/useLoadout";

/** Whether the switched-on add-on finds the catalog's engine on this machine. */
export function LoadoutCard({ t }: { t: Translator }) {
  const { enabled, installed } = useLoadout();
  if (!enabled || installed === null) return null;
  return (
    <p className={`lo-setting ${installed ? "lo-setting--ok" : "lo-setting--missing"}`}>
      {installed ? t("settings.loadoutFound") : t("settings.loadoutMissing")}
    </p>
  );
}
