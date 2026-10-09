import {
  HARNESS_TITLE,
  type HarnessId,
  type Session,
} from "../../sessions/model/session";
import { UsageLimitNotice } from "../../sessions/ui/UsageLimitNotice";
import { UsageLimitAccountPicker } from "../../sessions/ui/UsageLimitAccountPicker";
import { ModelPicker } from "../../sessions/ui/ModelPicker";

export function MonoUsageLimitNotice({
  session,
  onModelChange,
  onAccountChange,
  onResume,
  onResumeAtReset,
}: {
  session: Session;
  onModelChange: (harness: HarnessId, model: string) => void;
  onAccountChange?: (accountId: string) => void;
  onResume: () => void;
  onResumeAtReset: (enabled: boolean) => void;
}) {
  if (!session.usageLimit) return null;
  return (
    <UsageLimitNotice
      variant="mono"
      limit={session.usageLimit}
      providerName={HARNESS_TITLE[session.harness]}
      onResume={onResume}
      onResumeAtReset={onResumeAtReset}
      modelPicker={
        <>
          <ModelPicker
            harness={session.harness}
            model={session.model}
            values={session.modelSettings}
            project={session.cwd}
            hideSettings
            triggerLabel="Choose another model"
            onChange={onModelChange}
            onSettingsChange={() => {}}
          />
          {onAccountChange ? (
            <UsageLimitAccountPicker
              harness={session.harness}
              providerAccountId={session.providerAccountId}
              onSelect={onAccountChange}
            />
          ) : null}
        </>
      }
    />
  );
}
