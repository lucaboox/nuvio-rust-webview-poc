/**
 * Desktop-only input conveniences for the shared web UI.
 *
 * WebView2 exposes standard Gamepad and MouseEvent APIs, so this stays in the
 * shell capability layer instead of teaching the shared browser UI about the
 * Rust host. It intentionally clicks/focuses existing controls: React remains
 * the owner of navigation and no second route model can drift out of sync.
 */

type Direction = "up" | "down" | "left" | "right";

const INTERACTIVE_SELECTOR = [
  "button:not([disabled])",
  "a[href]",
  "input:not([disabled]):not([type='hidden'])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[role='button']",
  "[role='option']",
  "[role='tab']",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

const TRANSIENT_LAYER_SELECTOR = [
  "[role='listbox']",
  ".context-menu",
  ".source-sheet",
  ".player-sources",
  ".player-episodes",
  ".player-diagnostics",
  ".external-player-menu",
  ".settings-menu",
].join(",");

const MODAL_SELECTOR = "[role='dialog'][aria-modal='true'], dialog[open]";
const CONTROLLER_CLASS = "nuvio-controller-navigation";
const FOCUS_STYLE_ID = "nuvio-controller-focus-style";
const STICK_DEAD_ZONE = 0.55;
const INITIAL_REPEAT_DELAY_MS = 360;
const REPEAT_INTERVAL_MS = 115;
const SEARCH_SELECTOR = ".topbar form";
let searchReturnFocus: HTMLElement | null = null;
let controllerSelect: HTMLSelectElement | null = null;

function toggleControllerSearch() {
  if (document.activeElement?.closest(SEARCH_SELECTOR)) {
    leaveControllerSearch();
    return;
  }
  // Search is behind these views; do not focus through an open overlay.
  if (topNavigationScope() !== document) return;
  const field = visibleElements(`${SEARCH_SELECTOR} input`)[0];
  if (!field) return;
  searchReturnFocus = document.activeElement instanceof HTMLElement
    ? document.activeElement : null;
  focusElement(field);
}

function leaveControllerSearch() {
  const previous = searchReturnFocus;
  searchReturnFocus = null;
  if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
  const elements = focusableElements();
  const target = previous && elements.includes(previous)
    ? previous : preferredInitialElement(elements);
  if (target) focusElement(target);
}

function installFocusStyle() {
  if (document.getElementById(FOCUS_STYLE_ID)) return;
  const style = document.createElement("style");
  style.id = FOCUS_STYLE_ID;
  style.textContent = `
    html.${CONTROLLER_CLASS} :focus {
      outline: 3px solid var(--accent, #ffffff) !important;
      outline-offset: 3px !important;
    }
    html.${CONTROLLER_CLASS} .source-main:focus {
      outline-offset: -3px !important;
      background: color-mix(in srgb, var(--accent, #ffffff) 20%, #101217) !important;
      box-shadow: inset 0 0 0 3px var(--accent, #ffffff) !important;
    }
  `;
  document.head.append(style);
}

function rectOf(element: HTMLElement) {
  return element.getBoundingClientRect();
}

function isVisible(element: HTMLElement) {
  if (element.closest("[aria-hidden='true'], [inert]")) return false;
  // Opacity is not inherited: a child of a faded-out panel still reports 1.
  for (let parent = element.parentElement; parent; parent = parent.parentElement) {
    const parentStyle = getComputedStyle(parent);
    if (parentStyle.visibility === "hidden" || Number(parentStyle.opacity) === 0) return false;
  }
  const style = window.getComputedStyle(element);
  if (
    style.display === "none" ||
    style.visibility === "hidden" ||
    style.pointerEvents === "none" ||
    Number(style.opacity) === 0
  ) {
    return false;
  }
  const rect = rectOf(element);
  return rect.width > 1 && rect.height > 1;
}

function visibleElements(selector: string, root: ParentNode = document) {
  return Array.from(root.querySelectorAll<HTMLElement>(selector)).filter(
    isVisible,
  );
}

function topNavigationScope(): ParentNode {
  // Details/person/player are overlays, even though they have no dialog role.
  // Never let navigation reach the still-mounted sidebar underneath them.
  const views = visibleElements(".detail-view, .person-view, .player-view");
  const view = views.sort((a, b) =>
    (Number.parseInt(getComputedStyle(a).zIndex) || 0) -
    (Number.parseInt(getComputedStyle(b).zIndex) || 0),
  ).at(-1);
  const layers = visibleElements(`${MODAL_SELECTOR}, ${TRANSIENT_LAYER_SELECTOR}`)
    .filter((layer) => !view || view.contains(layer) || !layer.closest(".detail-view, .person-view, .player-view"));
  // Portalled dropdowns can sit above dialogs; use actual hit testing to find
  // the uppermost layer instead of assuming DOM order equals paint order.
  const exposed = layers.filter((layer) => {
    const rect = rectOf(layer);
    const x = Math.max(0, Math.min(innerWidth - 1, rect.left + rect.width / 2));
    const y = Math.max(0, Math.min(innerHeight - 1, rect.top + rect.height / 2));
    const hit = document.elementFromPoint(x, y);
    return hit !== null && layer.contains(hit);
  });
  return exposed.at(-1) ?? view ?? document;
}

function focusableElements() {
  const scope = topNavigationScope();
  const elements = visibleElements(INTERACTIVE_SELECTOR, scope);
  if (scope instanceof HTMLElement && scope.matches(INTERACTIVE_SELECTOR)) {
    elements.unshift(scope);
  }
  return [...new Set(elements)].filter((element) =>
    !element.matches("[disabled], [aria-disabled='true']") && element.tabIndex >= 0 &&
    (!element.closest(SEARCH_SELECTOR) || Boolean(document.activeElement?.closest(SEARCH_SELECTOR))),
  );
}

function focusElement(element: HTMLElement) {
  installFocusStyle();
  document.documentElement.classList.add(CONTROLLER_CLASS);
  element.focus({ preventScroll: true });
  element.scrollIntoView({
    behavior: "instant",
    block: "nearest",
    inline: "nearest",
  });
}

function preferredInitialElement(elements: HTMLElement[]) {
  const selected = document.querySelector<HTMLElement>(
    "[aria-current='page'], [aria-selected='true'], nav .active, .sidebar .active",
  );
  const selectedControl = selected?.matches(INTERACTIVE_SELECTOR)
    ? selected
    : selected?.closest<HTMLElement>(INTERACTIVE_SELECTOR);
  if (selectedControl && elements.includes(selectedControl)) return selectedControl;

  return elements
    .filter((element) => {
      const rect = rectOf(element);
      return (
        rect.bottom > 0 &&
        rect.right > 0 &&
        rect.top < window.innerHeight &&
        rect.left < window.innerWidth
      );
    })
    .sort((a, b) => {
      const aRect = rectOf(a);
      const bRect = rectOf(b);
      return aRect.top - bRect.top || aRect.left - bRect.left;
    })[0];
}

function crossAxisGap(
  current: DOMRect,
  candidate: DOMRect,
  direction: Direction,
) {
  if (direction === "left" || direction === "right") {
    if (candidate.bottom >= current.top && candidate.top <= current.bottom) return 0;
    return candidate.top > current.bottom
      ? candidate.top - current.bottom
      : current.top - candidate.bottom;
  }
  if (candidate.right >= current.left && candidate.left <= current.right) return 0;
  return candidate.left > current.right
    ? candidate.left - current.right
    : current.left - candidate.right;
}

function directionalDistance(
  current: DOMRect,
  candidate: DOMRect,
  direction: Direction,
) {
  const currentX = current.left + current.width / 2;
  const currentY = current.top + current.height / 2;
  const candidateX = candidate.left + candidate.width / 2;
  const candidateY = candidate.top + candidate.height / 2;
  const dx = candidateX - currentX;
  const dy = candidateY - currentY;
  const primary =
    direction === "left"
      ? -dx
      : direction === "right"
        ? dx
        : direction === "up"
          ? -dy
          : dy;
  if (primary <= 2) return Number.POSITIVE_INFINITY;

  // Strongly prefer staying in the same visual row/column. This makes poster
  // shelves feel predictable while still allowing movement between sections.
  const cross = direction === "left" || direction === "right" ? Math.abs(dy) : Math.abs(dx);
  const gap = crossAxisGap(current, candidate, direction);
  // Keep a row/column when available, then prefer the closest aligned center.
  // Center distance breaks the old ties between differently sized controls.
  return primary + cross * 0.35 + gap * 6 + (gap > 0 ? 500 : 0);
}

function nearestScrollable(element: HTMLElement, direction: Direction) {
  let node: HTMLElement | null = element.parentElement;
  while (node && node !== document.body) {
    const style = getComputedStyle(node);
    const canScroll =
      direction === "left" || direction === "right"
        ? /(auto|scroll)/.test(style.overflowX) && node.scrollWidth > node.clientWidth
        : /(auto|scroll)/.test(style.overflowY) && node.scrollHeight > node.clientHeight;
    if (canScroll) return node;
    node = node.parentElement;
  }
  return document.scrollingElement instanceof HTMLElement
    ? document.scrollingElement
    : null;
}

function moveFocus(direction: Direction) {
  const elements = focusableElements();
  if (elements.length === 0) return;

  const active =
    document.activeElement instanceof HTMLElement &&
    elements.includes(document.activeElement)
      ? document.activeElement
      : null;
  if (!active) {
    const initial = preferredInitialElement(elements);
    if (initial) focusElement(initial);
    return;
  }

  const currentRect = rectOf(active);
  // Source rows have side actions of different heights. Up/down should follow
  // the main stream buttons, leaving side actions for left/right navigation.
  const sourceRows = active.matches(".source-main") && (direction === "up" || direction === "down")
    ? elements.filter((element) => element.matches(".source-main") &&
        Number.isFinite(directionalDistance(currentRect, rectOf(element), direction)))
    : [];
  const candidate = (sourceRows.length ? sourceRows : elements)
    .filter((element) => element !== active)
    .map((element) => ({
      element,
      distance: directionalDistance(currentRect, rectOf(element), direction),
    }))
    .filter(({ distance }) => Number.isFinite(distance))
    .sort((a, b) => a.distance - b.distance)[0]?.element;

  if (candidate) {
    focusElement(candidate);
    return;
  }

  // Virtualized grids may not mount the next row until their scroller moves.
  const scroller = nearestScrollable(active, direction);
  if (!scroller) return;
  const horizontal = direction === "left" || direction === "right";
  scroller.scrollBy({
    left: horizontal
      ? scroller.clientWidth * (direction === "left" ? -0.7 : 0.7)
      : 0,
    top: horizontal
      ? 0
      : scroller.clientHeight * (direction === "up" ? -0.7 : 0.7),
    behavior: "smooth",
  });
}

function activateFocusedElement() {
  const active = document.activeElement;
  const elements = focusableElements();
  if (!(active instanceof HTMLElement) || !elements.includes(active)) {
    const initial = preferredInitialElement(elements);
    if (initial) focusElement(initial);
    return;
  }
  const previousScope = topNavigationScope();
  const previousFirst = focusableElements()[0];
  if (active instanceof HTMLSelectElement) {
    controllerSelect = active;
    // Shared Select opens on Enter/mousedown; .click() alone never opens it.
    active.dispatchEvent(new KeyboardEvent("keydown", {
      key: "Enter", code: "Enter", bubbles: true, cancelable: true,
    }));
  } else active.click();
  // React commits the newly opened menu after the activation handler returns.
  requestAnimationFrame(() => requestAnimationFrame(() => {
    const scope = topNavigationScope();
    if (scope === document) return;
    const options = focusableElements();
    if (scope === previousScope && options[0] === previousFirst) return;
    const selected = options.find((element) => element.getAttribute("aria-selected") === "true");
    const target = selected ?? options[0];
    if (target) focusElement(target);
  }));
}

function dispatchEscape() {
  window.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Escape",
      code: "Escape",
      bubbles: true,
      cancelable: true,
    }),
  );
}

function firstVisible(root: ParentNode, selectors: string) {
  return visibleElements(selectors, root)[0];
}

function navigateBack() {
  if (document.activeElement?.closest(SEARCH_SELECTOR)) {
    leaveControllerSearch();
    return;
  }
  const listbox = visibleElements("[role='listbox'], .context-menu").at(-1);
  if (listbox) {
    if (listbox.matches("[role='listbox']") && controllerSelect?.isConnected) {
      controllerSelect.dispatchEvent(new KeyboardEvent("keydown", {
        key: "Escape", bubbles: true, cancelable: true,
      }));
      focusElement(controllerSelect);
      return;
    }
    dispatchEscape();
    return;
  }

  const modal = visibleElements(MODAL_SELECTOR).at(-1);
  if (modal) {
    const close = firstVisible(
      modal,
      "button[aria-label*='close' i], button[aria-label*='dismiss' i], button:has(svg.lucide-x)",
    );
    if (close) close.click();
    else dispatchEscape();
    return;
  }

  const transient = visibleElements(TRANSIENT_LAYER_SELECTOR).at(-1);
  if (transient) {
    if (transient.matches(".settings-menu")) {
      const back = firstVisible(transient, ".settings-back");
      const gear = transient.parentElement?.querySelector<HTMLElement>("button[aria-expanded='true']");
      const target = back ?? gear;
      if (target) {
        target.click();
        requestAnimationFrame(() => {
          const first = visibleElements(".settings-menu button")[0];
          if (first ?? gear) focusElement((first ?? gear)!);
        });
      }
      return;
    }
    const close = firstVisible(
      transient,
      [
        "button.source-sheet-back",
        "button[aria-label*='close' i]",
        "button[aria-label*='back' i]",
        "button:has(svg.lucide-x)",
        "button:has(svg.lucide-arrow-left)",
      ].join(","),
    );
    if (close) close.click();
    else dispatchEscape();
    return;
  }

  const view = visibleElements(
    ".player-view, .person-view, .detail-view, .grid-page, .settings-page",
  ).at(-1);
  const back = firstVisible(
    view ?? document,
    [
      "button.back",
      "button[aria-label*='back' i]",
      "button[title*='back' i]",
      "button:has(svg.lucide-arrow-left)",
    ].join(","),
  );
  if (back) {
    back.click();
    return;
  }

  window.history.back();
}

function navigateForward() {
  window.history.forward();
}

let mouseDownHandledButton = -1;

function handleNavigationMouseButton(event: MouseEvent) {
  if (event.button !== 3 && event.button !== 4) return;
  event.preventDefault();
  event.stopPropagation();

  // Chromium normally emits mousedown and auxclick for one side-button press.
  // Treat auxclick as a fallback, not a second navigation. The marker resets on
  // the next mousedown, so rapid intentional presses are never rate-limited.
  if (event.type === "auxclick" && mouseDownHandledButton === event.button) {
    mouseDownHandledButton = -1;
    return;
  }
  if (event.type === "mousedown") mouseDownHandledButton = event.button;
  if (event.button === 3) navigateBack();
  else navigateForward();
}

document.addEventListener("mousedown", handleNavigationMouseButton, true);
document.addEventListener("auxclick", handleNavigationMouseButton, true);
document.addEventListener(
  "pointerdown",
  (event) => {
    if (event.button < 3) {
      document.documentElement.classList.remove(CONTROLLER_CLASS);
    }
  },
  true,
);

let nativeSurfaceClickTimer: number | undefined;

function nativePlayerSurfaceAt(target: EventTarget | null) {
  if (!(target instanceof Element)) return null;
  const player = target.closest<HTMLElement>(".player-view.native-player");
  if (!player) return null;
  if (
    target.closest(
      [
        "button",
        "a",
        "input",
        "select",
        "textarea",
        "[role='button']",
        "[role='listbox']",
        ".player-controls",
        ".player-top",
        ".player-sources",
        ".player-episodes",
        ".player-diagnostics",
        ".settings-menu",
        ".external-player-menu",
      ].join(","),
    )
  ) {
    return null;
  }
  return player;
}

document.addEventListener("click", (event) => {
  if (!nativePlayerSurfaceAt(event.target)) return;
  window.clearTimeout(nativeSurfaceClickTimer);
  nativeSurfaceClickTimer = undefined;
  // The second click is followed by dblclick, which owns fullscreen.
  if (event.detail > 1) return;
  nativeSurfaceClickTimer = window.setTimeout(() => {
    nativeSurfaceClickTimer = undefined;
    playerShortcut("k");
  }, 220);
});

document.addEventListener("dblclick", (event) => {
  if (!nativePlayerSurfaceAt(event.target)) return;
  window.clearTimeout(nativeSurfaceClickTimer);
  nativeSurfaceClickTimer = undefined;
  playerShortcut("f");
});

let pollingGamepads = false;
let previousAccept = false;
let previousBack = false;
let previousStart = false;
let previousSelect = false;
let previousSeekModifier = false;

function playerShortcut(key: string) {
  window.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
}

function controllerDirection(direction: Direction, seeking: boolean) {
  if (seeking) {
    if (direction === "left" || direction === "right") {
      playerShortcut(direction === "left" ? "ArrowLeft" : "ArrowRight");
    }
  } else moveFocus(direction);
}
let repeatedDirection: Direction | null = null;
let nextDirectionAt = 0;

function pressed(gamepad: Gamepad, button: number) {
  return gamepad.buttons[button]?.pressed || gamepad.buttons[button]?.value > 0.5;
}

function connectedGamepads() {
  if (typeof navigator.getGamepads !== "function") return [];
  return Array.from(navigator.getGamepads()).filter(
    (candidate): candidate is Gamepad => candidate?.connected === true,
  );
}

function gamepadDirection(gamepad: Gamepad): Direction | null {
  if (pressed(gamepad, 12)) return "up";
  if (pressed(gamepad, 13)) return "down";
  if (pressed(gamepad, 14)) return "left";
  if (pressed(gamepad, 15)) return "right";

  const x = gamepad.axes[0] ?? 0;
  const y = gamepad.axes[1] ?? 0;
  if (Math.abs(x) < STICK_DEAD_ZONE && Math.abs(y) < STICK_DEAD_ZONE) return null;
  if (Math.abs(x) > Math.abs(y)) return x < 0 ? "left" : "right";
  return y < 0 ? "up" : "down";
}

function pollGamepads(now: number) {
  const gamepad = connectedGamepads()[0];
  if (!gamepad) {
    pollingGamepads = false;
    previousAccept = false;
    previousBack = false;
    previousStart = false;
    previousSelect = false;
    repeatedDirection = null;
    return;
  }

  if (!document.hasFocus() || document.hidden) {
    previousAccept = pressed(gamepad, 0);
    previousBack = pressed(gamepad, 1);
    previousStart = pressed(gamepad, 9);
    previousSelect = pressed(gamepad, 8);
    repeatedDirection = null;
    requestAnimationFrame(pollGamepads);
    return;
  }

  const accept = pressed(gamepad, 0);
  const start = pressed(gamepad, 9);
  const select = pressed(gamepad, 8);
  const player = visibleElements(".player-view")[0];
  const direction = gamepadDirection(gamepad);
  const seeking = Boolean(player) && pressed(gamepad, 3);
  if (player && (direction || accept || start || select || pressed(gamepad, 1))) {
    player.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, pointerType: "mouse" }));
  }
  if (start && !previousStart) {
    if (player) playerShortcut("k");
    else toggleControllerSearch();
  }
  if (player && select && !previousSelect) playerShortcut("f");
  previousSelect = select;
  previousStart = start;
  const back = pressed(gamepad, 1);
  if (accept && !previousAccept) activateFocusedElement();
  if (back && !previousBack) navigateBack();
  previousAccept = accept;
  previousBack = back;

  if (seeking !== previousSeekModifier) repeatedDirection = null;
  previousSeekModifier = seeking;
  if (direction !== repeatedDirection) {
    repeatedDirection = direction;
    nextDirectionAt = now + INITIAL_REPEAT_DELAY_MS;
    if (direction) controllerDirection(direction, seeking);
  } else if (direction && now >= nextDirectionAt) {
    controllerDirection(direction, seeking);
    nextDirectionAt = now + (seeking ? 300 : REPEAT_INTERVAL_MS);
  }

  requestAnimationFrame(pollGamepads);
}

function startGamepadPolling() {
  if (pollingGamepads) return;
  pollingGamepads = true;
  requestAnimationFrame(pollGamepads);
}

window.addEventListener("gamepadconnected", startGamepadPolling);
if (connectedGamepads().length > 0) {
  startGamepadPolling();
}
