import { useState, type Ref } from "react";
import { Modal } from "../../../shared/ui/Modal";
import type { MonoLook, MonoState } from "../model/mono";
import { SoulPage, useMonoFiles } from "./MonoFilePages";

export function MonoSoulDialog({
  monoId,
  agent,
  status,
  saveRef,
  frozen,
  onClose,
}: {
  monoId: string;
  agent: MonoLook;
  status: MonoState["status"];
  saveRef: Ref<() => Promise<boolean>>;
  frozen: boolean;
  onClose: () => void;
}) {
  const [loadError, setLoadError] = useState(false);
  const files = useMonoFiles(monoId, status, setLoadError);
  return (
    <Modal
      title={`${agent.name} soul`}
      size="lg"
      minimalHeader
      fitViewport
      onClose={onClose}
      className="h-[min(680px,calc(100dvh-32px))] [&>.modal-panel>header]:top-1.5"
    >
      <div inert={frozen || undefined} className="flex h-full min-h-0 flex-col">
        {loadError && !files ? (
          <p role="alert" className="p-4 text-sm text-red-400">
            Could not load the Soul. Close and try again.
          </p>
        ) : (
          <SoulPage
            monoId={monoId}
            agent={agent}
            files={files}
            saveRef={saveRef}
          />
        )}
      </div>
    </Modal>
  );
}
