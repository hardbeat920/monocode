import { Folder, type IconComponent } from "../../../shared/ui/icons";
import { useEffect, useState } from "react";
import { projectLogoSrc } from "../model/projectLogos";
import {
  TAB_GROUP_LOGOS_CHANGED,
  tabGroupLogoDisplayRevision,
} from "../../workspace/model/tabGroups";

type Props = {
  path?: string | null;
  className?: string;
  imageClassName?: string;
  fallback?: IconComponent;
  fallbackStrokeWidth?: number;
};

export function ProjectLogoIcon({
  path,
  className = "size-3.5 shrink-0",
  imageClassName,
  fallback: Fallback = Folder,
  fallbackStrokeWidth = 1.5,
}: Props) {
  const [revision, setRevision] = useState(tabGroupLogoDisplayRevision);

  useEffect(() => {
    const refresh = () => setRevision(tabGroupLogoDisplayRevision());
    window.addEventListener(TAB_GROUP_LOGOS_CHANGED, refresh);
    return () => window.removeEventListener(TAB_GROUP_LOGOS_CHANGED, refresh);
  }, []);

  const src = projectLogoSrc(path);
  if (src) {
    return (
      <img
        key={`${path ?? ""}:${revision}`}
        src={src}
        alt=""
        className={`rounded-sm object-cover ${className} ${imageClassName ?? ""}`}
      />
    );
  }
  return (
    <Fallback className={className} strokeWidth={fallbackStrokeWidth} />
  );
}
