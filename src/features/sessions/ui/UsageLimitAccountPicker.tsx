import {
  useId,
  useRef,
  useState,
  useSyncExternalStore,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { ChevronDown } from "../../../shared/ui/icons";
import { Popover } from "../../../shared/ui/Popover";
import type { HarnessId } from "../model/session";
import { HarnessIcon } from "./HarnessIcon";
import {
  providerAccounts,
  sameProviderAccountId,
  subscribeProviderAccounts,
  supportsProviderAccounts,
  type ProviderAccount,
} from "../../providers/model/providerAccounts";

/** The provider's other accounts, offered when the current one is limited. */
export function UsageLimitAccountPicker({
  harness,
  accountId,
  onSelect,
  variant = "mono",
}: {
  harness: HarnessId;
  accountId?: string;
  onSelect: (accountId: string) => void;
  variant?: "composer" | "mono";
}) {
  const snapshot = useSyncExternalStore(
    subscribeProviderAccounts,
    () =>
      supportsProviderAccounts(harness)
        ? JSON.stringify(providerAccounts(harness))
        : "[]",
    () => "[]",
  );
  const accounts = (JSON.parse(snapshot) as ProviderAccount[]).filter(
    (account) => !sameProviderAccountId(account.id, accountId),
  );
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const button = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const dismiss = (restoreFocus = false) => {
    setOpen(false);
    if (restoreFocus) button.current?.focus();
  };
  const pick = (account: ProviderAccount) => {
    dismiss(true);
    onSelect(account.id);
  };
  if (accounts.length === 0) return null;
  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const direction = event.key === "ArrowDown" ? 1 : -1;
      setActive(
        (index) => (index + direction + accounts.length) % accounts.length,
      );
    } else if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      setActive(event.key === "Home" ? 0 : accounts.length - 1);
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      const account = accounts[active];
      if (account) pick(account);
    } else if (event.key === "Escape") {
      event.preventDefault();
      dismiss(true);
    } else if (event.key === "Tab") {
      dismiss();
    }
  };

  return (
    <>
      <button
        ref={button}
        type="button"
        aria-label="Choose another account"
        aria-haspopup="menu"
        aria-expanded={open}
        data-mono-account-picker
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => {
          if (open) dismiss(true);
          else {
            setActive(0);
            setOpen(true);
          }
        }}
        className={
          variant === "mono"
            ? "flex h-6.5 shrink-0 items-center gap-1 rounded-md bg-selection px-1.5 text-[11px] text-content hover:bg-selection-hover"
            : "flex h-6 shrink-0 items-center gap-1 rounded-md px-1.5 hover:bg-content/10 hover:text-content"
        }
      >
        <span>
          {variant === "mono" ? "Choose another account" : "Switch account"}
        </span>
        <ChevronDown
          className={`size-3 shrink-0 text-content/50 ${open ? "rotate-180" : ""}`}
          strokeWidth={1.75}
        />
      </button>
      {open ? (
        <Popover
          anchor={button}
          side="top"
          width={250}
          maxHeight={260}
          autoFocus
          onDismiss={(reason) => dismiss(reason === "escape")}
          ignore="[data-mono-account-picker]"
          role="menu"
          aria-label="Choose another account"
          aria-activedescendant={`${menuId}-${active}`}
          tabIndex={-1}
          onKeyDown={onKeyDown}
          className="overflow-y-auto p-1 font-sans text-content"
        >
          {accounts.map((account, index) => (
            <button
              key={account.id}
              id={`${menuId}-${index}`}
              type="button"
              role="menuitem"
              tabIndex={-1}
              aria-label={account.label}
              onMouseDown={(event) => event.preventDefault()}
              onMouseEnter={() => setActive(index)}
              onClick={() => pick(account)}
              className={`flex h-9 w-full items-center gap-2 rounded-lg px-2 text-left text-[13px] ${index === active ? "bg-selection" : "hover:bg-content/5"}`}
            >
              <HarnessIcon
                harness={harness}
                className="size-3.5 shrink-0 text-content/55"
              />
              <span className="min-w-0 flex-1 truncate">{account.label}</span>
            </button>
          ))}
        </Popover>
      ) : null}
    </>
  );
}
