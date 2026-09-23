// Node 22+ exposes Web Storage only as lazy getters that warn — and yield
// undefined — unless --localstorage-file is set. Read the property descriptor
// instead of the property itself: an existing value storage (e.g. happy-dom)
// is kept as-is, while a bare getter is replaced before it is ever invoked so
// no worker emits the ExperimentalWarning.
function installMemoryStorage(name: "localStorage" | "sessionStorage") {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, name);
  if (descriptor?.value && typeof descriptor.value.clear === "function") return;
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
