import { useEffect, useMemo, useState } from "react";
import { invoke } from "../bridge/nativeBridge";
import type {
  CatalogSection,
  ContentMeta,
  MetaPerson,
  PersonCredit,
  PersonDetail,
  ProgressSnapshot,
} from "../bridge/types";
import { Icon } from "./Icon";
import { MediaRow } from "./MediaRow";

export function PersonPage({
  seed,
  progress,
  onBack,
  onSelect,
}: {
  seed: MetaPerson;
  progress: ProgressSnapshot;
  onBack(): void;
  onSelect(item: ContentMeta): void;
}) {
  const [person, setPerson] = useState<PersonDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setPerson(null);
    setError(null);
    invoke<PersonDetail>("content.personDetails", {
      personId: seed.tmdbId,
      preferCrew: prefersCrew(seed.role),
    })
      .then((result) => {
        if (active) setPerson(result);
      })
      .catch((reason) => {
        if (active)
          setError(
            reason instanceof Error ? reason.message : "Could not load this person",
          );
      });
    return () => {
      active = false;
    };
  }, [seed.tmdbId, seed.role]);

  const sections = useMemo(() => buildSections(person), [person]);

  return (
    <div className="person-page">
      <button
        className="person-back round-back-button"
        aria-label="Back"
        title="Back"
        onClick={onBack}
      >
        <Icon name="back" size={25} />
      </button>
      <header className="person-identity">
        {person?.profilePhoto || seed.photo ? (
          <img src={person?.profilePhoto || seed.photo} alt="" />
        ) : (
          <div className="person-identity-placeholder">{seed.name.slice(0, 1)}</div>
        )}
        <div>
          <span>PERSON</span>
          <h1>{person?.name || seed.name}</h1>
          <p className="person-role">{person?.knownFor || seed.role}</p>
          {person && <PersonFacts person={person} />}
          {person?.biography && <p className="person-biography">{person.biography}</p>}
        </div>
      </header>
      {!person && !error && (
        <div className="person-loading">
          <i className="loading-spinner" />
          <span>Loading filmography…</span>
        </div>
      )}
      {error && <div className="inline-error person-error">{error}</div>}
      {person && sections.length === 0 && (
        <div className="person-empty">No film or television credits were found.</div>
      )}
      <div className="person-credit-rows">
        {sections.map((section) => (
          <MediaRow
            key={section.key}
            section={section}
            progress={progress}
            onSelect={onSelect}
          />
        ))}
      </div>
    </div>
  );
}

function PersonFacts({ person }: { person: PersonDetail }) {
  const life = [formatPersonDate(person.birthday), formatPersonDate(person.deathday)]
    .filter(Boolean)
    .join(" – ");
  if (!life && !person.placeOfBirth) return null;
  return (
    <div className="person-facts">
      {life && <span>{life}</span>}
      {person.placeOfBirth && <span>{person.placeOfBirth}</span>}
    </div>
  );
}

function formatPersonDate(value?: string) {
  if (!value) return "";
  const parsed = new Date(`${value}T00:00:00`);
  return Number.isNaN(parsed.getTime())
    ? value
    : parsed.toLocaleDateString(undefined, {
        year: "numeric",
        month: "short",
        day: "numeric",
      });
}

function prefersCrew(role?: string) {
  const normalized = role?.trim().toLowerCase();
  return normalized === "creator" || normalized === "director" || normalized === "writer";
}

function buildSections(person: PersonDetail | null): CatalogSection[] {
  if (!person) return [];
  const seen = new Set<string>();
  const all = [...person.movieCredits, ...person.tvCredits].filter((credit) => {
    const key = `${credit.contentType}:${credit.id}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  const today = new Date().toISOString().slice(0, 10);
  const groups = [
    {
      key: "popular",
      title: "Popular",
      items: [...all].sort((left, right) => (right.popularity ?? 0) - (left.popularity ?? 0)),
    },
    {
      key: "latest",
      title: "Latest",
      items: all
        .filter((credit) => credit.rawReleaseDate && credit.rawReleaseDate <= today)
        .sort((left, right) => (right.rawReleaseDate || "").localeCompare(left.rawReleaseDate || "")),
    },
    {
      key: "upcoming",
      title: "Upcoming",
      items: all
        .filter((credit) => credit.rawReleaseDate && credit.rawReleaseDate > today)
        .sort((left, right) => (left.rawReleaseDate || "").localeCompare(right.rawReleaseDate || "")),
    },
  ];
  return groups
    .filter((group) => group.items.length > 0)
    .map((group) => ({
      key: `person:${person.tmdbId}:${group.key}`,
      prefKey: `person:${person.tmdbId}:${group.key}`,
      title: group.title,
      subtitle: group.key === "popular" ? person.knownFor || "Film and television" : "",
      manifestUrl: "",
      contentType: "mixed",
      catalogId: group.key,
      items: group.items.map(creditToContentMeta),
    }));
}

function creditToContentMeta(credit: PersonCredit): ContentMeta {
  return {
    id: credit.id,
    contentType: credit.contentType,
    name: credit.name,
    poster: credit.poster,
    background: credit.background,
    banner: credit.background,
    description: credit.description,
    releaseInfo: credit.releaseInfo,
    released: credit.rawReleaseDate,
    genres: [],
    cast: [],
    director: [],
    writer: [],
    trailers: [],
    externalRatings: [],
    hasScheduledVideos: false,
    videos: [],
    sourceManifestUrl: "",
    addonName: "TMDB",
  };
}
