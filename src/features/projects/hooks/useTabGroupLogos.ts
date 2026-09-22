import { useEffect, useState } from "react";
import {
  loadTabGroupLogos,
  TAB_GROUP_LOGOS_CHANGED,
} from "../../workspace/model/tabGroups";

export function useTabGroupLogos(): Record<string, string> {
  const [logos, setLogos] = useState(loadTabGroupLogos);
  useEffect(() => {
    const refresh = () => setLogos(loadTabGroupLogos());
    window.addEventListener(TAB_GROUP_LOGOS_CHANGED, refresh);
    return () => window.removeEventListener(TAB_GROUP_LOGOS_CHANGED, refresh);
  }, []);
  return logos;
}
