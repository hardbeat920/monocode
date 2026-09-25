import { useEffect, useState } from "react";
import { projectKey, projectName } from "../../../shared/lib/paths";
import { projectLogoSrc } from "../../projects/model/projectLogos";
import { ProjectMascot } from "../../projects/ui/ProjectMascot";
import {
  loadTabGroupColors,
  loadTabGroupCustomColors,
  loadTabGroupLogos,
  loadTabGroupMascots,
  resolveTabGroupColor,
  resolveTabGroupLogo,
  resolveTabGroupMascot,
} from "../../workspace/model/tabGroups";

export function loadQuickProjectAppearance() {
  return {
    logos: loadTabGroupLogos(),
    mascots: loadTabGroupMascots(),
    colors: loadTabGroupColors(),
    customColors: loadTabGroupCustomColors(),
  };
}

/** Project paths identify appearance settings; only saved logo files are images. */
export function QuickProjectIcon({
  projectPath,
  appearance,
  className,
}: {
  projectPath: string;
  appearance: ReturnType<typeof loadQuickProjectAppearance>;
  className?: string;
}) {
  const key = projectKey(projectPath);
  const seed = projectName(projectPath);
  const logoPath = resolveTabGroupLogo(key, appearance.logos);
  const src = projectLogoSrc(logoPath);
  const [failed, setFailed] = useState(false);
  // Reopening reloads appearance and retries a logo that may have been replaced.
  useEffect(() => setFailed(false), [src, appearance]);

  if (src && !failed)
    return (
      <img
        src={src}
        alt=""
        className={`rounded-sm object-cover ${className ?? ""}`}
        onError={() => setFailed(true)}
      />
    );
  return (
    <ProjectMascot
      project={seed}
      name={resolveTabGroupMascot(key, appearance.mascots)}
      color={resolveTabGroupColor(
        key,
        appearance.colors,
        appearance.customColors,
        seed,
      )}
      className={className}
    />
  );
}
