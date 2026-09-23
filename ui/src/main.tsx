import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./tokens.css";
import "./index.css";
import "./workspace.css";
import "./components.css";

// Follow the system theme until the saved preference loads.
document.documentElement.dataset.theme = window.matchMedia?.(
  "(prefers-color-scheme: dark)",
).matches
  ? "dark"
  : "light";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
