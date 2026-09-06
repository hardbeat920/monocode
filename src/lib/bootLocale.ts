import { setLocale } from "./i18n";

/**
 * Sets the app locale. Imported FIRST in main.tsx — before App and every
 * other module — because catalogs (models.ts, harness catalogs) call t() at
 * module-evaluation time and would otherwise freeze English strings.
 */
setLocale("zh-CN");
