// Node 22+ exposes Web Storage only when --localstorage-file is set. Without
// that flag the globals are missing and happy-dom does not fill them in.
function installMemoryStorage(name: "localStorage" | "sessionStorage") {
  const current = (globalThis as Record<string, unknown>)[name];
  if (current && typeof (current as { clear?: unknown }).clear === "function") return;
  const store = new Map<string, string>();
  const storage: Storage = {
    get length() {
      return store.size;
    },
    clear() {
      store.clear();
    },
    getItem(key) {
      return store.get(String(key)) ?? null;
    },
    key(index) {
      return [...store.keys()][index] ?? null;
    },
    removeItem(key) {
      store.delete(String(key));
    },
    setItem(key, value) {
      store.set(String(key), String(value));
    },
  };
  Object.defineProperty(globalThis, name, {
    value: storage,
    configurable: true,
    writable: true,
  });
}

installMemoryStorage("localStorage");
installMemoryStorage("sessionStorage");
