"use client";

import { FormEvent, useEffect, useRef, useState } from "react";

import {
  DarwinCoreSearchFilters,
  type DarwinCoreSearchFilter,
  activeDarwinCoreSearchFilters,
  emptyDarwinCoreSearchFilter,
} from "@/components/darwin-core-search-filters";
import { apiFetch } from "@/lib/api";

const MAPLIBRE_VERSION = "6.6.0";
const MAPLIBRE_MODULE_URL = `https://unpkg.com/maplibre-gl@${MAPLIBRE_VERSION}/dist/maplibre-gl.mjs`;
const MAPLIBRE_CSS_URL = `https://unpkg.com/maplibre-gl@${MAPLIBRE_VERSION}/dist/maplibre-gl.css`;
const DEFAULT_MAP_STYLE_URL = "https://tiles.openfreemap.org/styles/liberty";
const MAP_STYLE_URL = process.env.NEXT_PUBLIC_MAP_STYLE_URL ?? DEFAULT_MAP_STYLE_URL;

interface OccurrenceMapFeatureCollection {
  type: "FeatureCollection";
  features: OccurrenceMapFeature[];
}

interface OccurrenceMapFeature {
  type: "Feature";
  id: string;
  geometry: {
    type: "Point";
    coordinates: [number, number];
  };
  properties: OccurrenceMapProperties;
}

interface OccurrenceMapProperties {
  occurrenceId: string;
  occurrenceUri: string;
  scientificName: string | null;
  eventDate: string | null;
  locality: string | null;
  municipality: string | null;
  county: string | null;
  stateProvince: string | null;
  country: string | null;
  coordinateSource: "original" | "nominatim";
}

interface OccurrenceCoordinateIndex {
  groupsByCoordinate: Map<string, OccurrenceMapFeature[]>;
  coordinateKeyByOccurrenceId: Map<string, string>;
}

interface MapLibrePopup {
  setLngLat(coordinates: [number, number]): MapLibrePopup;
  setDOMContent(node: Node): MapLibrePopup;
  addTo(map: MapLibreMap): MapLibrePopup;
}

interface MapLibreFeature {
  properties?: Record<string, unknown>;
  geometry?: {
    type?: string;
    coordinates?: unknown;
  };
}

interface MapLibreLayerEvent {
  features?: MapLibreFeature[];
}

interface MapLibreCanvas {
  style: {
    cursor: string;
  };
}

interface MapLibreMap {
  on(event: string, listener: () => void): void;
  on(event: string, layerId: string, listener: (event: MapLibreLayerEvent) => void): void;
  addSource(id: string, source: Record<string, unknown>): void;
  addLayer(layer: Record<string, unknown>): void;
  fitBounds(bounds: [[number, number], [number, number]], options: Record<string, unknown>): void;
  jumpTo(options: Record<string, unknown>): void;
  getCanvas(): MapLibreCanvas;
  resize(): void;
  remove(): void;
}

interface MapLibreApi {
  Map: new (options: Record<string, unknown>) => MapLibreMap;
  Popup: new (options?: Record<string, unknown>) => MapLibrePopup;
}

declare global {
  interface Window {
    __bioDatabaseMapLibre?: MapLibreApi;
  }
}

let mapLibrePromise: Promise<MapLibreApi> | null = null;

function loadMapLibre(): Promise<MapLibreApi> {
  if (typeof window === "undefined") {
    return Promise.reject(new Error("MapLibre can only be loaded in the browser"));
  }
  if (window.__bioDatabaseMapLibre) return Promise.resolve(window.__bioDatabaseMapLibre);
  if (mapLibrePromise) return mapLibrePromise;

  mapLibrePromise = new Promise<MapLibreApi>((resolve, reject) => {
    if (!document.querySelector(`link[data-maplibre-version="${MAPLIBRE_VERSION}"]`)) {
      const link = document.createElement("link");
      link.rel = "stylesheet";
      link.href = MAPLIBRE_CSS_URL;
      link.dataset.maplibreVersion = MAPLIBRE_VERSION;
      document.head.appendChild(link);
    }

    const readyEvent = `bio-database-maplibre-ready-${MAPLIBRE_VERSION}`;
    const errorEvent = `bio-database-maplibre-error-${MAPLIBRE_VERSION}`;
    const onReady = () => {
      cleanup();
      if (window.__bioDatabaseMapLibre) resolve(window.__bioDatabaseMapLibre);
      else reject(new Error("MapLibre module did not initialize"));
    };
    const onError = () => {
      cleanup();
      reject(new Error("Failed to load MapLibre"));
    };
    const cleanup = () => {
      window.removeEventListener(readyEvent, onReady);
      window.removeEventListener(errorEvent, onError);
    };
    window.addEventListener(readyEvent, onReady);
    window.addEventListener(errorEvent, onError);

    if (!document.querySelector(`script[data-maplibre-version="${MAPLIBRE_VERSION}"]`)) {
      const script = document.createElement("script");
      script.type = "module";
      script.dataset.maplibreVersion = MAPLIBRE_VERSION;
      script.textContent = `
        import * as maplibregl from ${JSON.stringify(MAPLIBRE_MODULE_URL)};
        window.__bioDatabaseMapLibre = maplibregl;
        window.dispatchEvent(new Event(${JSON.stringify(readyEvent)}));
      `;
      script.addEventListener("error", () => window.dispatchEvent(new Event(errorEvent)), {
        once: true,
      });
      document.head.appendChild(script);
    }
  });

  return mapLibrePromise;
}

export function OccurrenceMap() {
  const containerRef = useRef<HTMLDivElement>(null);
  const fullscreenRef = useRef<HTMLDivElement>(null);
  const mapRef = useRef<MapLibreMap | null>(null);
  const [filters, setFilters] = useState<DarwinCoreSearchFilter[]>([
    emptyDarwinCoreSearchFilter(),
  ]);
  const [appliedFilters, setAppliedFilters] = useState<DarwinCoreSearchFilter[]>([]);
  const [status, setStatus] = useState("地図データを読み込んでいます…");
  const [featureCount, setFeatureCount] = useState<number | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isFullscreen, setIsFullscreen] = useState(false);

  useEffect(() => {
    const handleFullscreenChange = () => {
      setIsFullscreen(document.fullscreenElement === fullscreenRef.current);
      window.requestAnimationFrame(() => mapRef.current?.resize());
    };

    document.addEventListener("fullscreenchange", handleFullscreenChange);
    return () => document.removeEventListener("fullscreenchange", handleFullscreenChange);
  }, []);

  useEffect(() => {
    let active = true;
    let map: MapLibreMap | null = null;

    async function initialize() {
      setIsLoading(true);
      setStatus("地図データを読み込んでいます…");

      try {
        const [maplibregl, data] = await Promise.all([
          loadMapLibre(),
          apiFetch<OccurrenceMapFeatureCollection>("/occurrences/map/search", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ filters: appliedFilters }),
          }),
        ]);
        if (!active || !containerRef.current) return;

        setFeatureCount(data.features.length);
        const featureIndex = indexFeaturesByExactCoordinates(data.features);
        map = new maplibregl.Map({
          container: containerRef.current,
          style: MAP_STYLE_URL,
          center: [138, 36],
          zoom: 4,
        });
        mapRef.current = map;

        map.on("load", () => {
          if (!map || !active) return;
          map.addSource("occurrences", {
            type: "geojson",
            data,
          });
          map.addLayer({
            id: "occurrences-original",
            type: "circle",
            source: "occurrences",
            filter: ["==", ["get", "coordinateSource"], "original"],
            paint: {
              "circle-radius": 6,
              "circle-color": "#176b57",
              "circle-stroke-color": "#ffffff",
              "circle-stroke-width": 1.5,
            },
          });
          map.addLayer({
            id: "occurrences-nominatim",
            type: "circle",
            source: "occurrences",
            filter: ["==", ["get", "coordinateSource"], "nominatim"],
            paint: {
              "circle-radius": 8,
              "circle-color": "#d97706",
              "circle-opacity": 0.62,
              "circle-stroke-color": "#92400e",
              "circle-stroke-width": 2,
            },
          });

          for (const layerId of ["occurrences-original", "occurrences-nominatim"]) {
            map.on("click", layerId, (event) =>
              showOccurrencePopup(maplibregl, map, event, featureIndex),
            );
            map.on("mouseenter", layerId, () => {
              if (map) map.getCanvas().style.cursor = "pointer";
            });
            map.on("mouseleave", layerId, () => {
              if (map) map.getCanvas().style.cursor = "";
            });
          }

          focusFeatures(map, data.features);
          setStatus(data.features.length === 0 ? "条件に一致する座標付きOccurrenceはありません。" : "");
          setIsLoading(false);
        });
      } catch {
        if (active) {
          setStatus("地図データの読み込みに失敗しました。");
          setIsLoading(false);
        }
      }
    }

    void initialize();

    return () => {
      active = false;
      if (mapRef.current === map) mapRef.current = null;
      map?.remove();
    };
  }, [appliedFilters]);

  function applyFilters(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setAppliedFilters(activeDarwinCoreSearchFilters(filters));
  }

  function clearFilters() {
    setFilters([emptyDarwinCoreSearchFilter()]);
    setAppliedFilters([]);
  }

  async function toggleFullscreen() {
    const element = fullscreenRef.current;
    if (!element) return;

    try {
      if (document.fullscreenElement === element) {
        await document.exitFullscreen();
      } else {
        await element.requestFullscreen();
      }
    } catch {
      setStatus("全画面表示への切り替えに失敗しました。");
    }
  }

  return (
    <div className="space-y-4">
      <form className="space-y-3 rounded-lg border border-[#d8dfe2] bg-[#f8faf9] p-4" onSubmit={applyFilters}>
        <div>
          <h2 className="text-sm font-semibold">地図を絞り込む</h2>
          <p className="mt-1 text-xs text-[#65747a]">
            データ検索と同じDarwin Core条件を使用します。複数条件はANDです。
          </p>
        </div>
        <DarwinCoreSearchFilters disabled={isLoading} filters={filters} onChange={setFilters} />
        <div className="flex gap-3">
          <button
            className="h-10 rounded-md bg-[#176b57] px-5 text-sm font-medium text-white hover:bg-[#125746] disabled:cursor-not-allowed disabled:bg-[#829b95]"
            disabled={isLoading}
            type="submit"
          >
            地図に適用
          </button>
          <button
            className="h-10 rounded-md border border-[#b8c3c8] bg-white px-4 text-sm font-medium hover:bg-[#eef2f3] disabled:cursor-not-allowed"
            disabled={isLoading}
            onClick={clearFilters}
            type="button"
          >
            条件をクリア
          </button>
        </div>
      </form>

      <div className="flex flex-wrap items-center justify-between gap-3 text-sm text-[#526168]">
        <div className="flex flex-wrap gap-5">
          <span className="inline-flex items-center gap-2">
            <span className="h-3 w-3 rounded-full bg-[#176b57]" aria-hidden="true" />
            元データの座標
          </span>
          <span className="inline-flex items-center gap-2">
            <span className="h-4 w-4 rounded-full border-2 border-[#92400e] bg-[#d97706]/60" aria-hidden="true" />
            Nominatimで地名から取得
          </span>
        </div>
        {featureCount !== null ? <span>{featureCount} 件表示</span> : null}
      </div>

      <div
        ref={fullscreenRef}
        className={
          isFullscreen
            ? "relative h-screen w-screen overflow-hidden bg-[#eef2f3]"
            : "relative overflow-hidden rounded-lg border border-[#d8dfe2] bg-[#eef2f3]"
        }
      >
        <div
          ref={containerRef}
          className={isFullscreen ? "h-full w-full" : "h-[640px] min-h-[420px] w-full"}
          aria-label="Occurrence map"
        />
        <button
          aria-label={isFullscreen ? "全画面表示を終了" : "地図を全画面表示"}
          className="absolute right-3 top-3 z-10 rounded-md border border-[#b8c3c8] bg-white/95 px-3 py-2 text-sm font-medium text-[#263238] shadow-sm hover:bg-white"
          onClick={() => void toggleFullscreen()}
          type="button"
        >
          {isFullscreen ? "全画面を終了" : "全画面表示"}
        </button>
        {status ? (
          <div className="pointer-events-none absolute left-1/2 top-4 -translate-x-1/2 rounded-md bg-white/95 px-4 py-2 text-sm shadow-sm">
            {status}
          </div>
        ) : null}
      </div>

      <p className="text-sm text-[#65747a]">
        Nominatim由来の点は地名から生成した座標です。正確な採集地点を意味しません。
      </p>
    </div>
  );
}

function focusFeatures(map: MapLibreMap, features: OccurrenceMapFeature[]) {
  if (features.length === 0) return;
  if (features.length === 1) {
    map.jumpTo({ center: features[0].geometry.coordinates, zoom: 8 });
    return;
  }

  let minLongitude = 180;
  let minLatitude = 90;
  let maxLongitude = -180;
  let maxLatitude = -90;
  for (const feature of features) {
    const [longitude, latitude] = feature.geometry.coordinates;
    minLongitude = Math.min(minLongitude, longitude);
    maxLongitude = Math.max(maxLongitude, longitude);
    minLatitude = Math.min(minLatitude, latitude);
    maxLatitude = Math.max(maxLatitude, latitude);
  }
  map.fitBounds(
    [
      [minLongitude, minLatitude],
      [maxLongitude, maxLatitude],
    ],
    { padding: 48, maxZoom: 10 },
  );
}

function showOccurrencePopup(
  maplibregl: MapLibreApi,
  map: MapLibreMap | null,
  event: MapLibreLayerEvent,
  featureIndex: OccurrenceCoordinateIndex,
) {
  if (!map) return;
  const feature = event.features?.[0];
  if (!feature?.properties) return;
  const coordinates = feature.geometry?.coordinates;
  if (!Array.isArray(coordinates) || coordinates.length < 2) return;
  const longitude = Number(coordinates[0]);
  const latitude = Number(coordinates[1]);
  if (!Number.isFinite(longitude) || !Number.isFinite(latitude)) return;

  const occurrenceId = stringProperty(feature.properties, "occurrenceId");
  const sourceCoordinateKey = occurrenceId
    ? featureIndex.coordinateKeyByOccurrenceId.get(occurrenceId)
    : undefined;
  const groupedFeatures = sourceCoordinateKey
    ? featureIndex.groupsByCoordinate.get(sourceCoordinateKey) ?? []
    : featureIndex.groupsByCoordinate.get(exactCoordinateKey([longitude, latitude])) ?? [];

  if (groupedFeatures.length >= 2) {
    const popupCoordinates = groupedFeatures[0].geometry.coordinates;
    new maplibregl.Popup({ closeButton: true })
      .setLngLat(popupCoordinates)
      .setDOMContent(createOccurrenceListPopup(groupedFeatures))
      .addTo(map);
    return;
  }

  const properties = feature.properties;
  const root = document.createElement("div");
  root.className = "min-w-52 space-y-1 text-sm";

  const title = document.createElement("strong");
  title.className = "block text-sm";
  title.textContent = stringProperty(properties, "scientificName") ?? "Occurrence";
  root.appendChild(title);

  const location = [
    stringProperty(properties, "locality"),
    stringProperty(properties, "municipality"),
    stringProperty(properties, "stateProvince"),
    stringProperty(properties, "country"),
  ]
    .filter((value): value is string => Boolean(value))
    .filter((value, index, values) => values.indexOf(value) === index)
    .join(", ");
  appendPopupRow(root, location || null);
  appendPopupRow(root, stringProperty(properties, "eventDate"));
  appendPopupRow(
    root,
    stringProperty(properties, "coordinateSource") === "nominatim"
      ? "地名からNominatimで取得"
      : "元データの座標",
  );

  if (occurrenceId) {
    const link = document.createElement("a");
    link.href = `/occurrences/${encodeURIComponent(occurrenceId)}`;
    link.textContent = "詳細を見る";
    link.className = "mt-2 inline-block font-medium text-[#176b57] underline";
    root.appendChild(link);
  }

  new maplibregl.Popup({ closeButton: true })
    .setLngLat([longitude, latitude])
    .setDOMContent(root)
    .addTo(map);
}

function indexFeaturesByExactCoordinates(
  features: OccurrenceMapFeature[],
): OccurrenceCoordinateIndex {
  const groupsByCoordinate = new Map<string, OccurrenceMapFeature[]>();
  const coordinateKeyByOccurrenceId = new Map<string, string>();

  for (const feature of features) {
    const key = exactCoordinateKey(feature.geometry.coordinates);
    const group = groupsByCoordinate.get(key);
    if (group) {
      group.push(feature);
    } else {
      groupsByCoordinate.set(key, [feature]);
    }

    if (feature.properties.occurrenceId) {
      coordinateKeyByOccurrenceId.set(feature.properties.occurrenceId, key);
    }
  }

  return { groupsByCoordinate, coordinateKeyByOccurrenceId };
}

function exactCoordinateKey(coordinates: [number, number]): string {
  return `${coordinates[0]}\u0000${coordinates[1]}`;
}

function createOccurrenceListPopup(features: OccurrenceMapFeature[]): HTMLElement {
  const root = document.createElement("div");
  root.className = "min-w-64 max-w-sm text-sm";

  const title = document.createElement("strong");
  title.className = "block text-sm";
  title.textContent = `${features.length}件のOccurrence`;
  root.appendChild(title);

  const list = document.createElement("div");
  list.className = "mt-2 max-h-72 space-y-3 overflow-y-auto pr-1";

  features.forEach((feature, index) => {
    const properties = feature.properties;
    const item = document.createElement("div");
    item.className =
      index === 0
        ? "space-y-1"
        : "space-y-1 border-t border-[#d8dfe2] pt-2";

    const itemTitle = document.createElement("strong");
    itemTitle.className = "block text-sm";
    itemTitle.textContent = properties.scientificName ?? "Occurrence";
    item.appendChild(itemTitle);

    appendPopupRow(item, occurrenceLocation(properties));
    appendPopupRow(item, properties.eventDate);
    appendPopupRow(
      item,
      properties.coordinateSource === "nominatim"
        ? "地名からNominatimで取得"
        : "元データの座標",
    );

    if (properties.occurrenceId) {
      const link = document.createElement("a");
      link.href = `/occurrences/${encodeURIComponent(properties.occurrenceId)}`;
      link.textContent = "詳細を見る";
      link.className = "inline-block font-medium text-[#176b57] underline";
      item.appendChild(link);
    }

    list.appendChild(item);
  });

  root.appendChild(list);
  return root;
}

function occurrenceLocation(properties: OccurrenceMapProperties): string | null {
  const location = [
    properties.locality,
    properties.municipality,
    properties.stateProvince,
    properties.country,
  ]
    .filter((value): value is string => Boolean(value))
    .filter((value, index, values) => values.indexOf(value) === index)
    .join(", ");
  return location || null;
}

function appendPopupRow(root: HTMLElement, value: string | null) {
  if (!value) return;
  const row = document.createElement("div");
  row.textContent = value;
  root.appendChild(row);
}

function stringProperty(properties: Record<string, unknown>, key: string): string | null {
  const value = properties[key];
  return typeof value === "string" && value.length > 0 ? value : null;
}
