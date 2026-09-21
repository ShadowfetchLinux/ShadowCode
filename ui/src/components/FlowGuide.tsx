import { useState } from "react";
import {
  BookOpen,
  Check,
  ChevronRight,
  Code2,
  Cpu,
  FileCode2,
  GitPullRequest,
  Keyboard,
  ListChecks,
  ListPlus,
  Play,
  RotateCcw,
  ShieldCheck,
  Sparkles,
  TestTube2,
  X,
  Zap,
} from "lucide-react";

interface FlowGuideProps {
  onClose: () => void;
  onSelectPrompt?: (prompt: string, mode?: string) => void;
}

export function FlowGuide({ onClose, onSelectPrompt }: FlowGuideProps) {
  const [activeTab, setActiveTab] = useState<
    "overview" | "modes" | "loop" | "steering" | "shortcuts"
  >("overview");

  const startTask = (prompt: string, mode = "coder") => {
    if (onSelectPrompt) {
      onSelectPrompt(prompt, mode);
    }
    onClose();
  };

  return (
    <div className="modal-back flow-guide-backdrop" role="dialog" aria-modal="true" aria-labelledby="flow-guide-title">
      <div className="flow-guide-modal">
        <header className="flow-guide-header">
          <div className="flow-guide-title-wrap">
            <div className="flow-guide-badge">
              <Sparkles size={14} />
              <span>20-MINUTE MASTERY</span>
            </div>
            <h2 id="flow-guide-title">ShadowCode Fast-Track Flow</h2>
            <p className="flow-guide-sub">
              Master the autonomous coding loop powered by open-source, open-weight models.
            </p>
          </div>
          <button
            type="button"
            className="icon-btn close-btn"
            aria-label="Close guide"
            onClick={onClose}
          >
            <X size={18} />
          </button>
        </header>

        <nav className="flow-guide-nav" aria-label="Guide sections">
          {[
            { id: "overview", label: "1. Open Weights Pillar", icon: Cpu },
            { id: "modes", label: "2. The 4 Modes", icon: Code2 },
            { id: "loop", label: "3. Codex / Cursor Loop", icon: GitPullRequest },
            { id: "steering", label: "4. Live Steering", icon: ListPlus },
            { id: "shortcuts", label: "5. Hotkeys & Commands", icon: Keyboard },
          ].map((tab) => (
            <button
              key={tab.id}
              type="button"
              className={`flow-nav-btn ${activeTab === tab.id ? "active" : ""}`}
              onClick={() => setActiveTab(tab.id as typeof activeTab)}
            >
              <tab.icon size={15} />
              <span>{tab.label}</span>
            </button>
          ))}
        </nav>

        <div className="flow-guide-content">
          {activeTab === "overview" && (
            <div className="flow-tab-pane">
              <div className="flow-hero-card">
                <div className="hero-icon-wrap">
                  <Cpu size={28} />
                </div>
                <div>
                  <h3>A True Pillar for Open Source & Open Weights</h3>
                  <p>
                    ShadowCode is engineered to treat open-weight models (Qwen 2.5 Coder, DeepSeek R1/V3, Llama 3.3, Devstral) as first-class citizens. Run on your GPU with 100% private, zero-subscription computing.
                  </p>
                </div>
              </div>

              <div className="flow-grid-2">
                <div className="flow-card">
                  <h4>⚡ Open-Weight Coding Champions</h4>
                  <ul className="flow-checklist">
                    <li>
                      <strong>Qwen 2.5 Coder (7B / 14B / 32B):</strong> SOTA benchmark leader for code generation, bug fixing, and tool use.
                    </li>
                    <li>
                      <strong>DeepSeek R1 & V3 (14B / 32B / 671B):</strong> Unmatched chain-of-thought planning and reasoning.
                    </li>
                    <li>
                      <strong>Llama 3.3 (70B) & 3.1 (8B):</strong> Meta&apos;s reliable, highly aligned open-weight foundation.
                    </li>
                    <li>
                      <strong>Mistral Devstral / Codestral (22B):</strong> Blazing-fast refactoring and test writing.
                    </li>
                  </ul>
                </div>

                <div className="flow-card">
                  <h4>🖥️ Hardware & VRAM Compatibility</h4>
                  <p className="flow-card-desc">
                    ShadowCode automatically inspects your GPU and memory to ensure high-performance token generation:
                  </p>
                  <div className="vram-guide-table">
                    <div className="vram-row">
                      <span className="vram-model">7B / 8B Models</span>
                      <span className="vram-badge optimal">~5–7 GB VRAM (RTX 3060/4060+)</span>
                    </div>
                    <div className="vram-row">
                      <span className="vram-model">12B / 14B Models</span>
                      <span className="vram-badge optimal">~9–12 GB VRAM (RTX 4070/5060Ti)</span>
                    </div>
                    <div className="vram-row">
                      <span className="vram-model">20B / 32B Models</span>
                      <span className="vram-badge quant">~18–24 GB VRAM (Q4/Q5 Quant)</span>
                    </div>
                    <div className="vram-row">
                      <span className="vram-model">70B+ Models</span>
                      <span className="vram-badge offload">~40 GB+ or vLLM cluster</span>
                    </div>
                  </div>
                </div>
              </div>

              <div className="flow-callout">
                <ShieldCheck size={18} />
                <span>
                  <strong>Zero data leaves your machine:</strong> When using local providers (Ollama on <code>:11434</code> or LM Studio on <code>:1234</code>), every file, prompt, and tool execution stays strictly inside your Linux environment.
                </span>
              </div>
            </div>
          )}

          {activeTab === "modes" && (
            <div className="flow-tab-pane">
              <h3>Choose the Right Agent Mode for Your Goal</h3>
              <p className="flow-pane-sub">
                Switch modes instantly in the composer dropdown to align the model&apos;s autonomy and tool permissions.
              </p>

              <div className="flow-grid-2">
                <div className="mode-card coder">
                  <div className="mode-header">
                    <Code2 size={20} />
                    <h4>Build (Coder)</h4>
                    <span className="mode-tag">Default</span>
                  </div>
                  <p>
                    Full autonomous execution. The agent searches files, creates modules, modifies code, runs commands, and inspects test output.
                  </p>
                  <button
                    type="button"
                    className="starter-btn"
                    onClick={() => startTask("Help me build a new feature: ", "coder")}
                  >
                    <Play size={13} /> Try Build Mode
                  </button>
                </div>

                <div className="mode-card planner">
                  <div className="mode-header">
                    <ListChecks size={20} />
                    <h4>Plan (Architect)</h4>
                    <span className="mode-tag">Non-destructive</span>
                  </div>
                  <p>
                    High-level roadmap and architectural breakdown. Plans milestones and step-by-step strategies without touching your code.
                  </p>
                  <button
                    type="button"
                    className="starter-btn"
                    onClick={() => startTask("Create an implementation plan for: ", "planner")}
                  >
                    <Play size={13} /> Try Plan Mode
                  </button>
                </div>

                <div className="mode-card reviewer">
                  <div className="mode-header">
                    <GitPullRequest size={20} />
                    <h4>Review (Auditor)</h4>
                    <span className="mode-tag">Security & Bugs</span>
                  </div>
                  <p>
                    Deep code auditing. Scans git diffs or whole projects for vulnerabilities, exposed keys, memory leaks, and performance bottlenecks.
                  </p>
                  <button
                    type="button"
                    className="starter-btn"
                    onClick={() => startTask("Review this project for bugs and security issues. Explain findings before changing files.", "reviewer")}
                  >
                    <Play size={13} /> Try Review Mode
                  </button>
                </div>

                <div className="mode-card tester">
                  <div className="mode-header">
                    <TestTube2 size={20} />
                    <h4>Test (QA)</h4>
                    <span className="mode-tag">Verification</span>
                  </div>
                  <p>
                    Automated quality assurance. Writes unit tests, tests edge cases, executes your test harness, and validates regression safety.
                  </p>
                  <button
                    type="button"
                    className="starter-btn"
                    onClick={() => startTask("Write unit tests for the core modules and verify they pass.", "tester")}
                  >
                    <Play size={13} /> Try Test Mode
                  </button>
                </div>
              </div>
            </div>
          )}

          {activeTab === "loop" && (
            <div className="flow-tab-pane">
              <h3>The 4-Step Codex & Cursor Iterative Loop</h3>
              <p className="flow-pane-sub">
                Designed to give you full control with minimal friction. You never have to copy-paste diffs manually.
              </p>

              <div className="flow-steps-timeline">
                <div className="timeline-item">
                  <div className="timeline-num">1</div>
                  <div className="timeline-content">
                    <h4>Prompt with Clear Intent & Context</h4>
                    <p>
                      Describe what to build or fix. Drag-and-drop relevant files into the composer or use chips to target specific modules.
                    </p>
                  </div>
                </div>

                <div className="timeline-item">
                  <div className="timeline-num">2</div>
                  <div className="timeline-content">
                    <h4>Inspect Live Reasoning & Tool Cards</h4>
                    <p>
                      ShadowCode executes tools transparently: reading files, running linters, or checking git status. Reasoning models (like DeepSeek R1) stream their thought process before any edits occur.
                    </p>
                  </div>
                </div>

                <div className="timeline-item">
                  <div className="timeline-num">3</div>
                  <div className="timeline-content">
                    <h4>Review Diffs with Inline Hunk Controls</h4>
                    <p>
                      Open the Changes drawer (<code>Ctrl+B</code>) or click on any operation card to inspect color-coded additions and deletions. Accept or reject changes per-hunk or per-file.
                    </p>
                  </div>
                </div>

                <div className="timeline-item">
                  <div className="timeline-num">4</div>
                  <div className="timeline-content">
                    <h4>Iterate or Rewind Safely</h4>
                    <p>
                      Send follow-up instructions to refine your code. If something isn&apos;t right, click <strong>Rewind</strong> on any card to undo that specific step cleanly.
                    </p>
                  </div>
                </div>
              </div>
            </div>
          )}

          {activeTab === "steering" && (
            <div className="flow-tab-pane">
              <h3>Live Steering & Follow-up Queueing</h3>
              <p className="flow-pane-sub">
                Unlike rigid assistants that force you to wait or restart, ShadowCode lets you guide work while it runs.
              </p>

              <div className="flow-grid-2">
                <div className="flow-card">
                  <h4>🎯 Live Steering (Pause & Steer)</h4>
                  <p>
                    Notice the model heading down the wrong path? Don&apos;t cancel!
                  </p>
                  <ol className="styled-list">
                    <li>Click <strong>Pause</strong> on the live steer bar.</li>
                    <li>Type steering directions (e.g. <em>&quot;Use serde instead of manual JSON&quot;</em>).</li>
                    <li>Click <strong>Resume</strong> to inject guidance into the agent&apos;s working context immediately.</li>
                  </ol>
                </div>

                <div className="flow-card">
                  <h4>📋 Queued Tasks</h4>
                  <p>
                    Keep your train of thought going while the agent is busy:
                  </p>
                  <ul className="flow-checklist">
                    <li>
                      Type follow-up prompts into the composer while a task is running.
                    </li>
                    <li>
                      Press <code>Enter</code> to append to the task queue.
                    </li>
                    <li>
                      Follow-ups execute seamlessly as soon as the active job verifies and finishes.
                    </li>
                  </ul>
                </div>
              </div>

              <div className="flow-card full-width">
                <h4>🛡️ Durable Safety Checkpoints</h4>
                <p>
                  ShadowCode records dirty-state snapshots before file edits. If an automated change fails verification or a command is interrupted, your git working tree is safeguarded and can be reverted with one click.
                </p>
              </div>
            </div>
          )}

          {activeTab === "shortcuts" && (
            <div className="flow-tab-pane">
              <h3>Keyboard Shortcuts & Slash Commands</h3>
              <p className="flow-pane-sub">
                Designed for lightning-fast keyboard-driven development.
              </p>

              <div className="flow-grid-2">
                <div className="flow-card">
                  <h4>⌨️ Essential Hotkeys</h4>
                  <table className="shortcuts-table">
                    <tbody>
                      <tr>
                        <td><kbd>Enter</kbd></td>
                        <td>Send prompt or queue follow-up</td>
                      </tr>
                      <tr>
                        <td><kbd>Shift</kbd> + <kbd>Enter</kbd></td>
                        <td>Insert newline in composer</td>
                      </tr>
                      <tr>
                        <td><kbd>Ctrl</kbd> + <kbd>K</kbd></td>
                        <td>Open Command Palette</td>
                      </tr>
                      <tr>
                        <td><kbd>Ctrl</kbd> + <kbd>B</kbd></td>
                        <td>Toggle Right Drawer (Files / Diff / Goals)</td>
                      </tr>
                      <tr>
                        <td><kbd>Ctrl</kbd> + <kbd>.</kbd></td>
                        <td>Emergency stop active task</td>
                      </tr>
                      <tr>
                        <td><kbd>?</kbd></td>
                        <td>Quick Keyboard Reference</td>
                      </tr>
                    </tbody>
                  </table>
                </div>

                <div className="flow-card">
                  <h4>⚡ Fast Slash Commands</h4>
                  <table className="shortcuts-table">
                    <tbody>
                      <tr>
                        <td><code>/new</code></td>
                        <td>Start a clean new session</td>
                      </tr>
                      <tr>
                        <td><code>/model &lt;id&gt;</code></td>
                        <td>Switch active model immediately</td>
                      </tr>
                      <tr>
                        <td><code>/plan &lt;goal&gt;</code></td>
                        <td>Create a structured milestone plan</td>
                      </tr>
                      <tr>
                        <td><code>/review</code></td>
                        <td>Audit workspace for bugs & security</td>
                      </tr>
                      <tr>
                        <td><code>/skill &lt;name&gt;</code></td>
                        <td>Execute a configured agent skill</td>
                      </tr>
                      <tr>
                        <td><code>/health</code></td>
                        <td>Run system doctor & inspect routes</td>
                      </tr>
                    </tbody>
                  </table>
                </div>
              </div>
            </div>
          )}
        </div>

        <footer className="flow-guide-footer">
          <div className="guide-starter-chips">
            <span className="chips-label">Quick Starters:</span>
            <button
              type="button"
              className="chip-btn"
              onClick={() => startTask("Explore this workspace and explain its architecture and entrypoints.", "reviewer")}
            >
              🔍 Codebase Tour
            </button>
            <button
              type="button"
              className="chip-btn"
              onClick={() => startTask("Review this project for bugs and security issues.", "reviewer")}
            >
              🛡️ Security Audit
            </button>
            <button
              type="button"
              className="chip-btn"
              onClick={() => startTask("Help me build a new feature: ", "coder")}
            >
              🚀 New Feature
            </button>
          </div>
          <div className="guide-footer-actions">
            <button type="button" className="primary" onClick={onClose}>
              Got It, Let&apos;s Build <ChevronRight size={14} />
            </button>
          </div>
        </footer>
      </div>
    </div>
  );
}
