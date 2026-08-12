export type MediaCard = {
  id: string;
  title: string;
  meta: string;
  palette: [string, string];
  badge?: string;
  progress?: number;
};

export const continueWatching: MediaCard[] = [
  { id: "cw-1", title: "Signal Drift", meta: "S1 E4 · 31 min left", palette: ["#313849", "#12151c"], progress: 62 },
  { id: "cw-2", title: "Northstar", meta: "S2 E1 · 44 min left", palette: ["#203b42", "#0c171b"], progress: 24 },
  { id: "cw-3", title: "Afterlight", meta: "58 min left", palette: ["#4a2833", "#190e13"], progress: 38 },
  { id: "cw-4", title: "The Meridian", meta: "S1 E7 · 18 min left", palette: ["#493824", "#17120c"], progress: 78 },
  { id: "cw-5", title: "Glass Harbor", meta: "S3 E2 · 42 min left", palette: ["#24354d", "#0d141f"], progress: 15 },
];

export const trending: MediaCard[] = [
  { id: "tr-1", title: "Pale Horizon", meta: "2026 · Sci-Fi", palette: ["#445870", "#121923"], badge: "4K" },
  { id: "tr-2", title: "Velvet Static", meta: "2025 · Drama", palette: ["#653e52", "#20121a"], badge: "HDR" },
  { id: "tr-3", title: "Arc Seven", meta: "2026 · Thriller", palette: ["#565545", "#181812"] },
  { id: "tr-4", title: "Silent Current", meta: "2024 · Mystery", palette: ["#274c55", "#0c191c"], badge: "4K" },
  { id: "tr-5", title: "Red Province", meta: "2026 · Crime", palette: ["#5b302f", "#1c0e0e"] },
  { id: "tr-6", title: "Lunar House", meta: "2025 · Adventure", palette: ["#3f4061", "#12131f"] },
];

