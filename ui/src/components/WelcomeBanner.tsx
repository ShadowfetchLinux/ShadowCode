import { Code2, GitPullRequest, Search, Wrench } from "lucide-react";

export type StarterCard = {
  title: string;
  body: string;
  prompt: string;
};

const STARTERS: StarterCard[] = [
  {
    title: "Build a feature",
    body: "Turn an idea into working code",
    prompt: "Help me build ",
  },
  {
    title: "Fix a bug",
    body: "Find the cause and apply a fix",
    prompt: "Find and fix the bug where ",
  },
  {
    title: "Understand code",
    body: "See a guided explanation",
    prompt:
      "Explore this workspace and explain its architecture, key entry points, and how to run it.",
  },
  {
    title: "Refactor",
    body: "Improve structure and flow",
    prompt: "Plan and apply a careful refactor for ",
  },
];

const ICONS = [Code2, Wrench, Search, GitPullRequest];

export function WelcomeBanner({
  onSelect,
}: {
  workspace?: string;
  model?: string;
  onSelect: (prompt: string, mode?: string) => void;
}) {
  return (
    <div className="welcome-banner">
      <div className="welcome-banner-header">
        <img src="/icon.svg" alt="" className="welcome-banner-icon" />
        <div>
          <h1 className="welcome-banner-title">What would you like to build?</h1>
          <p className="welcome-banner-sub">
            One app. Every model. Real results.
          </p>
        </div>
      </div>
      <div className="starter-grid starter-grid-quiet">
        {STARTERS.map((s, i) => {
          const Icon = ICONS[i];
          return (
            <button
              type="button"
              className="starter-chip"
              key={s.title}
              onClick={() => onSelect(s.prompt)}
            >
              <Icon size={15} />
              <span>
                <strong>{s.title}</strong>
                <small>{s.body}</small>
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
