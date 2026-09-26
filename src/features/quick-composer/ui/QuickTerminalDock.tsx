import { useLayoutEffect, useState } from "react";
import { Plus, Terminal, X } from "../../../shared/ui/icons";
import { pathKey } from "../../../shared/lib/paths";
import {
  defaultTerminalTitle,
  type TerminalMetaPatch,
} from "../../terminal/model/terminalTab";
import { TerminalView } from "../../terminal/ui/TerminalView";

type QuickTerminal = {
  id: string;
  cwd: string;
  title: string;
};

type TerminalGroup = {
  tabs: QuickTerminal[];
  activeId: string;
};

function groupKey(cwd: string): string {
  return cwd ? pathKey(cwd) : "";
}

function newTerminal(cwd: string, tabs: QuickTerminal[]): QuickTerminal {
  const base = cwd ? defaultTerminalTitle(cwd) : "Terminal";
  const taken = new Set(tabs.map((tab) => tab.title));
  let title = base;
  for (let index = 2; taken.has(title); index += 1) {
    title = `${base} ${index}`;
  }
  return { id: `quick-composer:${crypto.randomUUID()}`, cwd, title };
}

function newGroup(cwd: string): TerminalGroup {
  const first = newTerminal(cwd, []);
  return { tabs: [first], activeId: first.id };
}

/** Each project keeps its own live terminals while the composer is hidden. */
export function QuickTerminalDock({
  cwd,
  active,
}: {
  cwd: string;
  active: boolean;
}) {
  const selectedKey = groupKey(cwd);
  const [groups, setGroups] = useState<Record<string, TerminalGroup>>(() => ({
    [selectedKey]: newGroup(cwd),
  }));

  // Opening the terminal after changing projects creates that project's first
  // shell before paint. Merely browsing projects does not spawn shells.
  useLayoutEffect(() => {
    if (!active || groups[selectedKey]) return;
    setGroups((current) =>
      current[selectedKey]
        ? current
        : { ...current, [selectedKey]: newGroup(cwd) },
    );
  }, [active, cwd, groups, selectedKey]);

  const addTerminal = (key: string, projectCwd: string) => {
    setGroups((current) => {
      const group = current[key] ?? { tabs: [], activeId: "" };
      const tab = newTerminal(projectCwd, group.tabs);
      return {
        ...current,
        [key]: {
          tabs: [...group.tabs, tab],
          activeId: tab.id,
        },
      };
    });
  };

  const closeTerminal = (key: string, id: string) => {
    setGroups((current) => {
      const group = current[key];
      if (!group) return current;
      const index = group.tabs.findIndex((tab) => tab.id === id);
      if (index < 0) return current;
      const tabs = group.tabs.filter((tab) => tab.id !== id);
      return {
        ...current,
        [key]: {
          tabs,
          activeId:
            group.activeId === id
              ? (tabs[Math.min(index, tabs.length - 1)]?.id ?? "")
              : group.activeId,
        },
      };
    });
  };

  const updateTerminal = (
    key: string,
    id: string,
    patch: TerminalMetaPatch,
  ) => {
    setGroups((current) => {
      const group = current[key];
      if (!group) return current;
      return {
        ...current,
        [key]: {
          ...group,
          tabs: group.tabs.map((tab) =>
            tab.id === id
              ? {
                  ...tab,
                  cwd: patch.cwd ?? tab.cwd,
                  title: patch.title ?? tab.title,
                }
              : tab,
          ),
        },
      };
    });
  };

  return (
    <div className="h-full min-h-0">
      {Object.entries(groups).map(([key, group]) => (
        <div
          key={key}
          className={
            key === selectedKey ? "flex h-full min-h-0 flex-col" : "hidden"
          }
          aria-hidden={key !== selectedKey}
        >
          <div className="flex h-9 shrink-0 border-b border-stroke">
            <div
              role="tablist"
              aria-label="Floating terminals"
              className="scrollbar-none flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto overscroll-none px-1.5"
            >
              {group.tabs.map((tab) => {
                const selected = tab.id === group.activeId;
                return (
                  <div
                    key={tab.id}
                    className={`group relative flex h-7.5 min-w-28 max-w-52 flex-1 items-center rounded-md ${selected ? "bg-selection text-content" : "text-content/50 hover:bg-content/5 hover:text-content"}`}
                  >
                    <button
                      type="button"
                      role="tab"
                      aria-selected={selected}
                      title={`${tab.title} — ${tab.cwd}`}
                      onClick={() =>
                        setGroups((current) => ({
                          ...current,
                          [key]: { ...current[key], activeId: tab.id },
                        }))
                      }
                      className="flex h-full min-w-0 flex-1 cursor-default items-center gap-1.5 pl-2 pr-7 text-left text-[13px]"
                    >
                      <Terminal
                        className="size-3.5 shrink-0"
                        strokeWidth={1.75}
                      />
                      <span className="truncate">{tab.title}</span>
                    </button>
                    <button
                      type="button"
                      aria-label={`Close terminal ${tab.title}`}
                      title={`Close ${tab.title}`}
                      onClick={() => closeTerminal(key, tab.id)}
                      className={`absolute right-1 grid size-5 place-items-center rounded hover:bg-content/10 hover:text-content ${selected ? "text-content/60" : "text-content/50 opacity-0 group-hover:opacity-100"}`}
                    >
                      <X className="size-3" strokeWidth={1.75} />
                    </button>
                  </div>
                );
              })}
            </div>
            <button
              type="button"
              aria-label="New floating terminal"
              title="New terminal"
              onClick={() => addTerminal(key, cwd)}
              className="grid size-9 shrink-0 place-items-center text-content/50 hover:bg-selection-hover hover:text-content"
            >
              <Plus className="size-4" strokeWidth={1.75} />
            </button>
          </div>
          <div className="relative min-h-0 flex-1">
            {group.tabs.map((tab) => (
              <div
                key={tab.id}
                aria-hidden={tab.id !== group.activeId}
                className={
                  tab.id === group.activeId ? "absolute inset-0" : "hidden"
                }
              >
                <TerminalView
                  id={tab.id}
                  cwd={tab.cwd}
                  active={
                    active && key === selectedKey && tab.id === group.activeId
                  }
                  onMetaChange={(patch) => updateTerminal(key, tab.id, patch)}
                />
              </div>
            ))}
            {group.tabs.length === 0 ? (
              <p className="grid h-full place-items-center text-xs text-content/40">
                No terminals open
              </p>
            ) : null}
          </div>
        </div>
      ))}
    </div>
  );
}
