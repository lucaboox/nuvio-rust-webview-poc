import { useRef } from "react";
import type { CatalogSection, ContentMeta, ProgressSnapshot } from "../bridge/types";
import { WatchStatus, watchStateForContent } from "./WatchStatus";
import { showTitleContextMenu } from "./TitleContextMenu";

export function MediaRow({ section, progress, onSelect, onSeeAll }: { section: CatalogSection; progress: ProgressSnapshot; onSelect(item: ContentMeta): void; onSeeAll?(section: CatalogSection): void }) {
  const rowRef = useRef<HTMLDivElement>(null);
  const drag = useRef({ active: false, moved: false, startX: 0, startScroll: 0 });

  function beginDrag(event: React.PointerEvent<HTMLDivElement>) {
    if (event.button !== 0 || !rowRef.current) return;
    drag.current = { active: true, moved: false, startX: event.clientX, startScroll: rowRef.current.scrollLeft };
  }

  function moveDrag(event: React.PointerEvent<HTMLDivElement>) {
    if (!drag.current.active || !rowRef.current) return;
    const distance = event.clientX - drag.current.startX;
    if (Math.abs(distance) > 12) drag.current.moved = true;
    if (drag.current.moved) rowRef.current.scrollLeft = drag.current.startScroll - distance;
  }

  function endDrag() {
    drag.current.active = false;
  }

  function select(item: ContentMeta) {
    if (drag.current.moved) { drag.current.moved = false; return; }
    onSelect(item);
  }

  return <section className="media-section">
    <div className="section-heading"><div><h2>{section.title}</h2>{section.subtitle && <p>{section.subtitle}</p>}</div>{onSeeAll && <button className="text-button" onClick={() => onSeeAll(section)}>See all</button>}</div>
    <div ref={rowRef} className="media-row drag-scroll" onPointerDown={beginDrag} onPointerMove={moveDrag} onPointerUp={endDrag} onPointerLeave={endDrag} onPointerCancel={endDrag} onDragStart={(event) => event.preventDefault()}>
      {section.items.map((item) => {
        const shape = normalizedShape(item.posterShape);
        return <button className={`media-card media-card-${shape}`} key={`${item.sourceManifestUrl}:${item.id}`} onClick={() => select(item)} onContextMenu={(event) => showTitleContextMenu(event, item)}>
          <div className="poster-art real-poster" style={item.poster ? { backgroundImage: `url("${item.poster.replaceAll('"', '%22')}")` } : undefined}>{!item.poster && <span className="poster-title">{item.name}</span>}<WatchStatus state={watchStateForContent(item, progress)} /></div>
          <span className="card-title">{item.name}</span>
          <span className="card-meta">{item.releaseInfo || item.contentType}</span>
        </button>;
      })}
    </div>
  </section>;
}

function normalizedShape(shape?: string): "poster" | "square" | "landscape" {
  const value = shape?.toLowerCase();
  if (value === "landscape" || value === "wide") return "landscape";
  if (value === "square") return "square";
  return "poster";
}
