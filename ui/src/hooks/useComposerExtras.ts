import { useCallback, useEffect, useRef, useState } from "react";
import {
  readEffort,
  supportsEffort,
  writeEffort,
  type Effort,
  type TaskMode,
} from "../lib/effort";
import type { Mention } from "../lib/mentions";
import type { PickerTarget } from "../lib/picker";
import { pushHistory, readHistory, stepHistory } from "../lib/promptHistory";

/** ↑/↓ prompt recall for one project. `older`/`newer` return the text to
 * show, or null when there is nothing further. */
export function usePromptHistory(workspace: string) {
  const index = useRef(-1);
  const draft = useRef("");
  useEffect(() => {
    index.current = -1;
  }, [workspace]);
  const older = useCallback(
    (current: string) => {
      const list = readHistory(workspace);
      if (index.current < 0) draft.current = current;
      const step = stepHistory(list, index.current, "older", draft.current);
      if (!step) return null;
      index.current = step.index;
      return step.text;
    },
    [workspace],
  );
  const newer = useCallback(() => {
    const list = readHistory(workspace);
    const step = stepHistory(list, index.current, "newer", draft.current);
    if (!step) return null;
    index.current = step.index;
    return step.text;
  }, [workspace]);
  const push = useCallback(
    (text: string) => {
      pushHistory(workspace, text);
      index.current = -1;
      draft.current = "";
    },
    [workspace],
  );
  /** Typing leaves history browsing. */
  const reset = useCallback(() => {
    index.current = -1;
  }, []);
  const browsing = useCallback(() => index.current >= 0, []);
  return { older, newer, push, reset, browsing };
}
export type PromptHistory = ReturnType<typeof usePromptHistory>;

/** Composer choices besides the text and attachments: @-mentioned files
 * and folders, prompt history, reasoning effort (per model) and the task
 * mode (Code / Plan / Ask). */
export function useComposerExtras({
  workspace,
  sessionId,
  target,
}: {
  workspace: string;
  sessionId: string;
  target: PickerTarget | undefined;
}) {
  const [mentions, setMentions] = useState<Mention[]>([]);
  const [mode, setMode] = useState<TaskMode>("code");
  const targetId = target?.id || "";
  const [effortState, setEffortState] = useState<{
    target: string;
    effort: Effort;
  }>(() => ({ target: targetId, effort: readEffort(targetId) }));
  if (effortState.target !== targetId)
    setEffortState({ target: targetId, effort: readEffort(targetId) });
  const history = usePromptHistory(workspace);
  // Mentions belong to the draft of one conversation.
  useEffect(() => {
    setMentions([]);
  }, [sessionId, workspace]);
  const addMention = useCallback(
    (m: Mention) =>
      setMentions((list) =>
        list.some((x) => x.path === m.path) ? list : [...list, m],
      ),
    [],
  );
  const removeMention = useCallback(
    (path: string) =>
      setMentions((list) => list.filter((m) => m.path !== path)),
    [],
  );
  const setEffort = useCallback(
    (effort: Effort) => {
      if (!targetId) return;
      writeEffort(targetId, effort);
      setEffortState({ target: targetId, effort });
    },
    [targetId],
  );
  const effortShown = supportsEffort(target);
  return {
    mentions,
    setMentions,
    addMention,
    removeMention,
    history,
    mode,
    setMode,
    effort: effortShown ? effortState.effort : ("default" as Effort),
    effortShown,
    setEffort,
  };
}
export type ComposerExtras = ReturnType<typeof useComposerExtras>;
