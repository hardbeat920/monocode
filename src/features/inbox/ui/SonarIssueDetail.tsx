import { SonarIssueStatus } from "./SonarIssueStatus";
import { sonarLabel } from "../model/sonarLabels";
import { useSonarProjectSelection } from "../hooks/useSonarProjectSelection";
import { AgentMarkdown } from "../../sessions/ui/AgentMarkdown";
import {
  sonarExampleDiffs,
  sonarHtmlToMarkdown,
} from "../model/sonarRuleContent";
import { buildUnifiedFile } from "../../source-control/model/unifiedDiff";
import { UnifiedDiffView } from "../../source-control/ui/UnifiedDiffView";
import { languageForPath } from "../../files/editor/editorLanguage";
import {
  highlightSource,
  type SyntaxToken,
} from "../../files/editor/syntaxTokens";
import { useColorScheme } from "../../../shared/hooks/useColorScheme";
import { useEffect, useMemo, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { InboxItem } from "../model/githubTasks";
import {
  loadSonarIssueContent,
  peekSonarIssueContent,
  sonarIssueDetails,
  sonarRuleGuidance,
  type SonarIssueContent,
} from "../model/sonar";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { InboxProviderMark } from "./InboxProviderMark";

export function SonarIssueDetail({
  item,
  cwd,
  projects,
  onStart,
  onDiscuss,
  revision = 0,
  onItemChange,
}: {
  item: InboxItem;
  revision?: number;
  onItemChange?: (item: InboxItem) => void;
  cwd: string;
  projects: { path: string; name: string }[];
  onStart?: (item: InboxItem, body?: string) => void | Promise<void>;
  onDiscuss?: () => void;
}) {
  const [selectedProject, setSelectedProject] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const issue = item.sonarIssue;
  const [content, setContent] = useState(() => peekSonarIssueContent(item));
  const repositoryName = content?.repositoryName || item.projectName || "";
  const defaultProject = useSonarProjectSelection(
    projects,
    repositoryName,
    cwd,
    Boolean(onStart),
  );
  const project = selectedProject ?? defaultProject;
  const [loading, setLoading] = useState(!content);
  const [contentError, setContentError] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setContentError(null);
    void loadSonarIssueContent(item, { force: retry > 0 || revision > 0 })
      .then((next) => {
        if (!cancelled) setContent(next);
      })
      .catch((err) => {
        if (!cancelled)
          setContentError(String(err instanceof Error ? err.message : err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [item.id, item.repo, item.branch, item.updatedAt, retry, revision]);
  const guidance = useMemo(
    () => (content ? sonarRuleGuidance(content) : []),
    [content],
  );
  const exampleDiffs = useMemo(
    () =>
      guidance
        .flatMap((section) => sonarExampleDiffs(section.html))
        .map((example, index) => ({
          id: `example-${index}`,
          path: issue?.file || "example",
          label: `Rule example ${index + 1}`,
          ...buildUnifiedFile(example.original, example.current),
        })),
    [guidance, issue?.file],
  );
  const branchDiff = useMemo(
    () =>
      content?.comparison
        ? [
            {
              id: "analyzed-branch",
              path: issue?.file ?? "source",
              label: issue?.file ?? "Source file",
              ...buildUnifiedFile(
                content.comparison.original,
                content.comparison.current,
              ),
            },
          ]
        : [],
    [content?.comparison, issue?.file],
  );
  const start = async () => {
    if (!onStart || busy) return;
    setBusy(true);
    setError(null);
    try {
      const expanded =
        content ?? (await loadSonarIssueContent(item).catch(() => undefined));
      await onStart(
        { ...item, projectPath: project },
        sonarIssueDetails(item, expanded).body,
      );
    } catch (err) {
      setError(String(err instanceof Error ? err.message : err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="h-full overflow-y-auto overscroll-none px-6 py-5">
      <div className="flex items-center gap-2 text-[12px] text-content/55">
        <InboxProviderMark provider="sonar" className="size-4" />
        Sonar · {content?.repositoryName ||
          item.projectName ||
          item.repo} · {item.branch}
      </div>
      <h2 className="mt-3 text-lg font-semibold text-content">{item.title}</h2>
      <dl className="mt-4 grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-[13px]">
        {Object.entries({
          Branch: item.branch,
          Rule: issue?.rule,
          Severity: sonarLabel(issue?.severity),
          Type: sonarLabel(issue?.issueType),
          Status:
            content?.issue.issueStatus ||
            content?.issue.status ||
            issue?.status,
          File: issue
            ? `${issue.file}${issue.line == null ? "" : `:${issue.line}`}`
            : undefined,
          Assignee: issue?.assignee || "Unassigned",
        }).map(([label, value]) =>
          value ? (
            <div key={label} className="contents">
              <dt className="text-content/45">{label}</dt>
              <dd className="break-all text-content/80">
                {label === "Status" ? (
                  <SonarIssueStatus
                    item={item}
                    status={value}
                    transitions={content?.issue.transitions ?? []}
                    loading={loading}
                    onChanged={(next) => {
                      onItemChange?.(next);
                      setRetry((value) => value + 1);
                    }}
                  />
                ) : (
                  value
                )}
              </dd>
            </div>
          ) : null,
        )}
      </dl>
      <div className="mt-5 flex flex-wrap gap-2">
        <SecondaryButton onClick={() => void openUrl(item.url)}>
          Open in Sonar
        </SecondaryButton>
        {onDiscuss ? (
          <SecondaryButton onClick={onDiscuss}>Discuss</SecondaryButton>
        ) : null}
      </div>
      {onStart ? (
        <div className="mt-6 flex flex-col gap-2">
          {projects.length ? (
            <label className="flex flex-col gap-1 text-[12px] text-content/65">
              Work in project
              <select
                aria-label="Work in project"
                value={project}
                onChange={(event) => setSelectedProject(event.target.value)}
                className="h-8 rounded border border-content/10 bg-background-base px-2 text-content"
              >
                {projects.map((p) => (
                  <option key={p.path} value={p.path}>
                    {p.name}
                  </option>
                ))}
              </select>
            </label>
          ) : null}
          <SecondaryButton
            disabled={busy || !project}
            onClick={() => void start()}
          >
            {busy ? "Starting…" : "Start session"}
          </SecondaryButton>
        </div>
      ) : null}
      <div className="mt-6 border-t border-stroke pt-5">
        {loading ? (
          <p role="status" className="text-[13px] text-content/45">
            Loading Sonar issue details…
          </p>
        ) : null}
        {contentError ? (
          <div
            role="alert"
            className="flex items-center gap-3 text-[12px] text-rose-400"
          >
            <span>{contentError}</span>
            <SecondaryButton onClick={() => setRetry((value) => value + 1)}>
              Retry
            </SecondaryButton>
          </div>
        ) : null}
        {content ? (
          <>
            <section className="mb-6" aria-label="Issue details">
              <h3 className="mb-3 text-[14px] font-semibold">Issue details</h3>
              <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-[12px]">
                {Object.entries({
                  "Estimated effort":
                    content.issue.effort || content.issue.debt,
                  Created: content.issue.creationDate,
                  Updated: content.issue.updateDate,
                  Tags: content.issue.tags?.join(", "),
                  "Quality impacts": content.issue.impacts
                    ?.map(
                      (impact) =>
                        `${sonarLabel(impact.softwareQuality)}: ${sonarLabel(impact.severity)}`,
                    )
                    .join(", "),
                  "Coding attribute": sonarLabel(
                    content.rule?.cleanCodeAttribute,
                  ),
                }).map(([label, value]) =>
                  value ? (
                    <div key={label} className="contents">
                      <dt className="text-content/45">{label}</dt>
                      <dd className="break-words text-content/75">{value}</dd>
                    </div>
                  ) : null,
                )}
              </dl>
              <p className="mt-3 text-[13px] text-content/75">{item.title}</p>
            </section>
            {content.source.length ? (
              <section className="mb-6" aria-label="Analyzed source">
                <h3 className="mb-1 text-[14px] font-semibold">
                  Analyzed source
                </h3>
                <p className="mb-3 break-all text-[11px] text-content/45">
                  {issue?.file} · {item.branch}
                </p>
                <SonarSourceSnippet
                  content={content}
                  path={issue?.file ?? ""}
                  line={issue?.line}
                />
              </section>
            ) : null}
            {branchDiff.length ? (
              <section className="mb-6" aria-label="Analyzed branch diff">
                <h3 className="mb-1 text-[14px] font-semibold">
                  Analyzed branch diff
                </h3>
                <p className="mb-3 text-[11px] text-content/45">
                  {content.comparison?.baseBranch} → {item.branch}. Compares the
                  latest source analyzed by Sonar on each branch.
                </p>
                <UnifiedDiffView files={branchDiff} fill={false} />
              </section>
            ) : null}
            {content.issue.flows?.some((flow) => flow.locations.length) ? (
              <section className="mb-6" aria-label="Related locations">
                <h3 className="mb-3 text-[14px] font-semibold">
                  Related locations
                </h3>
                {content.issue.flows.map((flow, index) => (
                  <ol
                    key={index}
                    className="mb-3 list-decimal space-y-2 pl-5 text-[12px] text-content/70"
                  >
                    {flow.locations.map((location, i) => (
                      <li key={i}>
                        <p>{location.msg}</p>
                        <p className="break-all text-content/45">
                          {location.component?.replace(`${item.repo}:`, "")}
                          {location.textRange
                            ? `:${location.textRange.startLine}`
                            : ""}
                        </p>
                      </li>
                    ))}
                  </ol>
                ))}
              </section>
            ) : null}
            {guidance.map((section, index) => (
              <section key={index} className="mb-6" aria-label={section.title}>
                <h3 className="mb-3 text-[14px] font-semibold">
                  {section.title}
                </h3>
                <AgentMarkdown text={section.markdown} cwd={project || cwd} />
              </section>
            ))}
            {!guidance.length ? (
              <p className="mb-3 text-[12px] text-content/45">
                Sonar did not provide an explanation for this rule. Open its
                documentation for more information.
              </p>
            ) : null}
            {exampleDiffs.length ? (
              <section className="mb-6" aria-label="Rule example diff">
                <h3 className="mb-1 text-[14px] font-semibold">
                  Rule example diff
                </h3>
                <p className="mb-3 text-[11px] text-content/45">
                  Sonar's noncompliant and compliant examples. This illustrates
                  the fix and is not a patch for your file.
                </p>
                <UnifiedDiffView files={exampleDiffs} fill={false} />
              </section>
            ) : null}
            {content.rule?.params?.length ? (
              <section className="mb-6">
                <h3 className="mb-2 text-[14px] font-semibold">
                  Rule parameter defaults
                </h3>
                {content.rule.params.map((param) => (
                  <div key={param.key} className="mb-2 text-[12px]">
                    <p>
                      {param.key}: {param.defaultValue}
                    </p>
                    {param.htmlDesc ? (
                      <AgentMarkdown
                        text={sonarHtmlToMarkdown(param.htmlDesc)}
                        cwd={project || cwd}
                      />
                    ) : null}
                  </div>
                ))}
              </section>
            ) : null}
            {content.issue.comments?.length ? (
              <section className="mb-6">
                <h3 className="mb-3 text-[14px] font-semibold">Comments</h3>
                {content.issue.comments.map((comment) => (
                  <div key={comment.key} className="mb-4">
                    <p className="mb-1 text-[11px] text-content/45">
                      {comment.login} · {comment.createdAt}
                    </p>
                    <AgentMarkdown
                      text={
                        comment.markdown ||
                        sonarHtmlToMarkdown(comment.htmlText || "")
                      }
                      cwd={project || cwd}
                    />
                  </div>
                ))}
              </section>
            ) : null}
            <div className="flex flex-wrap items-center gap-3">
              <SecondaryButton onClick={() => void openUrl(content.ruleUrl)}>
                Rule documentation
              </SecondaryButton>
              {content.referenceDocumentation ? (
                <p className="text-[11px] text-content/45">
                  Latest official Sonar analyzer documentation. Your installed
                  analyzer may differ.
                </p>
              ) : null}
            </div>
            {content.errors.map((message, index) => (
              <p
                role="alert"
                key={index}
                className="mt-3 text-[12px] text-content/50"
              >
                {message}
              </p>
            ))}
          </>
        ) : null}
      </div>
      {error ? (
        <p role="alert" className="mt-3 text-[12px] text-rose-400">
          {error}
        </p>
      ) : null}
    </div>
  );
}

function SonarSourceSnippet({
  content,
  path,
  line,
}: {
  content: SonarIssueContent;
  path: string;
  line?: number | null;
}) {
  const scheme = useColorScheme();
  const [tokens, setTokens] = useState<SyntaxToken[][]>([]);
  useEffect(() => {
    let cancelled = false;
    void languageForPath(path)
      .then((language) => {
        if (!cancelled)
          setTokens(
            highlightSource(
              content.source.map((row) => row.code).join("\n"),
              language,
              scheme,
            ),
          );
      })
      .catch(() => {
        if (!cancelled) setTokens([]);
      });
    return () => {
      cancelled = true;
    };
  }, [content.source, path, scheme]);
  const start =
    content.issue.textRange?.startLine ?? content.issue.line ?? line;
  const end = content.issue.textRange?.endLine ?? start;
  return (
    <div className="overflow-x-auto rounded-md border border-stroke bg-content/3 py-2 font-mono text-[12px] leading-6">
      {content.source.map((row, index) => {
        const flagged =
          start != null && end != null && row.line >= start && row.line <= end;
        return (
          <div
            key={row.line}
            aria-label={flagged ? `Issue at line ${row.line}` : undefined}
            className={`flex min-w-max border-l-2 ${flagged ? "border-amber-400 bg-amber-400/10" : "border-transparent"}`}
          >
            <span className="w-14 shrink-0 select-none pr-3 text-right text-content/35">
              {row.line}
            </span>
            <pre className="pr-4 text-content/85">
              {tokens[index]?.map((token, i) => (
                <span
                  key={i}
                  style={token.color ? { color: token.color } : undefined}
                >
                  {token.text}
                </span>
              )) ?? row.code}
              {!row.code ? " " : null}
            </pre>
          </div>
        );
      })}
    </div>
  );
}
