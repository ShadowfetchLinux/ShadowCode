import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { UserMessage } from "./MessageActions";
import type { ChatItem } from "./cards";

afterEach(cleanup);

const item: Extract<ChatItem, { kind: "user" }> = {
  kind: "user",
  text: "Fix the add function",
  taskId: "t1",
  eventId: 7,
};

describe("UserMessage", () => {
  it("edits and resends, optionally undoing files", () => {
    const onEditResend = vi.fn();
    render(
      <UserMessage
        item={item}
        disabled={false}
        onEditResend={onEditResend}
        onRetry={vi.fn()}
        onCopy={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Edit and resend" }));
    const field = screen.getByLabelText("Edited message");
    fireEvent.change(field, { target: { value: "Fix add and sub" } });
    fireEvent.click(
      screen.getByLabelText(
        "Also undo the file changes made from this message on",
      ),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Send edited message" }),
    );
    expect(onEditResend).toHaveBeenCalledWith(item, "Fix add and sub", true);
    expect(screen.getByText("Fix the add function")).toBeTruthy();
  });

  it("cancels an edit with Escape and keeps the message", () => {
    const onEditResend = vi.fn();
    render(
      <UserMessage
        item={item}
        disabled={false}
        onEditResend={onEditResend}
        onRetry={vi.fn()}
        onCopy={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Edit and resend" }));
    fireEvent.keyDown(screen.getByLabelText("Edited message"), {
      key: "Escape",
    });
    expect(screen.queryByLabelText("Edited message")).toBeNull();
    expect(onEditResend).not.toHaveBeenCalled();
  });

  it("retries and copies; editing waits while a task runs", () => {
    const onRetry = vi.fn();
    const onCopy = vi.fn();
    const { rerender } = render(
      <UserMessage
        item={item}
        disabled={false}
        onEditResend={vi.fn()}
        onRetry={onRetry}
        onCopy={onCopy}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledWith("Fix the add function");
    fireEvent.click(screen.getByRole("button", { name: "Copy message" }));
    expect(onCopy).toHaveBeenCalledWith("Fix the add function");
    rerender(
      <UserMessage
        item={item}
        disabled
        onEditResend={vi.fn()}
        onRetry={onRetry}
        onCopy={onCopy}
      />,
    );
    expect(
      (
        screen.getByRole("button", {
          name: "Edit and resend",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
    expect(
      (
        screen.getByRole("button", {
          name: "Copy message",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false);
  });
});
