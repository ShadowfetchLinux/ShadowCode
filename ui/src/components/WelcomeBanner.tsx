import {
  Code2,
  Wrench,
  GitPullRequest,
  Sparkles,
  TestTube2,
  ShieldCheck,
  BookOpen,
  Zap,
} from "lucide-react";

export type StarterCard = {
  icon: React.ComponentType<{ size?: number }>;
  title: string;
  body: string;
  prompt: string;
  mode: string;
};

const STARTERS: StarterCard[] = [
  {
    icon: Code2,
    title: "Build a feature",
    body: "Turn an idea into working, verified code",
    prompt: "Help me build ",
    mode: "coder",
  },
  {
    icon: Wrench,
    title: "Fix a bug",
    body: "Find the root cause and apply a clean fix",
    prompt:
      "Find and fix the bug where ",
    mode: "coder",
  },
  {
    icon: BookOpen,
    title: "Explain this codebase",
    body: "Get a guided tour of architecture & entry points",
    prompt:
      "Explore this workspace and explain its architecture, key entry points, and how to run it.",
    mode: "reviewer",
  },
  {
    icon: TestTube2,
    title: "Write tests",
    body: "Author unit tests and verify they pass",
    prompt:
      "Write automated tests for the core modules and verify they pass.",
    mode: "tester",
  },
  {
    icon: GitPullRequest,
    title: "Review my changes",
    body: "Spot bugs, edge cases, and style issues",
    prompt:
      "Review the current git diff for bugs, edge cases, and code quality issues. Explain findings before changing anything.",
    mode: "reviewer",
  },
  {
    icon: Sparkles,
    title: "Plan a refactor",
    body: "Get a blueprint before touching code",
    prompt:
      "Create a detailed refactoring plan for ",
    mode: "planner",
  },
  {
    icon: ShieldCheck,
    title: "Security audit",
    body: "Find vulnerabilities and exposed secrets",
    prompt:
      "Audit this workspace for security risks, unvalidated inputs, and exposed secrets.",
    mode: "reviewer",
  },
  {
    icon: Zap,
    title: "Optimize performance",
    body: "Profile hot paths and reduce overhead",
    prompt:
      "Analyze this codebase for performance bottlenecks and suggest concrete optimizations.",
    mode: "reviewer",
  },
];

export function WelcomeBanner({
  workspace,
  model,
  onSelect,
}: {
  workspace: string;
  model: string;
  onSelect: (prompt: string, mode: string) => void;
}) {
  const projectName = workspace.split("/").pop() || "your project";
  return (
    <div className="welcome-banner">
      <div className="welcome-banner-header">
        <img src="/icon.svg" alt="" className="welcome-banner-icon" />
        <div>
          <h1 className="welcome-banner-title">What are we building today?</h1>
          <p className="welcome-banner-sub">
            <strong>{projectName}</strong>
            {model ? <> · <span className="welcome-model-pill">{model}</span></> : null}
          </p>
        </div>
      </div>
      <div className="starter-grid">
        {STARTERS.map((s) => (
          <button
            type="button"
            className="starter-card"
            key={s.title}
            onClick={() => onSelect(s.prompt, s.mode)}
          >
            <span className="starter-icon">
              <s.icon size={16} />
            </span>
            <span className="starter-text">
              <strong className="starter-title">{s.title}</strong>
              <span className="starter-body">{s.body}</span>
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}
