import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { BackgroundActivity } from "../../src/features/sessions/ui/BackgroundActivity";
import "../../src/styles/index.css";

function Fixture() {
  const [props, setProps] = useState({
    tasks: ["pi-subagents"],
    busy: true,
    visible: true,
    interrupted: false,
  });
  useEffect(() => {
    Object.assign(window, { setBackgroundActivity: setProps });
  }, []);
  return (
    <main className="mx-auto max-w-xl p-4">
      <BackgroundActivity {...props} />
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<Fixture />);
