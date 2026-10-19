import {
  useCallback,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { historyHosts } from "./useMeasuredHistory";

/** A stable portal keeps offscreen content mounted without retaining layout. */
export function HistoryTurn({
  id,
  className,
  children,
}: {
  id: string;
  className: string;
  children: ReactNode;
}) {
  const server = useSyncExternalStore(
    noSubscription,
    clientSnapshot,
    serverSnapshot,
  );
  const [attached, setAttached] = useState(false);
  const [host] = useState(() => {
    if (typeof document === "undefined") return null;
    const element = document.createElement("div");
    element.style.display = "contents";
    element.dataset.historyContent = "";
    return element;
  });
  const attach = useCallback(
    (row: HTMLDivElement | null) => {
      if (!host) return;
      if (!row) {
        host.remove();
        return;
      }
      historyHosts.set(row, host);
      row.appendChild(host);
      // Descendant layout effects must see a connected host (prompt sizing,
      // selection and focus all rely on its real ancestors).
      setAttached(true);
    },
    [host],
  );
  return (
    <div ref={attach} data-transcript-turn={id} className={className}>
      {server || !host ? (
        <div data-history-content style={{ display: "contents" }}>
          {children}
        </div>
      ) : attached ? (
        createPortal(children, host)
      ) : null}
    </div>
  );
}

const noSubscription = () => () => {};
const clientSnapshot = () => false;
const serverSnapshot = () => true;
