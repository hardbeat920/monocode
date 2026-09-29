import { describe, expect, it, beforeEach } from "vitest";
import {
  loadLanguagePreference,
  saveLanguagePreference,
  resolveLanguage,
  t,
} from "./i18n";

function mockLocalStorage() {
  const data = new Map<string, string>();
  const storage = {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
    clear: () => {
      data.clear();
    },
  };
  Object.defineProperty(globalThis, "localStorage", {
    value: storage,
    configurable: true,
    writable: true,
  });
}

describe("i18n", () => {
  beforeEach(() => {
    mockLocalStorage();
    localStorage.clear();
  });

  it("defaults to system preference", () => {
    expect(loadLanguagePreference()).toBe("system");
  });

  it("persists language preference", () => {
    saveLanguagePreference("zh-CN");
    expect(loadLanguagePreference()).toBe("zh-CN");
    saveLanguagePreference("en");
    expect(loadLanguagePreference()).toBe("en");
  });

  it("resolves language explicitly", () => {
    expect(resolveLanguage("en")).toBe("en");
    expect(resolveLanguage("zh-CN")).toBe("zh-CN");
  });

  it("translates known keys in zh-CN", () => {
    saveLanguagePreference("zh-CN");
    expect(t("common.save", "Save")).toBe("保存");
    expect(t("nav.settings", "Settings")).toBe("设置");
    expect(t("settings.general.title", "General")).toBe("常规");
  });

  it("falls back to English when in en mode", () => {
    saveLanguagePreference("en");
    expect(t("common.save", "Save")).toBe("Save");
    expect(t("nav.settings", "Settings")).toBe("Settings");
  });

  it("falls back gracefully for missing keys", () => {
    expect(t("non.existent.key", "Custom Fallback")).toBe("Custom Fallback");
  });

  it("translates model setting and option keys correctly", () => {
    saveLanguagePreference("zh-CN");
    expect(t("models.settings.reasoning", "Reasoning")).toBe("推理强度");
    expect(t("models.settings.effort", "Effort")).toBe("推理强度");
    expect(t("models.settings.thinking", "Thinking")).toBe("思考");
    expect(t("models.settings.fast", "Fast")).toBe("快速");
    expect(t("models.settings.serviceTier", "Service Tier")).toBe("服务等级");
    expect(t("models.settings.variant", "Variant")).toBe("变体");
    expect(t("models.settings.agent", "Agent")).toBe("智能体");
    expect(t("models.options.low", "Low")).toBe("低");
    expect(t("models.options.medium", "Medium")).toBe("中");
    expect(t("models.options.high", "High")).toBe("高");
    expect(t("models.options.extraHigh", "Extra High")).toBe("超高");
    expect(t("models.options.max", "Max")).toBe("最高");
    expect(t("models.options.standard", "Standard")).toBe("标准");
    expect(t("models.options.on", "On")).toBe("开");
    expect(t("models.options.off", "Off")).toBe("关");

    saveLanguagePreference("en");
    expect(t("models.settings.reasoning", "Reasoning")).toBe("Reasoning");
    expect(t("models.settings.effort", "Effort")).toBe("Effort");
    expect(t("models.settings.thinking", "Thinking")).toBe("Thinking");
    expect(t("models.options.high", "High")).toBe("High");
    expect(t("models.options.extraHigh", "Extra High")).toBe("Extra High");
  });

  it("translates category 5 placeholder keys correctly", () => {
    saveLanguagePreference("zh-CN");
    expect(t("nav.searchProjectsPlaceholder", "Search projects...")).toBe("搜索项目...");
    expect(t("settings.searchSettingsPlaceholder", "Search settings")).toBe("搜索设置");
    expect(t("settings.autoDetectedPathPlaceholder", "Auto-detected path")).toBe("自动检测的路径");
    expect(t("settings.accountNamePlaceholder", "Work or Personal")).toBe("工作或个人");
    expect(t("settings.jiraApiTokenPlaceholder", "API token")).toBe("API Token");
    expect(t("files.goToFilePlaceholder", "Go to File (type > for commands)")).toBe("转到文件（输入 > 运行命令）");
    expect(t("files.findPlaceholder", "Find")).toBe("查找");
    expect(t("inbox.searchChatsPlaceholder", "Search chats...")).toBe("搜索会话...");
    expect(t("sessions.searchModelsOrHarnessesPlaceholder", "Search models or harnesses…")).toBe("搜索模型或服务商…");
    expect(t("sessions.searchModelsPlaceholder", "Search models")).toBe("搜索模型");
    expect(t("sessions.chooseSessionFolderPlaceholder", "Choose or name a session folder…")).toBe("选择或命名会话文件夹…");
    expect(t("sessions.findInConversationPlaceholder", "Find in conversation")).toBe("在会话中查找");
    expect(t("sessions.askSideQuestionPlaceholder", "Ask a side question…")).toBe("顺带提问…");
    expect(t("turn.typeYourAnswerPlaceholder", "Type your answer")).toBe("输入你的回答");
    expect(t("sourceControl.searchBaseBranchesPlaceholder", "Search base branches…")).toBe("搜索基准分支…");
    expect(t("search.filesToIncludePlaceholder", "files to include")).toBe("包含的文件");
    expect(t("search.filesToExcludePlaceholder", "files to exclude")).toBe("排除的文件");
    expect(t("automations.searchTriggersPlaceholder", "Search triggers")).toBe("搜索触发器");

    saveLanguagePreference("en");
    expect(t("nav.searchProjectsPlaceholder", "Search projects...")).toBe("Search projects...");
    expect(t("settings.searchSettingsPlaceholder", "Search settings")).toBe("Search settings");
    expect(t("settings.autoDetectedPathPlaceholder", "Auto-detected path")).toBe("Auto-detected path");
    expect(t("settings.accountNamePlaceholder", "Work or Personal")).toBe("Work or Personal");
    expect(t("settings.jiraApiTokenPlaceholder", "API token")).toBe("API token");
    expect(t("files.goToFilePlaceholder", "Go to File (type > for commands)")).toBe("Go to File (type > for commands)");
    expect(t("files.findPlaceholder", "Find")).toBe("Find");
    expect(t("inbox.searchChatsPlaceholder", "Search chats...")).toBe("Search chats...");
    expect(t("sessions.searchModelsOrHarnessesPlaceholder", "Search models or harnesses…")).toBe("Search models or harnesses…");
    expect(t("sessions.searchModelsPlaceholder", "Search models")).toBe("Search models");
    expect(t("sessions.chooseSessionFolderPlaceholder", "Choose or name a session folder…")).toBe("Choose or name a session folder…");
    expect(t("sessions.findInConversationPlaceholder", "Find in conversation")).toBe("Find in conversation");
    expect(t("sessions.askSideQuestionPlaceholder", "Ask a side question…")).toBe("Ask a side question…");
    expect(t("turn.typeYourAnswerPlaceholder", "Type your answer")).toBe("Type your answer");
    expect(t("sourceControl.searchBaseBranchesPlaceholder", "Search base branches…")).toBe("Search base branches…");
    expect(t("search.filesToIncludePlaceholder", "files to include")).toBe("files to include");
    expect(t("search.filesToExcludePlaceholder", "files to exclude")).toBe("files to exclude");
    expect(t("automations.searchTriggersPlaceholder", "Search triggers")).toBe("Search triggers");
  });
});
