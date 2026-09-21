import { afterEach, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { Markdown } from "./Markdown";

const parses = vi.hoisted(() => vi.fn());
vi.mock("react-markdown", async (original) => {
  const actual = await original<typeof import("react-markdown")>();
  return {
    ...actual,
    default: (props: Parameters<typeof actual.default>[0]) => {
      parses(props.children);
      return actual.default(props);
    },
  };
});
afterEach(() => {
  cleanup();
  parses.mockClear();
  vi.unstubAllGlobals();
});

it("parses only the changing response while earlier Markdown stays interactive", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  function Conversation({ response }: { response: string }) {
    return (
      <>
        <Markdown>
          {
            "## Earlier answer\n\n[Reference](https://example.com)\n\n```js\nconst answer = 42;\n```"
          }
        </Markdown>
        <Markdown>{response}</Markdown>
      </>
    );
  }
  const view = render(<Conversation response="Starting" />);
  expect(parses).toHaveBeenCalledTimes(2);
  for (let index = 0; index < 100; index++) {
    view.rerender(<Conversation response={`Streaming **${index}**`} />);
  }
  expect(parses).toHaveBeenCalledTimes(102);
  expect(screen.getByRole("heading", { name: "Earlier answer" })).toBeTruthy();
  expect(
    screen.getByRole("link", { name: "Reference" }).getAttribute("href"),
  ).toBe("https://example.com");
  expect(screen.getByRole("button", { name: "Copy code" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Copy code" }));
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Code copied" })).toBeTruthy(),
  );
  expect(writeText).toHaveBeenCalledWith("const answer = 42;\n");
  expect(screen.getByText("99").tagName).toBe("STRONG");
  view.rerender(<Conversation response="Streaming **99**" />);
  expect(parses).toHaveBeenCalledTimes(102);
});
