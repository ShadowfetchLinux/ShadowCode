import { useState } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { Dialog } from "./Dialog";

afterEach(cleanup);

function Harness() {
  const [open, setOpen] = useState(false);
  return (
    <div>
      <button type="button" onClick={() => setOpen(true)}>
        Open settings
      </button>
      {open ? (
        <Dialog label="Settings" onClose={() => setOpen(false)}>
          <button type="button" onClick={() => setOpen(false)}>
            Close
          </button>
        </Dialog>
      ) : null}
    </div>
  );
}

it("restores keyboard focus to the opener when the dialog unmounts", () => {
  render(<Harness />);
  const opener = screen.getByRole("button", { name: "Open settings" });
  opener.focus();
  fireEvent.click(opener);
  expect(screen.getByRole("dialog", { name: "Settings" })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(screen.queryByRole("dialog", { name: "Settings" })).toBeNull();
  expect(document.activeElement).toBe(opener);
});
