import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import {
  Check,
  ChevronDown,
  ChevronRight,
  Eye,
  Info,
  MessageSquareText,
  Plus,
  Search,
} from "lucide-react";
import {
  availabilityLabel,
  groupTargets,
  isLocal,
  isReady,
  matchesQuery,
  recentTargets,
  rowAction,
  shortName,
  usageDetailLines,
  usageLabel,
  vendorLabel,
  vendorSections,
  type PickerTarget,
} from "../lib/picker";

type Item =
  | { kind: "row"; key: string; target: PickerTarget }
  | {
      kind: "more";
      key: string;
      vendor: string;
      label: string;
      hidden: number;
    };

type Group = { id: string; title: string; items: Item[]; empty: string };

/** The one model control: subscription rows and local GGUF rows from
 * GET /api/picker in a searchable listbox (combobox pattern). */
export function UnifiedPicker({
  targets,
  value,
  open,
  onOpenChange,
  onSelect,
  onConnect,
  onSetup,
  onAddLocal,
  note,
  loading,
}: {
  targets: PickerTarget[];
  value: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSelect: (id: string) => void;
  /** Accounts › Connect, optionally for one vendor ("codex"). */
  onConnect: (vendor?: string) => void;
  /** Where a setup-required row is fixed (Accounts or Local models). */
  onSetup: (target: PickerTarget) => void;
  onAddLocal: () => void;
  note?: string;
  loading?: boolean;
}) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [expanded, setExpanded] = useState<string[]>([]);
  const [details, setDetails] = useState<string | null>(null);
  const [recent] = useState(recentTargets);
  const trigger = useRef<HTMLButtonElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const uid = useId().replace(/:/g, "");
  const listId = `${uid}-list`;
  const selected = targets.find((t) => t.id === value);

  const groups = useMemo<Group[]>(() => {
    const matching = targets.filter((t) => matchesQuery(t, query));
    const all = groupTargets(matching);
    const build = (rows: PickerTarget[]): Item[] =>
      vendorSections(rows, {
        expanded,
        recent,
        selected: value,
        searching: Boolean(query.trim()),
      }).flatMap((section) => [
        ...section.rows.map((target): Item => ({
          kind: "row",
          key: target.id,
          target,
        })),
        ...(section.hidden
          ? [
              {
                kind: "more" as const,
                key: `more:${section.key}`,
                vendor: section.key,
                label: section.label,
                hidden: section.hidden,
              },
            ]
          : []),
      ]);
    return [
      {
        id: "subscriptions",
        title: "Subscriptions",
        items: build(all.subscriptions),
        empty: query ? "No subscription matches" : "No accounts connected yet",
      },
      {
        id: "local",
        title: "On this computer",
        items: build(all.local),
        empty: query ? "No local model matches" : "No local models added yet",
      },
    ];
  }, [targets, query, expanded, recent, value]);
  const items = useMemo(() => groups.flatMap((g) => g.items), [groups]);
  const optionId = (item: Item) =>
    `${uid}-opt-${item.key.replace(/[^a-zA-Z0-9_-]/g, "_")}`;
  const activeItem = items[Math.min(active, items.length - 1)];
  const detailTarget = details
    ? targets.find((t) => t.id === details)
    : undefined;

  useEffect(() => {
    if (!open) return;
    const index = items.findIndex(
      (item) => item.kind === "row" && item.target.id === value,
    );
    setActive(index >= 0 ? index : 0);
    setDetails(null);
    // Focus the search field once the menu is in the document.
    requestAnimationFrame(() => search.current?.focus());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (event: MouseEvent) => {
      if (!root.current?.contains(event.target as Node)) close(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  });

  useEffect(() => {
    if (!open || !activeItem) return;
    document
      .getElementById(optionId(activeItem))
      ?.scrollIntoView?.({ block: "nearest" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, open]);

  function close(restoreFocus = true) {
    onOpenChange(false);
    setQuery("");
    setDetails(null);
    if (restoreFocus) requestAnimationFrame(() => trigger.current?.focus());
  }

  function activate(item: Item | undefined) {
    if (!item) return;
    if (item.kind === "more") {
      setExpanded((keys) => [...keys, item.vendor]);
      return;
    }
    const target = item.target;
    const action = rowAction(target);
    if (action.kind === "select") {
      onSelect(target.id);
      close();
    } else if (action.kind === "connect") {
      close(false);
      onConnect(action.vendor);
    } else if (details === target.id && action.kind === "setup") {
      close(false);
      onSetup(target);
    } else {
      setDetails(target.id);
    }
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    const last = items.length - 1;
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        setActive((i) => Math.min(last, i + 1));
        break;
      case "ArrowUp":
        event.preventDefault();
        setActive((i) => Math.max(0, i - 1));
        break;
      case "Home":
        event.preventDefault();
        setActive(0);
        break;
      case "End":
        event.preventDefault();
        setActive(Math.max(0, last));
        break;
      case "Enter":
        event.preventDefault();
        activate(activeItem);
        break;
      case "ArrowRight":
        if (activeItem?.kind === "row" && !query) {
          event.preventDefault();
          setDetails(activeItem.target.id);
        }
        break;
      case "ArrowLeft":
        if (details && !query) {
          event.preventDefault();
          setDetails(null);
        }
        break;
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        if (details) setDetails(null);
        else close();
        break;
    }
  }

  // Keep the detail pane on the row the keyboard is on once it is open.
  useEffect(() => {
    if (
      details &&
      activeItem?.kind === "row" &&
      activeItem.target.id !== details
    )
      setDetails(activeItem.target.id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  return (
    <div className="unified-picker" ref={root}>
      <button
        ref={trigger}
        type="button"
        className={`unified-picker-trigger ${selected ? "" : "is-empty"}`}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label={
          selected
            ? `Model for this task: ${selected.name}`
            : "Model for this task: none chosen"
        }
        onClick={() => (open ? close() : onOpenChange(true))}
      >
        {selected && (
          <span
            className={`inference-badge ${isLocal(selected) ? "local" : "cloud"}`}
          >
            {isLocal(selected) ? "Local" : "Cloud"}
          </span>
        )}
        <span className="unified-picker-current">
          {selected
            ? shortName(selected)
            : loading
              ? "Loading models…"
              : "Choose a model"}
        </span>
        <ChevronDown size={14} aria-hidden="true" />
      </button>
      {note && (
        <span className="picker-note" role="status">
          {note}
        </span>
      )}
      {open && (
        <div
          className="unified-picker-menu"
          role="dialog"
          aria-label="Choose a model"
        >
          <label className="unified-picker-search">
            <Search size={14} aria-hidden="true" />
            <input
              ref={search}
              value={query}
              role="combobox"
              aria-label="Search models"
              aria-expanded="true"
              aria-controls={listId}
              aria-autocomplete="list"
              aria-activedescendant={
                activeItem ? optionId(activeItem) : undefined
              }
              placeholder="Search models…"
              onChange={(e) => {
                setQuery(e.target.value);
                setActive(0);
                setDetails(null);
              }}
              onKeyDown={onKeyDown}
            />
          </label>
          <div
            className="unified-picker-list"
            role="listbox"
            id={listId}
            aria-label="Models"
          >
            {groups.map((group) => (
              <div
                key={group.id}
                className="unified-picker-group"
                role="group"
                aria-labelledby={`${uid}-${group.id}`}
              >
                <div
                  className="unified-picker-heading"
                  id={`${uid}-${group.id}`}
                  role="presentation"
                >
                  {group.title}
                </div>
                {group.items.length === 0 && (
                  <div className="unified-picker-empty" role="presentation">
                    {group.empty}
                  </div>
                )}
                {group.items.map((item) => {
                  const index = items.indexOf(item);
                  const isActive = index === active;
                  if (item.kind === "more")
                    return (
                      <div
                        key={item.key}
                        id={optionId(item)}
                        role="option"
                        aria-selected={false}
                        className={`unified-picker-more ${isActive ? "is-active" : ""}`}
                        onMouseDown={(e) => e.preventDefault()}
                        onMouseMove={() => setActive(index)}
                        onClick={() => activate(item)}
                      >
                        <ChevronRight size={13} aria-hidden="true" />
                        Show all {item.hidden +
                          countShown(items, item.vendor)}{" "}
                        {item.label} models
                      </div>
                    );
                  return (
                    <Row
                      key={item.key}
                      id={optionId(item)}
                      target={item.target}
                      active={isActive}
                      selected={item.target.id === value}
                      detailsOpen={details === item.target.id}
                      detailsId={`${uid}-details`}
                      onHover={() => setActive(index)}
                      onPick={() => {
                        setActive(index);
                        activate(item);
                      }}
                      onDetails={() => {
                        setActive(index);
                        setDetails((d) =>
                          d === item.target.id ? null : item.target.id,
                        );
                        search.current?.focus();
                      }}
                    />
                  );
                })}
              </div>
            ))}
          </div>
          {detailTarget && (
            <Details
              id={`${uid}-details`}
              target={detailTarget}
              onConnect={(vendor) => {
                close(false);
                onConnect(vendor);
              }}
              onSetup={(target) => {
                close(false);
                onSetup(target);
              }}
            />
          )}
          <div className="unified-picker-actions">
            <button
              type="button"
              onClick={() => {
                close(false);
                onConnect();
              }}
            >
              <Plus size={14} aria-hidden="true" /> Connect account…
            </button>
            <button
              type="button"
              onClick={() => {
                close(false);
                onAddLocal();
              }}
            >
              <Plus size={14} aria-hidden="true" /> Add local model…
            </button>
          </div>
          <p className="unified-picker-keys" aria-hidden="true">
            ↑↓ move · Enter choose · → details · Esc close
          </p>
        </div>
      )}
    </div>
  );
}

function countShown(items: Item[], vendor: string) {
  return items.filter(
    (item) =>
      item.kind === "row" &&
      (isLocal(item.target)
        ? vendor === "local"
        : item.target.provider.replace(/^cli:/, "") === vendor),
  ).length;
}

function Row({
  id,
  target,
  active,
  selected,
  detailsOpen,
  detailsId,
  onHover,
  onPick,
  onDetails,
}: {
  id: string;
  target: PickerTarget;
  active: boolean;
  selected: boolean;
  detailsOpen: boolean;
  detailsId: string;
  onHover: () => void;
  onPick: () => void;
  onDetails: () => void;
}) {
  const ready = isReady(target);
  return (
    <div
      id={id}
      role="option"
      aria-selected={selected}
      aria-describedby={detailsOpen ? `${id}-meta ${detailsId}` : `${id}-meta`}
      className={`unified-picker-row ${ready ? "" : "is-blocked"} ${selected ? "is-selected" : ""} ${active ? "is-active" : ""}`}
      onMouseDown={(e) => e.preventDefault()}
      onMouseMove={onHover}
      onClick={onPick}
    >
      <span className="unified-picker-copy">
        <span className="unified-picker-name">
          <strong>{target.name}</strong>
          {target.vision === true && (
            <span className="cap-badge" title="Accepts images">
              <Eye size={11} aria-hidden="true" /> Vision
            </span>
          )}
          {target.tools === false && (
            <span className="cap-badge" title="Answers only; cannot use tools">
              <MessageSquareText size={11} aria-hidden="true" /> Chat only
            </span>
          )}
        </span>
        <small id={`${id}-meta`}>
          {isLocal(target) ? "Local" : "Cloud"} ·{" "}
          <span className={`avail avail-${target.availability}`}>
            {availabilityLabel(target)}
          </span>{" "}
          · {usageLabel(target.usage, target.inference)}
        </small>
      </span>
      <span className="unified-picker-meta">
        {selected && <Check size={14} aria-hidden="true" />}
        {/* Keyboard users open the same details with the right arrow key;
            options cannot contain nested interactive controls. */}
        <span
          className={`picker-info ${detailsOpen ? "on" : ""}`}
          aria-hidden="true"
          title="Details"
          onClick={(e) => {
            e.stopPropagation();
            onDetails();
          }}
        >
          <Info size={13} />
        </span>
      </span>
    </div>
  );
}

function Details({
  id,
  target,
  onConnect,
  onSetup,
}: {
  id: string;
  target: PickerTarget;
  onConnect: (vendor: string) => void;
  onSetup: (target: PickerTarget) => void;
}) {
  const action = rowAction(target);
  const lines = usageDetailLines(target.usage);
  return (
    <div className="unified-picker-details" id={id} aria-live="polite">
      <strong>{target.name}</strong>
      <p>
        {isLocal(target) ? "Runs on this computer" : "Cloud"} ·{" "}
        {availabilityLabel(target)}
        {target.reason ? ` · ${target.reason}` : ""}
      </p>
      <p>{usageLabel(target.usage, target.inference)}</p>
      {lines.length > 0 && (
        <ul>
          {lines.map((line) => (
            <li key={line}>{line}</li>
          ))}
        </ul>
      )}
      {action.kind === "setup" && (
        <p className="unified-picker-hint">{action.hint}</p>
      )}
      <div className="row">
        {target.usage?.provider_usage_url && (
          <a
            href={target.usage.provider_usage_url}
            target="_blank"
            rel="noreferrer"
          >
            Open {vendorLabel(target)} usage
          </a>
        )}
        {action.kind === "connect" && (
          <button
            type="button"
            className="mini"
            onClick={() => onConnect(action.vendor)}
          >
            Sign in to {vendorLabel(target)}
          </button>
        )}
        {action.kind === "setup" && (
          <button
            type="button"
            className="mini"
            onClick={() => onSetup(target)}
          >
            {action.local ? "Open Local models" : "Open Accounts"}
          </button>
        )}
      </div>
    </div>
  );
}
