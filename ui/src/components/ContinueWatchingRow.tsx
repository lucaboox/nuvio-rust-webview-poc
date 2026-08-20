import { useRef } from "react";
import type { ContentMeta, SettingsSnapshot } from "../bridge/types";
import {
  artworkForContinueWatching,
  continueWatchingPreferences,
  type ContinueWatchingCard,
  progressForCard,
  remainingLabel,
  splitContinueWatching,
} from "../data/continueWatching";
import { showTitleContextMenu } from "./TitleContextMenu";

type Props = {
  cards: ContinueWatchingCard[];
  settings: SettingsSnapshot | null;
  onSelect(item: ContentMeta): void;
};

export function ContinueWatchingRow({ cards, settings, onSelect }: Props) {
  const preferences = continueWatchingPreferences(settings);
  if (!preferences.continueWatchingVisible) return null;
  const { current, upcoming } = splitContinueWatching(
    cards,
    preferences.continueWatchingSortMode,
  );
  return (
    <>
      <ContinueRow
        title="Continue watching"
        cards={current}
        settings={preferences}
        onSelect={onSelect}
      />
      <ContinueRow
        title="Upcoming"
        cards={upcoming}
        settings={preferences}
        onSelect={onSelect}
      />
    </>
  );
}

function ContinueRow({
  title,
  cards,
  settings,
  onSelect,
}: {
  title: string;
  cards: ContinueWatchingCard[];
  settings: ReturnType<typeof continueWatchingPreferences>;
  onSelect(item: ContentMeta): void;
}) {
  const row = useRef<HTMLDivElement>(null);
  const drag = useRef({ active: false, moved: false, x: 0, scroll: 0 });
  if (!cards.length) return null;
  const style = settings.continueWatchingStyle.toLowerCase();
  return (
    <section className="media-section continue-watching-section">
      <div className="section-heading">
        <h2>{title}</h2>
      </div>
      <div
        ref={row}
        className="continue-watching-row drag-scroll"
        onPointerDown={(event) => {
          if (event.button === 0 && row.current)
            drag.current = {
              active: true,
              moved: false,
              x: event.clientX,
              scroll: row.current.scrollLeft,
            };
        }}
        onPointerMove={(event) => {
          if (!drag.current.active || !row.current) return;
          const distance = event.clientX - drag.current.x;
          if (Math.abs(distance) > 12) drag.current.moved = true;
          if (drag.current.moved)
            row.current.scrollLeft = drag.current.scroll - distance;
        }}
        onPointerUp={() => {
          drag.current.active = false;
        }}
        onPointerLeave={() => {
          drag.current.active = false;
        }}
      >
        {cards.map((card) => {
          const video = card.video;
          const artwork = artworkForContinueWatching(card, settings);
          const percent = progressForCard(card);
          const selected = video
            ? { ...card.item, selectedVideoId: video.id }
            : card.item;
          const blur =
            settings.continueWatchingBlurNextUp &&
            settings.continueWatchingUseEpisodeThumbnails &&
            card.nextUp &&
            !!video?.thumbnail &&
            artwork === video.thumbnail;
          const copy = (
            <div className="continue-card-copy">
              {video?.season != null && video?.episode != null && (
                <small>
                  S{video.season} E{video.episode}
                </small>
              )}
              <strong>{card.item.name}</strong>
              {video?.title && <span>{video.title}</span>}
            </div>
          );
          return (
            <button
              className={`continue-card style-${style}`}
              key={`${card.item.id}:${video?.id || card.progress?.videoId || "next"}`}
              onClick={() => {
                if (!drag.current.moved) onSelect(selected);
                drag.current.moved = false;
              }}
              onContextMenu={(event) =>
                showTitleContextMenu(
                  event,
                  selected,
                  undefined,
                  card.nextUp
                    ? {
                        kind: "nextUp",
                        contentId: card.item.id,
                        season: card.seedSeason,
                        episode: card.seedEpisode,
                      }
                    : { kind: "resume", contentId: card.item.id },
                )
              }
            >
              <div className="continue-card-art">
                <span
                  className={`continue-card-image${blur ? " is-blurred" : ""}`}
                  style={
                    artwork
                      ? {
                          backgroundImage: `url("${artwork.replaceAll('"', "%22")}")`,
                        }
                      : undefined
                  }
                />
                <span className="continue-card-badge">
                  {card.nextUp ? "Next up" : remainingLabel(card.progress)}
                </span>
                {settings.continueWatchingStyle !== "Poster" && copy}
                {percent > 0 && (
                  <i className="continue-card-progress">
                    <b style={{ width: percent + "%" }} />
                  </i>
                )}
              </div>
              {settings.continueWatchingStyle === "Poster" && copy}
            </button>
          );
        })}
      </div>
    </section>
  );
}
