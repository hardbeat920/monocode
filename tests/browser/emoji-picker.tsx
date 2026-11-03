import { StrictMode, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { EmojiPickerPopover } from "../../src/shared/ui/EmojiPicker";
import "../../src/styles/index.css";

// The real picker and its bundled data, opened from a button like the
// composer's, with each pick written out for the test to read.
function Fixture() {
  const button = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(true);
  const [picked, setPicked] = useState("");
  return (
    <div className="text-content">
      <button ref={button} type="button" onClick={() => setOpen(true)}>
        Open
      </button>
      <output data-picked>{picked}</output>
      {open ? (
        <EmojiPickerPopover
          anchor={button}
          onPick={(emoji) => {
            setPicked((current) => current + emoji);
            setOpen(false);
          }}
          onDismiss={() => setOpen(false)}
        />
      ) : null}
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Fixture />
  </StrictMode>,
);
