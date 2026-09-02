"use client";

import Link from "next/link";
import { useEffect, useRef, useState } from "react";

import { apiFetch } from "@/lib/api";

const MAPLIBRE_VERSION = "6.6.0";
const MAPLIBRE_MODULE_URL = `https://unpkg.com/maplibre-gl@${MAPLIBRE_VERSION}/dist/maplibre-gl.mjs`;
const MAPLIBRE_CSS_URL = `https://unpkg.com/maplibre-gl@${MAPLIBRE_VERSION}/dist/maplibre-gl.css`;
const DEFAULT_MAP_STYLE_URL = "https://demotiles.maplibre.org/style.json";
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
  const [status, setStatus] = useState("地図データを読み込んでいます…");
  const [featureCount, setFeatureCount] = useState<number | null>(null);

  useEffect(() => {
    let active = true;
    let map: MapLibreMap | null = null;

    async function initialize() {
      try {
        const [maplibregl, data] = await Promise.all([
          loadMapLibre(),
          apiFetch<OccurrenceMapFeatureCollection>("/occurrences/map"),
        ]);
        if (!active || !containerRef.current) return;

        setFeatureCount(data.features.length);
        map = new maplibregl.Map({
          container: containerRef.current,
          style: MAP_STYLE_URL,
          center: [138, 36],
          zoom: 4,
        });

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
            map.on("click", layerId, (event) => showOccurrencePopup(maplibregl, map, event));
            map.on("mouseenter", layerId, () => {
              if (map) map.getCanvas().style.cursor = "pointer";
            });
            map.on("mouseleave", layerId, () => {
              if (map) map.getCanvas().style.cursor = "";
            });
          }

          focusFeatures(map, data.features);
          setStatus(data.features.length === 0 ? "座標付きOccurrenceはありません。" : "");
        });
      } catch {
        if (active) setStatus("地図データの読み込みに失敗しました。");
      }
    }

    void initialize();

    return () => {
      active = false;
      map?.remove();
    };
  }, []);

  return (
    <div className="space-y-4">
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

      <div className="relative overflow-hidden rounded-lg border border-[#d8dfe2] bg-[#eef2f3]">
        <div ref={containerRef} className="h-[640px] min-h-[420px] w-full" aria-label="Occurrence map" />
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
) {
  if (!map) return;
  const feature = event.features?.[0];
  if (!feature?.properties) return;
  const coordinates = feature.geometry?.coordinates;
  if (!Array.isArray(coordinates) || coordinates.length < 2) return;
  const longitude = Number(coordinates[0]);
  const latitude = Number(coordinates[1]);
  if (!Number.isFinite(longitude) || !Number.isFinite(latitude)) return;

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

  const occurrenceId = stringProperty(properties, "occurrenceId");
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

// Keep Link imported in this client bundle so Next can prefetch the map page from navigation
// without requiring the popup's runtime-created anchor to depend on React state.
void Link;
