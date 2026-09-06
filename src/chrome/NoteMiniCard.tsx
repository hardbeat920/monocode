import { File, X } from "./icons";
import { useState } from "react";
import { t } from "../lib/i18n";
import { ProjectLogoIcon } from "./ProjectLogoIcon";
import { ProjectMascot } from "./ProjectMascot";
import { useTabGroupLogos } from "../hooks/useTabGroupLogos";
import {
  noteSourceProject,
  type NoteCardMeta,
} from "../lib/notes";
import {
  loadTabGroupColors,
  loadTabGroupCustomColors,
  loadTabGroupMascots,
  resolveTabGroupColor,
  resolveTabGroupLogo,
  resolveTabGroupMascot,
} from "../lib/tabGroups";

type Props = {
  card: NoteCardMeta;
  onDismiss?: () => void;
  embedded?: boolean;
};

export function NoteMiniCard({ card, onDismiss, embedded = false }: Props) {
  const logos = useTabGroupLogos();
  const [mascots] = useState(loadTabGroupMascots);
  const [colors] = useState(loadTabGroupColors);
  const [customColors] = useState(loadTabGroupCustomColors);
  const project = noteSourceProject(card.sourceCwd);
  const logoPath = project ? resolveTabGroupLogo(project, logos) : null;
  const mascotName = project ? resolveTabGroupMascot(project, mascots) : null;
  const mascotColor = project
    ? resolveTabGroupColor(project, colors, customColors, project)
    : undefined;

  const inner = (
    <div
      className={`relative rounded-md border border-content/10 bg-content/6 px-2.5 py-2 ${
        onDismiss ? "pr-8" : ""
      }`}
    >
      <div className="flex w-full flex-col text-left">
        <span className="flex min-w-0 items-center gap-1.5">
          <File
            className="size-3.5 shrink-0 text-content/45"
            strokeWidth={1.75}
          />
          <span className="min-w-0 truncate text-[11px] text-content/50">
            {t("Note")}{!embedded && card.slug ? ` · ${card.slug}` : ""}
          </span>
        </span>
        <span className="mt-1 line-clamp-1 text-[13px] font-semibold leading-snug text-content">
          {card.title || t("Untitled")}
        </span>
        {!embedded && project ? (
          <span className="mt-1 flex min-w-0 items-center gap-1.5 text-[11px] text-content/45">
            {logoPath ? (
              <ProjectLogoIcon
                path={logoPath}
                className="size-3.5 shrink-0 rounded-sm"
                imageClassName="size-3.5"
              />
            ) : (
              <ProjectMascot
                project={project}
                color={mascotColor}
                name={mascotName}
                className="size-3 shrink-0"
              />
            )}
            <span className="min-w-0 truncate">{project}</span>
          </span>
        ) : null}
      </div>
      {onDismiss ? (
        <button
          type="button"
          title={t("Remove")}
          aria-label={t("Remove note {0}", [card.title || t("Untitled")])}
          onClick={onDismiss}
          className="absolute right-1.5 top-1.5 grid size-5 place-items-center rounded text-content/40 hover:bg-content/10 hover:text-content"
        >
          <X className="size-3" strokeWidth={2} />
        </button>
      ) : null}
    </div>
  );

  if (embedded) return inner;
  return <div className="px-3 pt-2">{inner}</div>;
}
