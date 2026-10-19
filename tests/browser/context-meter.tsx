import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { ContextMeter } from "../../src/features/sessions/ui/ContextMeter";
import type { ContextUsage } from "../../src/features/sessions/model/contextUsage";
import "../../src/styles/index.css";

declare global {
  interface Window {
    setContextUsage: (usage: ContextUsage | undefined) => void;
    setContextTheme: (theme: "light" | "dark") => void;
    compactCount: number;
  }
}

window.compactCount = 0;

function Fixture() {
  const [usage, setUsage] = useState<ContextUsage | undefined>({
    used: 234_000,
    window: 1_000_000,
    compactionWindow: 400_000,
  });
  useEffect(() => {
    window.setContextUsage = setUsage;
    window.setContextTheme = (theme) => {
      document.documentElement.classList.toggle("theme-light", theme === "light");
      document.body.style.background = theme === "light" ? "#ffffff" : "#1b1b1d";
    };
  }, []);
  return (
    <div className="text-content">
      <ContextMeter
        usage={usage}
        onCompact={() => {
          window.compactCount += 1;
        }}
      />
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Fixture />
  </StrictMode>,
);
