import { useEffect, useRef, useState } from "react";
import { useTranslate } from "../../../shared/i18n/locale";
import { Copy, PanelRightToggle, Trash2 } from "../../../shared/ui/icons";
import { copyMessage } from "../../../platform/tauri/clipboard";
import { IconButton } from "../../../app/shell/TitleBar";
import { MonoSidebarHeader } from "../../monos/ui/MonoSidebar";
import { PageHeader } from "../../monos/ui/monoPanelParts";
import { artifactLabel, deleteArtifact } from "../artifacts";
import { ArtifactContent, useArtifact } from "./ArtifactContent";

/**
 * A document's title, body and actions. With `onBack` it is a page over a
 * list, whose back button and Escape return to that list.
 */
export function ArtifactReader({
  id,
  onBack,
  onClose,
  onOpenFile,
}: {
  id: string;
  onBack?: () => void;
  onClose: () => void;
  onOpenFile?: (path: string) => void;
}) {
  const leave = onBack ?? onClose;
  const t = useTranslate();
  const { artifact, loaded, error, setError } = useArtifact(id);
  const [copied, setCopied] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => setCopied(false), [id]);
  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !event.defaultPrevented) {
        event.preventDefault();
        leave();
      }
    };
    window.addEventListener("keydown", escape);
    return () => window.removeEventListener("keydown", escape);
  }, [leave]);

  const label = t(artifact ? artifactLabel(artifact.kind) : "Document");
  const noun = label.toLowerCase();
  const onDelete = async () => {
    if (
      !artifact ||
      deleting ||
      !window.confirm(t("Delete “{title}”?", { title: artifact.title }))
    )
      return;
    setDeleting(true);
    setError(null);
    try {
      await deleteArtifact(artifact.id);
      if (mounted.current) leave();
    } catch {
      setError(t("Could not delete this {noun}.", { noun }));
    } finally {
      setDeleting(false);
    }
  };
  const actions = artifact ? (
    <>
      <IconButton
        label={t("Delete {noun}", { noun })}
        disabled={deleting}
        onClick={() => void onDelete()}
      >
        <Trash2 className="size-3.5" />
      </IconButton>
      <IconButton
        label={copied ? t("Copied") : t("Copy {noun}", { noun })}
        onClick={() =>
          void copyMessage(artifact.body).then(
            () => setCopied(true),
            () => setError(t("Could not copy this {noun}.", { noun })),
          )
        }
      >
        <Copy className="size-3.5" />
      </IconButton>
    </>
  ) : null;
  return (
    <>
      {onBack ? (
        <PageHeader title={artifact?.title ?? label} onBack={onBack}>
          {actions}
          <IconButton label={t("Hide overview")} active onClick={onClose}>
            <PanelRightToggle className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        </PageHeader>
      ) : (
        <MonoSidebarHeader title={label} onClose={onClose} actions={actions} />
      )}
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-none px-4 py-3">
        {!loaded ? (
          <p role="status" className="text-[13px] text-content/50">
            {t("Loading document…")}
          </p>
        ) : error && !artifact ? (
          <p role="alert" className="text-[13px] text-content/65">
            {error}
          </p>
        ) : !artifact ? (
          <p role="status" className="text-[13px] text-content/50">
            {t("This document is no longer available.")}
          </p>
        ) : (
          <article data-artifact-reader={artifact.id}>
            <ArtifactContent artifact={artifact} onOpenFile={onOpenFile} />
            {error ? (
              <p role="alert" className="mt-3 text-[12px] text-content/60">
                {error}
              </p>
            ) : null}
            <span role="status" className="sr-only">
              {copied ? t("{label} copied", { label }) : ""}
            </span>
          </article>
        )}
      </div>
    </>
  );
}
