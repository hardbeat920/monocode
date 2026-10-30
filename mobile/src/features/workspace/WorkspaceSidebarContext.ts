import { createContext, use } from "react";

export const WorkspaceSidebarContext = createContext({
  wide: false,
  open: false,
  toggle: () => {},
});
export const useWorkspaceSidebar = () => use(WorkspaceSidebarContext);
