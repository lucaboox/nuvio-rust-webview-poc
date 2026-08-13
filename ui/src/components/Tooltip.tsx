import { CSSProperties, useEffect, useLayoutEffect, useRef, useState } from "react";

const SHOW_DELAY_MS = 340;
const EDGE_PADDING = 8;
const GAP = 10;

type Tip = { text: string; rect: DOMRect };

/**
 * Moves a native `title` onto `data-tooltip`. Windows draws its own tooltip for
 * any element that still has a `title`, so the attribute has to be gone before
 * the OS timer fires — restyling is not an option, it is not part of the page.
 *
 * `title` is also the accessible name for an element with no text, so it is
 * re-homed to `aria-label` rather than dropped.
 */
function adopt(element: Element) {
  const title = element.getAttribute("title");
  if (title === null) return;
  const text = title.trim();
  element.removeAttribute("title");
  if (!text) return;
  element.setAttribute("data-tooltip", text);
  const named =
    element.hasAttribute("aria-label") ||
    element.hasAttribute("aria-labelledby") ||
    !!element.textContent?.trim();
  if (!named) element.setAttribute("aria-label", text);
}

function adoptTree(root: ParentNode) {
  if (root instanceof Element) adopt(root);
  root.querySelectorAll?.("[title]").forEach(adopt);
}

/**
 * One styled tooltip for the whole client. Mount once, near the app root — every
 * `title` in the tree is upgraded automatically, including ones React adds on a
 * later render.
 */
export function TooltipLayer() {
  const [tip, setTip] = useState<Tip | null>(null);
  const [style, setStyle] = useState<CSSProperties>({ opacity: 0 });
  const bubble = useRef<HTMLDivElement>(null);
  const host = useRef<Element | null>(null);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => {
    adoptTree(document.body);
    const observer = new MutationObserver((records) => {
      for (const record of records) {
        if (record.type === "attributes") {
          if (record.target instanceof Element) adopt(record.target);
          continue;
        }
        record.addedNodes.forEach((node) => {
          if (node instanceof Element) adoptTree(node);
        });
      }
    });
    observer.observe(document.body, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ["title"],
    });
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const hide = () => {
      window.clearTimeout(timer.current);
      host.current = null;
      setTip(null);
    };

    const consider = (target: EventTarget | null) => {
      if (!(target instanceof Element)) return hide();
      const next = target.closest<HTMLElement>("[data-tooltip]");
      if (!next || next.hasAttribute("data-no-tooltip")) return hide();
      // Moving between children of the same control must not restart the timer.
      if (next === host.current) return;
      const text = next.getAttribute("data-tooltip")?.trim();
      if (!text) return hide();
      window.clearTimeout(timer.current);
      host.current = next;
      setTip(null);
      timer.current = window.setTimeout(() => {
        setStyle({ opacity: 0 });
        setTip({ text, rect: next.getBoundingClientRect() });
      }, SHOW_DELAY_MS);
    };

    const onOver = (event: PointerEvent) => consider(event.target);
    const onFocus = (event: FocusEvent) => consider(event.target);
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") hide();
    };

    document.addEventListener("pointerover", onOver);
    document.addEventListener("pointerdown", hide, true);
    document.addEventListener("focusin", onFocus);
    document.addEventListener("focusout", hide);
    document.addEventListener("keydown", onKey);
    document.addEventListener("scroll", hide, true);
    window.addEventListener("blur", hide);
    return () => {
      window.clearTimeout(timer.current);
      document.removeEventListener("pointerover", onOver);
      document.removeEventListener("pointerdown", hide, true);
      document.removeEventListener("focusin", onFocus);
      document.removeEventListener("focusout", hide);
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("scroll", hide, true);
      window.removeEventListener("blur", hide);
    };
  }, []);

  // Placed after measuring so a wide tooltip near an edge clamps instead of
  // overflowing, and flips below the control when there is no room above.
  useLayoutEffect(() => {
    if (!tip || !bubble.current) return;
    const box = bubble.current.getBoundingClientRect();
    const above = tip.rect.top - box.height - GAP;
    const below = tip.rect.bottom + GAP;
    const flipped = above < EDGE_PADDING;
    const top = flipped
      ? Math.min(below, window.innerHeight - box.height - EDGE_PADDING)
      : above;
    const left = Math.min(
      Math.max(
        tip.rect.left + tip.rect.width / 2 - box.width / 2,
        EDGE_PADDING,
      ),
      Math.max(window.innerWidth - box.width - EDGE_PADDING, EDGE_PADDING),
    );
    setStyle({ top, left, opacity: 1 });
  }, [tip]);

  if (!tip) return null;
  return (
    <div ref={bubble} className="app-tooltip" role="tooltip" style={style}>
      {tip.text}
    </div>
  );
}
