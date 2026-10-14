import { useState } from "react";
import { createRoot } from "react-dom/client";
import { ScrollableTabStrip } from "../../src/shared/ui/ScrollableTabStrip";
import "../../src/styles/index.css";

function Fixture() {
  const [width, setWidth] = useState(240);
  const [sources, setSources] = useState([
    "GitHub",
    "Bitbucket",
    "Sonar",
    "Linear",
    "Jira",
  ]);
  const [active, setActive] = useState("GitHub");
  return (
    <>
      <button onClick={() => setWidth(700)}>Expand</button>
      <button onClick={() => setWidth(240)}>Narrow</button>
      <button onClick={() => setSources(["GitHub"])}>Remove connections</button>
      <button onClick={() => setActive("Jira")}>Select last</button>
      <div
        data-row
        style={{ width }}
        className="flex h-9 items-center border border-stroke px-2"
      >
        <ScrollableTabStrip
          label="Inbox source"
          activeKey={active}
          add={{
            label: "Add connection",
            ariaLabel: "Connect an inbox source",
            expanded: false,
            onClick() {},
          }}
        >
          {sources.map((source) => (
            <button
              key={source}
              role="tab"
              aria-selected={source === active}
              onClick={() => setActive(source)}
              className="h-6 shrink-0 whitespace-nowrap px-2 text-[12px]"
            >
              {source}
            </button>
          ))}
        </ScrollableTabStrip>
      </div>
    </>
  );
}
createRoot(document.getElementById("root")!).render(<Fixture />);
